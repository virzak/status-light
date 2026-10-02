//! Blue-flame internet status as the idle screen of the GL.iNet Flint 4's
//! built-in LCD, with GL's own screen UI one touch away.
//!
//! The panel is a standard Linux fbtft framebuffer: `/dev/fb0`, 240x320
//! RGB565, driver `st7789p3` (GPL source in GL's gl-image repo). It is
//! mounted landscape, so this renders a 320x240 landscape image and rotates
//! it into the portrait framebuffer.
//!
//! Two modes:
//!
//! - Flame: gl_screen is stopped and this draws the status. A touch on the
//!   screen switches to menu mode.
//! - Menu: gl_screen runs and owns the display. After `IDLE` without touches
//!   it is stopped again and the flame returns.
//!
//! State comes from a one-letter file written by netled (`B` online, `A`
//! degraded, `R` offline, the same letters as the serial protocol). If the
//! file is missing or older than 60 s, the screen blinks red like the USB
//! boards' watchdog.
//!
//! Options: `--flip` rotates the image 180 degrees, `--bgr` swaps red and blue.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
use flame::{Flame, FlameParams};

/// Physical framebuffer (portrait).
const FB_W: usize = 240;
const FB_H: usize = 320;
/// Logical image as seen on the landscape-mounted panel.
const W: usize = 320;
const H: usize = 240;

const FB: &str = "/dev/fb0";
const TOUCH: &str = "/dev/input/event0";
const STATE_FILE: &str = "/tmp/netled.state";
const STALE: Duration = Duration::from_secs(60);
/// Back to the flame after this long without a touch in menu mode.
const IDLE: Duration = Duration::from_secs(60);
/// fbtft refreshes the panel at about 20 fps, so drawing faster is wasted.
const FRAME: Duration = Duration::from_millis(50);
const O_NONBLOCK: i32 = 0o4000;
/// Linux `struct input_event` on 64-bit: timeval (16) + type + code + value.
const EVENT_SIZE: usize = 24;
const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;

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

/// True if any touch event arrived since the last call. Drains the queue.
fn touched(dev: &mut Option<File>) -> bool {
    let Some(f) = dev else { return false };
    let mut buf = [0u8; EVENT_SIZE * 32];
    let mut any = false;
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                any |= buf[..n].chunks_exact(EVENT_SIZE).any(|e| {
                    let t = u16::from_le_bytes([e[16], e[17]]);
                    t == EV_KEY || t == EV_ABS
                });
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(_) => {
                // Device went away; stop polling it rather than spinning.
                *dev = None;
                break;
            }
        }
    }
    any
}

fn gl_screen_running() -> bool {
    Command::new("pidof")
        .arg("gl_screen")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn gl_screen(action: &str) {
    let _ = Command::new("/etc/init.d/gl_screen").arg(action).status();
}

/// Stop GL's UI and wait until it is really gone: it ignores SIGTERM, so
/// procd only SIGKILLs it about 5 s after `stop` returns.
fn stop_gl_screen() {
    gl_screen("stop");
    for _ in 0..15 {
        if !gl_screen_running() {
            break;
        }
        sleep(Duration::from_secs(1));
    }
}

fn wake_panel() {
    let _ = fs::write("/sys/class/graphics/fb0/blank", "0");
    let _ = fs::write("/sys/class/backlight/backlight/bl_power", "0");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bgr = args.iter().any(|a| a == "--bgr");
    let flip = args.iter().any(|a| a == "--flip");

    let fb = OpenOptions::new()
        .write(true)
        .open(FB)
        .unwrap_or_else(|e| panic!("open {FB}: {e}"));
    let mut touch = OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(TOUCH)
        .ok();

    // Also covers a procd respawn, where the init script's start hook does
    // not run again.
    stop_gl_screen();
    wake_panel();

    // The default cooling suits the 170-row T-Display; on this 240-row
    // landscape image, 1 lights about the bottom two thirds.
    let params = FlameParams {
        cooling: 1,
        ..FlameParams::default()
    };
    let mut fl = Flame::new(W, H, params, 0xC0FF_EE11);
    let mut heat = vec![0u8; W * H];
    let mut px = vec![Rgb565::BLACK; W * H];
    let mut bytes = vec![0u8; FB_W * FB_H * 2];

    let start = Instant::now();
    let mut state = State::NoSignal;
    let mut last_check: Option<Instant> = None;
    // For static screens, only push a frame when it changes.
    let mut last_static: Option<(State, bool)> = None;
    // Some(last touch) while GL's menu owns the display.
    let mut menu: Option<Instant> = None;

    loop {
        let t = Instant::now();
        let touch_now = touched(&mut touch);

        if let Some(last) = menu {
            if touch_now {
                menu = Some(t);
            } else if t - last >= IDLE {
                stop_gl_screen();
                wake_panel();
                fl.reset(&mut heat);
                last_static = None;
                menu = None;
            }
            sleep(FRAME);
            continue;
        }
        if touch_now {
            gl_screen("start");
            menu = Some(t);
            continue;
        }

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
            for y in 0..H {
                for x in 0..W {
                    let c = px[y * W + x];
                    let c = if bgr { Rgb565::new(c.b(), c.g(), c.r()) } else { c };
                    // Landscape (x, y) to portrait framebuffer: 90 degrees
                    // clockwise on the mounted panel, or 270 with --flip.
                    let (fx, fy) = if flip {
                        (y, FB_H - 1 - x)
                    } else {
                        (FB_W - 1 - y, x)
                    };
                    let i = (fy * FB_W + fx) * 2;
                    let v = RawU16::from(c).into_inner();
                    bytes[i..i + 2].copy_from_slice(&v.to_le_bytes());
                }
            }
            let _ = fb.write_all_at(&bytes, 0);
        }

        let elapsed = t.elapsed();
        if elapsed < FRAME {
            sleep(FRAME - elapsed);
        }
    }
}
