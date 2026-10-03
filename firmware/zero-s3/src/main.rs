//! Internet-status firmware for the Waveshare ESP32-S3-Zero.
//!
//! Reads the status protocol (see ../../PROTOCOL.md) from the router over USB
//! serial and shows it on two WS2812 chains: the board's own LED (GPIO21) and an
//! optional addressable strip (GPIO2). Online, the LED breathes blue and a soft
//! blue glow sweeps back and forth along the strip; the other states are solid
//! colours. `S` lines set the brightness, the strip's length, the sweep's speed
//! and width, and a counting pattern for finding out how many LEDs a strip has.
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
use smart_leds_trait::{RGB8, SmartLedsWriteAsync};
use status_protocol::{Command, Line, LineBuf, parse_line};

esp_bootloader_esp_idf::esp_app_desc!();

/// The longest strip the firmware drives (the RMT buffer is sized for it). How
/// many LEDs a strip actually has is a setting, `S strip_leds`: a WS2812 strip
/// cannot report its length, so the user counts it with `S strip_identify 1`.
const STRIP_MAX: usize = 300;

/// Full-scale channel levels at 100% brightness. The onboard LED is a single
/// status light; the strip is capped so a full strip of solid colour stays
/// within what a USB port can supply.
const LED_LEVEL: u32 = 40;
const STRIP_LEVEL: u32 = 32;

/// No command for this long means the router hung or netled died (PROTOCOL.md).
const WATCHDOG: Duration = Duration::from_secs(60);
const FRAME_DT: Duration = Duration::from_millis(33);
const BREATHE_MS: u64 = 4000;
const BLINK_MS: u64 = 500;

/// Share of full brightness the strip keeps away from the sweep, so it reads as
/// one lit strip with a moving highlight rather than a lone moving dot.
const SWEEP_BASE: f32 = 0.08;

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

/// Live settings from `S` lines (keys in PROTOCOL.md, ranges in
/// settings.schema.json).
struct Settings {
    /// Percent, 0-100, for both the onboard LED and the strip.
    brightness: u32,
    /// LEDs on the strip, 0 to STRIP_MAX.
    strip_leds: usize,
    /// Sweep speed, 1 (slow) to 100 (fast).
    speed: u32,
    /// Half-width of the sweep's glow, in LEDs.
    width: u32,
    /// Show the counting pattern on the strip instead of the status.
    identify: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            brightness: 100,
            strip_leds: 60,
            speed: 30,
            width: 8,
            identify: false,
        }
    }
}

impl Settings {
    /// Apply one `S` line. Unknown keys and bad values are ignored, per the
    /// protocol.
    fn apply(&mut self, key: &str, value: &str) {
        let is = |name: &str| key.eq_ignore_ascii_case(name);
        if is("reset") {
            *self = Self::default();
            return;
        }
        let Ok(v) = value.parse::<u16>() else {
            return;
        };
        if is("brightness") {
            self.brightness = u32::from(v.min(100));
        } else if is("strip_leds") {
            self.strip_leds = usize::from(v).min(STRIP_MAX);
        } else if is("strip_speed") {
            self.speed = u32::from(v.clamp(1, 100));
        } else if is("strip_width") {
            self.width = u32::from(v.clamp(1, 50));
        } else if is("strip_identify") {
            self.identify = v != 0;
        }
    }

    /// Scale a full-scale colour to `level` at the current brightness.
    fn scale(&self, c: RGB8, level: u32) -> RGB8 {
        let s = |v: u8| (u32::from(v) * level * self.brightness / (255 * 100)) as u8;
        RGB8::new(s(c.r), s(c.g), s(c.b))
    }

    /// One round trip of the sweep, in ms: about 20 s at speed 1, 5 s at the
    /// default 30, 2 s at 100.
    fn sweep_period_ms(&self) -> u64 {
        200_000 / u64::from(self.speed + 9)
    }
}

/// Breathing blue for the onboard LED: a raised cosine, squared so the fade
/// looks even to the eye.
fn breathe(ms: u64) -> RGB8 {
    let phase = (ms % BREATHE_MS) as f32 / BREATHE_MS as f32;
    let v = (1.0 - cos(2.0 * core::f32::consts::PI * phase)) / 2.0;
    RGB8::new(0, 0, (255.0 * v * v) as u8)
}

/// The online strip: a soft glow sweeping from end to end and back, slowing at
/// the ends like a pendulum, blue with a cyan core, over a faint blue base.
fn sweep(settings: &Settings, ms: u64, px: &mut [RGB8]) {
    let n = settings.strip_leds;
    if n == 0 {
        return;
    }
    let period = settings.sweep_period_ms();
    let phase = (ms % period) as f32 / period as f32;
    let pos = (n - 1) as f32 * (1.0 - cos(2.0 * core::f32::consts::PI * phase)) / 2.0;
    let width = settings.width as f32;
    for (i, p) in px.iter_mut().take(n).enumerate() {
        let d = (i as f32 - pos) / width;
        // A smooth bump that reaches zero at +/- width.
        let glow = if d.abs() < 1.0 { (1.0 - d * d) * (1.0 - d * d) } else { 0.0 };
        let v = SWEEP_BASE + (1.0 - SWEEP_BASE) * glow;
        let core = glow * glow * glow;
        *p = settings.scale(RGB8::new(0, (160.0 * core) as u8, (255.0 * v) as u8), STRIP_LEVEL);
    }
}

/// The counting pattern, over every position the firmware can drive so a strip
/// of any length shows all of it: the first LED green, every 10th red, the rest
/// dim blue. Count the reds, then the blues after the last one.
fn identify(settings: &Settings, px: &mut [RGB8]) {
    for (i, p) in px.iter_mut().enumerate() {
        let c = match i {
            0 => RGB8::new(0, 255, 0),
            _ if (i + 1) % 10 == 0 => RGB8::new(255, 0, 0),
            _ => RGB8::new(0, 0, 64),
        };
        *p = settings.scale(c, STRIP_LEVEL);
    }
}

/// cos() without std or libm: a Taylor series near 0, after folding x into
/// [-pi/2, pi/2]. Plenty for an LED fade.
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

                let mut px = [RGB8::default(); STRIP_MAX];
                let onboard = match state.color(ms) {
                    Some(c) => {
                        let s = settings.scale(c, STRIP_LEVEL);
                        px.iter_mut().take(settings.strip_leds).for_each(|p| *p = s);
                        settings.scale(c, LED_LEVEL)
                    }
                    None => {
                        sweep(&settings, ms, &mut px);
                        settings.scale(breathe(ms), LED_LEVEL)
                    }
                };
                if settings.identify {
                    identify(&settings, &mut px);
                }

                // A failed frame is simply redrawn on the next tick.
                let _ = join(led.write([onboard]), strip.write(px)).await;
            }
            Either::Second(Ok(1)) => {
                let Some(raw) = line.push(byte[0]) else {
                    continue;
                };
                match parse_line(raw) {
                    Line::Command(cmd) => {
                        last_cmd = Instant::now();
                        state = State::from(cmd);
                    }
                    Line::Setting(key, value) => {
                        last_cmd = Instant::now();
                        settings.apply(key, value);
                    }
                    Line::Unknown => {}
                }
            }
            Either::Second(_) => {}
        }
    }
}
