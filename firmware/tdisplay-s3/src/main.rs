//! Internet-status display for the LilyGO T-Display-S3.
//!
//! Reads the one-letter status protocol (see ../../PROTOCOL.md) from the router
//! over USB serial and shows it on the ST7789 LCD. When online it renders an
//! animated blue flame; the other states are static colour + label screens.
//! The router side (`router/netled`) is shared with the WS2812 board, unchanged.
//!
//! Build/flash notes are in README.md. Not compile-tested in the authoring
//! session (no Xtensa toolchain there); the two spots to check on first build
//! are marked `VERIFY`.

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
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::usb_serial_jtag::UsbSerialJtag;
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

/// Blue-fire palette: heat 0..=255 -> black, navy, blue, cyan, white tip.
fn build_palette() -> [Rgb565; 256] {
    let mut p = [Rgb565::BLACK; 256];
    let mut h = 0usize;
    while h < 256 {
        let hv = h as u32;
        // Blue rises first and saturates early; green joins for cyan as it gets
        // hotter; red only near the top, for a white flame tip.
        let b = (hv * 255 / 170).min(255);
        let g = if hv > 100 { ((hv - 100) * 255 / 155).min(255) } else { 0 };
        let r = if hv > 200 { ((hv - 200) * 255 / 55).min(255) } else { 0 };
        p[h] = Rgb565::new((r >> 3) as u8, (g >> 2) as u8, (b >> 3) as u8);
        h += 1;
    }
    p
}

/// One step of a Doom-style fire, seeded along the bottom row, rising upward.
fn step_fire(heat: &mut [u8], rng: &mut Rng) {
    // Seed the bottom row hot, with a little flicker.
    for x in 0..W {
        heat[(H - 1) * W + x] = if rng.random() & 7 == 0 { 170 } else { 255 };
    }
    // Spread: each source cell feeds a cell one row up, drifting horizontally.
    for x in 0..W {
        for y in 1..H {
            let src = y * W + x;
            let pixel = heat[src];
            if pixel == 0 {
                heat[src - W] = 0;
            } else {
                let rand = (rng.random() & 3) as usize; // 0..=3
                // dst = src - rand + 1, then one row up (- W). Guard the ends.
                let dst = src + 1;
                if dst >= rand + W {
                    let target = dst - rand - W;
                    heat[target] = pixel.saturating_sub((rand & 1) as u8);
                }
            }
        }
    }
}

fn blit_flame(heat: &[u8], palette: &[Rgb565; 256], frame: &mut FrameBuffer) {
    let px = frame.pixels_mut();
    for i in 0..(W * H) {
        px[i] = palette[heat[i] as usize];
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

/// Feed one received byte into the parser. Mirrors the WS2812 board: remember the
/// last command letter, apply it on newline. Returns the new state if it changed.
fn feed(
    byte: u8,
    pending: &mut Option<State>,
    state: State,
    last_cmd: &mut Instant,
) -> Option<State> {
    if byte == b'\r' || byte == b'\n' {
        if let Some(new) = pending.take() {
            *last_cmd = Instant::now();
            if new != state {
                return Some(new);
            }
        }
    } else if let Some(s) = State::from_cmd(byte) {
        *pending = Some(s);
    }
    None
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
    let palette = build_palette();
    let mut rng = Rng::new();

    // USB Serial/JTAG is the link to the router (it enumerates under vendor 303a
    // as a ttyACM, which netled finds). We drive it directly and leave
    // esp-println's logger uninitialised so nothing else owns this peripheral.
    //
    // VERIFY: the exact async API for esp-hal ~1.2.2 - `into_async()`, the order
    // of `split()`, and the embedded-io-async `Read` impl. See
    // https://docs.rs/esp-hal/1.2.2/esp_hal/usb_serial_jtag/index.html
    let usb = UsbSerialJtag::new(peripherals.USB_DEVICE).into_async();
    let (mut rx, _tx) = usb.split();

    let mut state = State::Boot;
    render_static(&mut frame, &mut board.display, state);

    let mut pending: Option<State> = None;
    let mut last_cmd = Instant::now();
    let mut byte = [0u8; 1];

    loop {
        // Draw, or wait, depending on whether we are animating the flame.
        let got = if state == State::Online {
            step_fire(heat, &mut rng);
            blit_flame(heat, &palette, &mut frame);
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
            if let Some(new) = feed(byte[0], &mut pending, state, &mut last_cmd) {
                let was_online = state == State::Online;
                state = new;
                if state == State::Online && !was_online {
                    // Start the fire cold so it grows in.
                    heat.fill(0);
                } else if state != State::Online {
                    render_static(&mut frame, &mut board.display, state);
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
