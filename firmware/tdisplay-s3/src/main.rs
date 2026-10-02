//! Internet-status display for the LilyGO T-Display-S3.
//!
//! Reads the one-letter status protocol (see ../../PROTOCOL.md) from the router
//! over USB serial and shows it on the ST7789 LCD. When online it renders an
//! animated blue flame; the other states are static colour + label screens.
//! The router side (`router/netled`) is shared with the WS2812 board, unchanged.
//! `S` lines from netled (PROTOCOL.md) set the backlight brightness and the
//! flame parameters live, from the router's /etc/status-light.json.
//!
//! Build/flash notes are in README.md. Builds clean against esp-hal 1.2.2 on the
//! `esp` toolchain.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor, WebColors};
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Text};
use embedded_io_async::Read;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::usb::usb_serial_jtag::UsbSerialJtag;
use flame::{Flame, FlameParams};
use lilygo_t_display_s3::{Board, FrameBuffer, HEIGHT, Lcd, WIDTH, resources};

esp_bootloader_esp_idf::esp_app_desc!();

const W: usize = WIDTH as usize; // 320
const H: usize = HEIGHT as usize; // 170

/// No command for this long means the router hung or netled died (PROTOCOL.md).
const WATCHDOG: Duration = Duration::from_secs(60);
/// Frame pacing for the flame; the flush overlaps the next frame's drawing.
const FRAME_DT: Duration = Duration::from_millis(33);
/// While showing a static screen, wake this often to re-check the watchdog.
const IDLE_TICK: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, PartialEq)]
enum State {
    Boot,
    Online,
    Degraded,
    Offline,
    Green,
    White,
    Off,
    NoSignal,
}

impl State {
    fn from_cmd(c: u8) -> Option<Self> {
        match c.to_ascii_uppercase() {
            b'B' => Some(Self::Online),
            b'A' => Some(Self::Degraded),
            b'R' => Some(Self::Offline),
            b'G' => Some(Self::Green),
            b'W' => Some(Self::White),
            b'O' => Some(Self::Off),
            _ => None,
        }
    }

    fn color(self) -> Rgb565 {
        match self {
            Self::Boot => Rgb565::CSS_DIM_GRAY,
            Self::Online => Rgb565::CSS_DODGER_BLUE, // unused: online draws the flame
            Self::Degraded => Rgb565::CSS_ORANGE,
            Self::Offline | Self::NoSignal => Rgb565::CSS_DARK_RED,
            Self::Green => Rgb565::GREEN,
            Self::White => Rgb565::WHITE,
            Self::Off => Rgb565::BLACK,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Boot => "Waiting for router",
            Self::Online => "ONLINE",
            Self::Degraded => "Reconnecting",
            Self::Offline => "OFFLINE",
            Self::Green | Self::White => "TEST",
            Self::Off => "",
            Self::NoSignal => "NO SIGNAL FROM ROUTER",
        }
    }
}

fn render_static(frame: &mut FrameBuffer, display: &mut Lcd, state: State) {
    let _ = frame.clear(state.color());
    let label = state.label();
    if !label.is_empty() {
        let fg = if state == State::White {
            Rgb565::BLACK
        } else {
            Rgb565::WHITE
        };
        let center = Point::new(W as i32 / 2, H as i32 / 2);
        let _ = Text::with_alignment(
            label,
            center,
            MonoTextStyle::new(&FONT_10X20, fg),
            Alignment::Center,
        )
        .draw(frame);
    }
    let _ = frame.flush(display);
}

/// Accumulates received bytes into protocol lines (PROTOCOL.md). A line is
/// handed out on newline; a line longer than the buffer is dropped whole, so a
/// garbled burst cannot turn into a command.
struct LineBuf {
    buf: [u8; 48],
    len: usize,
    overflow: bool,
}

impl LineBuf {
    const fn new() -> Self {
        Self {
            buf: [0; 48],
            len: 0,
            overflow: false,
        }
    }

    fn push(&mut self, b: u8) -> Option<&[u8]> {
        if b == b'\r' || b == b'\n' {
            let done = !self.overflow && self.len > 0;
            let len = self.len;
            self.len = 0;
            self.overflow = false;
            return done.then(|| &self.buf[..len]);
        }
        if self.len < self.buf.len() {
            self.buf[self.len] = b;
            self.len += 1;
        } else {
            self.overflow = true;
        }
        None
    }
}

/// One protocol line: a single state letter, or `S <key> [value]`.
enum Line<'a> {
    State(State),
    Setting(&'a str, &'a str),
    Unknown,
}

fn parse_line(raw: &[u8]) -> Line<'_> {
    let Ok(text) = core::str::from_utf8(raw) else {
        return Line::Unknown;
    };
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() == 1 {
        return State::from_cmd(bytes[0]).map_or(Line::Unknown, Line::State);
    }
    match text.split_once(' ') {
        Some((s, rest)) if s.eq_ignore_ascii_case("S") => {
            let (key, value) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
            Line::Setting(key, value.trim())
        }
        _ => Line::Unknown,
    }
}

/// Apply one `S` line. Unknown keys and bad values are ignored, per the
/// protocol. Returns true if the flame parameters changed.
fn apply_setting(key: &str, value: &str, params: &mut FlameParams, board: &mut Board) -> bool {
    let is = |name: &str| key.eq_ignore_ascii_case(name);
    if is("reset") {
        *params = FlameParams::default();
        board.backlight.set_percent(100);
        return true;
    }
    let Ok(v) = value.parse::<u8>() else {
        return false;
    };
    if is("brightness") {
        board.backlight.set_percent(v.min(100));
        return false;
    }
    if is("cooling") {
        params.cooling = v;
    } else if is("drift") {
        params.drift = v.min(3);
    } else if is("flicker") {
        params.flicker = v.max(1);
    } else if is("seed_min") {
        params.seed_min = v;
    } else if is("seed_max") {
        params.seed_max = v;
    } else if is("blue_full") {
        params.blue_full = v.max(1);
    } else if is("green_start") {
        params.green_start = v;
    } else if is("white_start") {
        params.white_start = v;
    } else {
        return false;
    }
    true
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    // esp-rtos (embassy) needs a timer and a heap; the frame and heat buffers are
    // large, so add PSRAM to the global allocator and let big allocations land there.
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    let psram = esp_hal::psram::Psram::new(peripherals.PSRAM, Default::default());
    esp_alloc::psram_allocator!(&psram);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    // The board claims the LCD, backlight, buttons and battery. USB_DEVICE, TIMG0
    // and FROM_CPU_INTR0 are left free (see the board crate's `resources!`).
    let mut board = Board::new(resources!(peripherals)).expect("board init");
    board.backlight.set_percent(100);

    // ~106 KB frame + 54 KB heat, both in PSRAM.
    let pixels = alloc::vec![Rgb565::BLACK; FrameBuffer::LEN].leak();
    let mut frame = FrameBuffer::new(pixels);
    let heat = alloc::vec![0u8; W * H].leak();
    // The flame effect is shared with the PC preview (see firmware/flame). The
    // defaults can be overridden live from the router's settings via S lines.
    let mut flame = Flame::new(W, H, FlameParams::default(), 0xC0FF_EE11);

    // USB Serial/JTAG is the link to the router (it enumerates under vendor 303a
    // as a ttyACM, which netled finds). We drive it directly and leave
    // esp-println's logger uninitialised so nothing else owns this peripheral.
    // split() yields (rx, tx); async Read comes from embedded-io-async.
    let usb = UsbSerialJtag::new(peripherals.USB_DEVICE).into_async();
    let (mut rx, _tx) = usb.split();

    let mut state = State::Boot;
    render_static(&mut frame, &mut board.display, state);

    let mut line = LineBuf::new();
    let mut params = FlameParams::default();
    let mut last_cmd = Instant::now();
    let mut byte = [0u8; 1];

    loop {
        // Draw, or wait, depending on whether we are animating the flame.
        let got = if state == State::Online {
            flame.step(heat);
            flame.render(heat, frame.pixels_mut());
            let _ = frame.flush(&mut board.display);
            match select(Timer::after(FRAME_DT), rx.read(&mut byte)).await {
                Either::First(_) => None,
                Either::Second(res) => Some(res),
            }
        } else {
            match select(Timer::after(IDLE_TICK), rx.read(&mut byte)).await {
                Either::First(_) => None,
                Either::Second(res) => Some(res),
            }
        };

        if let Some(Ok(1)) = got {
            if let Some(raw) = line.push(byte[0]) {
                match parse_line(raw) {
                    Line::State(new) => {
                        last_cmd = Instant::now();
                        if new != state {
                            let was_online = state == State::Online;
                            state = new;
                            if state == State::Online && !was_online {
                                // Start the fire cold so it grows in.
                                flame.reset(heat);
                            } else if state != State::Online {
                                render_static(&mut frame, &mut board.display, state);
                            }
                        }
                    }
                    Line::Setting(key, value) => {
                        last_cmd = Instant::now();
                        if apply_setting(key, value, &mut params, &mut board) {
                            flame.set_params(params);
                        }
                    }
                    Line::Unknown => {}
                }
            }
        }

        // Watchdog: no valid command for WATCHDOG -> the router went silent.
        if state != State::NoSignal && last_cmd.elapsed() > WATCHDOG {
            state = State::NoSignal;
            render_static(&mut frame, &mut board.display, state);
        }
    }
}
