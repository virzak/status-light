//! Internet-status firmware for the Waveshare ESP32-S3-Zero.
//!
//! Reads the status protocol (see ../../PROTOCOL.md) from the router over USB
//! serial and shows it on two WS2812 chains: the board's own LED (GPIO21) and an
//! optional addressable strip (GPIO2). Online, the LED breathes blue and the
//! strip shimmers with the shared blue flame; the other states are solid
//! colours. `S` lines set the brightness, the strip length and the flame.
//!
//! Build and flash notes are in README.md.

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::Read;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::rmt::Rmt;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::usb::usb_serial_jtag::UsbSerialJtag;
use esp_hal_smartled::{RmtSmartLeds, buffer_size, color_order};
use flame::{Flame, FlameParams};
use smart_leds_trait::{RGB8, SmartLedsWriteAsync};
use status_protocol::{Command, Line, LineBuf, apply_flame_setting, parse_line};

esp_bootloader_esp_idf::esp_app_desc!();

/// The longest strip the firmware drives; `S strip_leds` picks how many light.
const STRIP_MAX: usize = 150;
const STRIP_DEFAULT: usize = 60;

/// The strip shows one row of a small flame field: each LED is a column, and
/// the row is where the default heat sits mid-palette (about 108 +/- 28), so it
/// shimmers instead of sitting at full blue. Measured with the flame crate.
const FLAME_H: usize = 16;
const FLAME_ROW: usize = 4;
const STRIP_COOLING: u8 = 12;

/// Full-scale channel levels at 100% brightness. The onboard LED is a single
/// status light; the strip is capped so 60 LEDs stay within what the router's
/// USB port can supply even at a white flame tip.
const LED_LEVEL: u32 = 40;
const STRIP_LEVEL: u32 = 32;

/// No command for this long means the router hung or netled died (PROTOCOL.md).
const WATCHDOG: Duration = Duration::from_secs(60);
const FRAME_DT: Duration = Duration::from_millis(33);
const BREATHE_MS: u64 = 4000;
const BLINK_MS: u64 = 500;

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
    /// Full-scale colour of a solid state at time `ms`; `None` while online,
    /// which animates instead.
    fn color(self, ms: u64) -> Option<RGB8> {
        Some(match self {
            Self::Online => return None,
            Self::Boot => RGB8::new(64, 64, 64),
            Self::Degraded => RGB8::new(255, 85, 0),
            Self::Offline => RGB8::new(255, 0, 0),
            Self::Green => RGB8::new(0, 255, 0),
            Self::White => RGB8::new(255, 255, 255),
            Self::Off => RGB8::default(),
            Self::NoSignal if (ms / BLINK_MS) % 2 == 0 => RGB8::new(255, 0, 0),
            Self::NoSignal => RGB8::default(),
        })
    }
}

/// Live settings from `S` lines.
struct Settings {
    brightness: u32,
    strip_leds: usize,
    flame: FlameParams,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            brightness: 100,
            strip_leds: STRIP_DEFAULT,
            flame: FlameParams { cooling: STRIP_COOLING, ..FlameParams::default() },
        }
    }
}

impl Settings {
    /// Apply one `S` line. Unknown keys and bad values are ignored, per the
    /// protocol. Returns true if the flame parameters changed.
    fn apply(&mut self, key: &str, value: &str) -> bool {
        if key.eq_ignore_ascii_case("reset") {
            *self = Self::default();
            return true;
        }
        let Ok(v) = value.parse::<u16>() else {
            return false;
        };
        if key.eq_ignore_ascii_case("brightness") {
            self.brightness = u32::from(v.min(100));
            false
        } else if key.eq_ignore_ascii_case("strip_leds") {
            self.strip_leds = usize::from(v).min(STRIP_MAX);
            false
        } else {
            u8::try_from(v).is_ok_and(|v| apply_flame_setting(key, v, &mut self.flame))
        }
    }

    /// Scale a full-scale colour to `level` at the current brightness.
    fn scale(&self, c: RGB8, level: u32) -> RGB8 {
        let s = |v: u8| (u32::from(v) * level * self.brightness / (255 * 100)) as u8;
        RGB8::new(s(c.r), s(c.g), s(c.b))
    }
}

/// Breathing blue: a raised cosine, squared so the fade looks even to the eye.
fn breathe(ms: u64) -> RGB8 {
    let phase = (ms % BREATHE_MS) as f32 / BREATHE_MS as f32;
    let v = (1.0 - cos(2.0 * core::f32::consts::PI * phase)) / 2.0;
    RGB8::new(0, 0, (255.0 * v * v) as u8)
}

/// cos() without std or libm: a 7th-order Taylor series around 0, after folding
/// x into [-pi, pi]. Plenty for an LED fade.
fn cos(x: f32) -> f32 {
    use core::f32::consts::PI;
    let mut x = x % (2.0 * PI);
    if x > PI {
        x -= 2.0 * PI;
    }
    // cos is even, and cos(x) = -cos(pi - x) keeps the series near 0.
    let (x, sign) = if x.abs() > PI / 2.0 { (PI - x.abs(), -1.0) } else { (x, 1.0) };
    let x2 = x * x;
    sign * (1.0 - x2 / 2.0 + x2 * x2 / 24.0 - x2 * x2 * x2 / 720.0)
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(size: 32 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    let freq = Rate::from_mhz(80);
    let rmt = Rmt::new(peripherals.RMT, freq).expect("RMT init").into_async();
    // The onboard LED takes RGB, not the usual GRB; the strip is GRB.
    let mut led = RmtSmartLeds::<{ buffer_size::<RGB8>(1) }, _, RGB8, color_order::Rgb>::new(
        esp_hal_smartled::WS2812_TIMING,
        rmt.channel0,
        peripherals.GPIO21,
        freq,
    )
    .expect("onboard LED");
    let mut strip = RmtSmartLeds::<{ buffer_size::<RGB8>(STRIP_MAX) }, _, RGB8, color_order::Grb>::new(
        esp_hal_smartled::WS2812_TIMING,
        rmt.channel1,
        peripherals.GPIO2,
        freq,
    )
    .expect("strip");

    // USB Serial/JTAG is the link to the router (vendor 303a, a ttyACM there).
    let usb = UsbSerialJtag::new(peripherals.USB_DEVICE).into_async();
    let (mut rx, _tx) = usb.split();

    let mut settings = Settings::default();
    let mut flame = Flame::new(STRIP_MAX, FLAME_H, settings.flame, 0x5EED_2E10);
    let mut heat = [0u8; STRIP_MAX * FLAME_H];

    let mut state = State::Boot;
    let mut line = LineBuf::new();
    let mut last_cmd = Instant::now();
    let mut next_frame = Instant::now();
    let mut byte = [0u8; 1];

    loop {
        match select(Timer::at(next_frame), rx.read(&mut byte)).await {
            Either::First(()) => {
                next_frame += FRAME_DT;
                let ms = Instant::now().as_millis();

                if state != State::NoSignal && last_cmd.elapsed() > WATCHDOG {
                    state = State::NoSignal;
                }

                let n = settings.strip_leds;
                let (onboard, strip_px): (RGB8, [RGB8; STRIP_MAX]) = match state.color(ms) {
                    Some(c) => {
                        let s = settings.scale(c, STRIP_LEVEL);
                        (settings.scale(c, LED_LEVEL), core::array::from_fn(|i| if i < n { s } else { RGB8::default() }))
                    }
                    None => {
                        flame.step(&mut heat);
                        let row = &heat[FLAME_ROW * STRIP_MAX..][..STRIP_MAX];
                        let px = core::array::from_fn(|i| {
                            if i >= n {
                                return RGB8::default();
                            }
                            let [r, g, b] = flame.color(row[i]);
                            settings.scale(RGB8::new(r, g, b), STRIP_LEVEL)
                        });
                        (settings.scale(breathe(ms), LED_LEVEL), px)
                    }
                };

                // A failed frame is simply redrawn on the next tick.
                let _ = join(led.write([onboard]), strip.write(strip_px)).await;
            }
            Either::Second(Ok(1)) => {
                let Some(raw) = line.push(byte[0]) else {
                    continue;
                };
                match parse_line(raw) {
                    Line::Command(cmd) => {
                        last_cmd = Instant::now();
                        let new = State::from(cmd);
                        if new == State::Online && state != State::Online {
                            // Start the fire cold so it grows in.
                            flame.reset(&mut heat);
                        }
                        state = new;
                    }
                    Line::Setting(key, value) => {
                        last_cmd = Instant::now();
                        if settings.apply(key, value) {
                            flame.set_params(settings.flame);
                        }
                    }
                    Line::Unknown => {}
                }
            }
            Either::Second(_) => {}
        }
    }
}
