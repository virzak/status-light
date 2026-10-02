//! Blue-flame internet status on the GL.iNet Flint 4's built-in LCD.
//!
//! The panel is a standard Linux fbtft framebuffer: `/dev/fb0`, 240x320
//! RGB565, driver `st7789p3` (GPL source in GL's gl-image repo). This writes
//! whole frames to it and the kernel pushes them to the panel.
//!
//! State comes from a one-letter file written by netled (`B` online, `A`
//! degraded, `R` offline, the same letters as the serial protocol). If the
//! file is missing or older than 60 s, the screen blinks red like the USB
//! boards' watchdog. Run it through `flame-screen.sh`, which stops GL's own
//! screen UI first and restores it on exit.
//!
//! Options: `--bgr` swaps red and blue if the panel shows the flame orange.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileExt;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
use flame::{Flame, FlameParams};

const W: usize = 240;
const H: usize = 320;
const FB: &str = "/dev/fb0";
const STATE_FILE: &str = "/tmp/netled.state";
const STALE: Duration = Duration::from_secs(60);
/// fbtft refreshes the panel at about 20 fps, so drawing faster is wasted.
const FRAME: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    Online,
    Degraded,
    Offline,
    NoSignal,
}

fn read_state() -> State {
    let fresh = fs::metadata(STATE_FILE)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < STALE);
    if !fresh {
        return State::NoSignal;
    }
    match fs::read_to_string(STATE_FILE)
        .ok()
        .and_then(|s| s.trim().chars().next())
        .map(|c| c.to_ascii_uppercase())
    {
        Some('B') => State::Online,
        Some('A') => State::Degraded,
        Some('R') => State::Offline,
        _ => State::NoSignal,
    }
}

fn main() {
    let bgr = std::env::args().any(|a| a == "--bgr");
    let fb = OpenOptions::new()
        .write(true)
        .open(FB)
        .unwrap_or_else(|e| panic!("open {FB}: {e}"));

    // GL's screen UI may have blanked the panel and its backlight; wake both.
    let _ = fs::write("/sys/class/graphics/fb0/blank", "0");
    let _ = fs::write("/sys/class/backlight/backlight/bl_power", "0");

    // The default cooling is tuned for the 170-row T-Display; on this 320-row
    // screen 1 gives the same bottom-half flame.
    let params = FlameParams {
        cooling: 1,
        ..FlameParams::default()
    };
    let mut fl = Flame::new(W, H, params, 0xC0FF_EE11);
    let mut heat = vec![0u8; W * H];
    let mut px = vec![Rgb565::BLACK; W * H];
    let mut bytes = vec![0u8; W * H * 2];

    let start = Instant::now();
    let mut state = State::NoSignal;
    let mut last_check: Option<Instant> = None;
    // For static screens, only push a frame when it changes.
    let mut last_static: Option<(State, bool)> = None;

    loop {
        let t = Instant::now();
        if last_check.map_or(true, |c| t - c >= Duration::from_secs(1)) {
            let s = read_state();
            if s != state {
                if s == State::Online {
                    // Start the fire cold so it grows in.
                    fl.reset(&mut heat);
                }
                state = s;
            }
            last_check = Some(t);
        }

        let draw = match state {
            State::Online => {
                fl.step(&mut heat);
                fl.render(&heat, &mut px);
                last_static = None;
                true
            }
            other => {
                let blink_on = (start.elapsed().as_millis() / 500) % 2 == 0;
                let key = (other, other == State::NoSignal && blink_on);
                let changed = last_static != Some(key);
                if changed {
                    px.fill(match other {
                        State::Degraded => Rgb565::new(31, 32, 0),
                        State::Offline => Rgb565::new(31, 0, 0),
                        _ if blink_on => Rgb565::new(20, 0, 0),
                        _ => Rgb565::BLACK,
                    });
                    last_static = Some(key);
                }
                changed
            }
        };

        if draw {
            for (i, &c) in px.iter().enumerate() {
                let c = if bgr { Rgb565::new(c.b(), c.g(), c.r()) } else { c };
                let v = RawU16::from(c).into_inner();
                bytes[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
            }
            let _ = fb.write_all_at(&bytes, 0);
        }

        let elapsed = t.elapsed();
        if elapsed < FRAME {
            sleep(FRAME - elapsed);
        }
    }
}
