//! Internet-status display for the LilyGO T-Display-S3.
//!
//! Reads the one-letter status protocol (see ../../PROTOCOL.md) from the router
//! over USB serial and shows it on the ST7789 LCD. When online it renders an
//! animated blue flame of glowing wisps; the other states are static colour +
//! label screens.
//! The router side (`router/netled`) is shared with the WS2812 board, unchanged.
//! `S` lines from netled (PROTOCOL.md) set the backlight brightness and the
//! wisp parameters live, from the router's /etc/status-light.json.
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
use flame::wisps::{WispParams, Wisps};
use lilygo_t_display_s3::{Board, FrameBuffer, HEIGHT, Lcd, WIDTH, resources};
use status_protocol::{Command, Line, LineBuf, parse_line};

esp_bootloader_esp_idf::esp_app_desc!();

const W: usize = WIDTH as usize; // 320
const H: usize = HEIGHT as usize; // 170

/// No command for this long means the router hung or netled died (PROTOCOL.md).
const WATCHDOG: Duration = Duration::from_secs(60);
/// Frame period for the flame, counted from the start of one frame to the
/// next; the flush overlaps the next frame's drawing.
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

impl From<Command> for State {
    fn from(c: Command) -> Self {
        match c {
            Command::Online => Self::Online,
            Command::Degraded => Self::Degraded,
            Command::Offline => Self::Offline,
            Command::Green => Self::Green,
            Command::White => Self::White,
            Command::Off => Self::Off,
        }
    }
}

impl State {
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

/// Apply one `S` line. Unknown keys and bad values are ignored, per the
/// protocol. Returns true if the wisp parameters changed.
fn apply_setting(key: &str, value: &str, params: &mut WispParams, board: &mut Board) -> bool {
    let is = |name: &str| key.eq_ignore_ascii_case(name);
    if is("reset") {
        *params = WispParams::default();
        board.backlight.set_percent(100);
        return true;
    }
    let Ok(v) = value.parse::<u8>() else {
        return false;
    };
    if key.eq_ignore_ascii_case("brightness") {
        board.backlight.set_percent(v.min(100));
        return false;
    }
    if is("strands") {
        params.strands = v.clamp(1, 64);
    } else if is("height") {
        params.height = v.clamp(10, 100);
    } else if is("sway") {
        params.sway = v.min(100);
    } else if is("speed") {
        params.speed = v.min(100);
    } else if is("glow") {
        params.glow = v.min(100);
    } else if is("width") {
        params.width = v.min(100);
    } else {
        return false;
    }
    true
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    // esp-rtos (embassy) needs a timer and a heap; the frame buffer is large, so
    // add PSRAM to the global allocator and let big allocations land there.
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    let psram = esp_hal::psram::Psram::new(peripherals.PSRAM, Default::default());
    esp_alloc::psram_allocator!(&psram);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    // The board claims the LCD, backlight, buttons and battery. USB_DEVICE, TIMG0
    // and FROM_CPU_INTR0 are left free (see the board crate's `resources!`).
    let mut board = Board::new(resources!(peripherals)).expect("board init");
    board.backlight.set_percent(100);

    // ~106 KB frame in PSRAM.
    let pixels = alloc::vec![Rgb565::BLACK; FrameBuffer::LEN].leak();
    let mut frame = FrameBuffer::new(pixels);
    // The flame effect is shared with the PC preview (see firmware/flame). The
    // defaults can be overridden live from the router's settings via S lines.
    // It holds a few KB of state, so it goes on the heap, not the stack.
    let flame = alloc::boxed::Box::leak(alloc::boxed::Box::new(Wisps::new(
        W,
        H,
        WispParams::default(),
        0xC0FF_EE11,
    )));

    // USB Serial/JTAG is the link to the router (it enumerates under vendor 303a
    // as a ttyACM, which netled finds). We drive it directly and leave
    // esp-println's logger uninitialised so nothing else owns this peripheral.
    // split() yields (rx, tx); async Read comes from embedded-io-async.
    let usb = UsbSerialJtag::new(peripherals.USB_DEVICE).into_async();
    let (mut rx, _tx) = usb.split();

    let mut state = State::Boot;
    render_static(&mut frame, &mut board.display, state);

    let mut line = LineBuf::new();
    let mut params = WispParams::default();
    let mut last_cmd = Instant::now();
    let mut last_frame = Instant::now();
    let mut next_frame = Instant::now();
    let mut byte = [0u8; 1];

    loop {
        // Draw, or wait, depending on whether we are animating the flame.
        let got = if state == State::Online {
            // Draw when the next frame is due; bytes arriving in between
            // only wake the loop to parse them.
            let now = Instant::now();
            if now >= next_frame {
                flame.step((now - last_frame).as_millis() as u32);
                last_frame = now;
                flame.render(frame.pixels_mut());
                let _ = frame.flush(&mut board.display);
                // Keep the cadence, but never try to catch up on missed frames.
                next_frame = (next_frame + FRAME_DT).max(now);
            }
            match select(Timer::at(next_frame), rx.read(&mut byte)).await {
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
                    Line::Command(cmd) => {
                        let new = State::from(cmd);
                        last_cmd = Instant::now();
                        if new != state {
                            let was_online = state == State::Online;
                            state = new;
                            if state == State::Online && !was_online {
                                // Start the flame over so it grows in.
                                flame.reset();
                                last_frame = Instant::now();
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
