//! Blue-flame internet status as the idle screen of the GL.iNet Flint 4's
//! built-in LCD, with GL's own screen UI one touch away.
//!
//! The panel is a standard Linux fbtft framebuffer: `/dev/fb0`, 240x320
//! RGB565, driver `st7789p3` (GPL source in GL's gl-image repo). It is
//! mounted landscape, so this renders a 320x240 landscape image and rotates
//! it into the portrait framebuffer.
//!
//! GL's own screen UI (`gl_screen`) keeps running. When it goes to sleep (its
//! screen timeout, `gl_screen.generic.AUTO_LOCK_TIME`) it switches the
//! backlight off and stops drawing; this then lights the panel and draws the
//! flame. A touch wakes `gl_screen` instantly, since it is already running, so
//! this stops drawing and hands the display back. If the screen has not
//! changed 3 s after a touch, `gl_screen` did not wake and the flame resumes.
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
use flame::wisps::{WispParams, Wisps};
use serde::Deserialize;

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
const BL_POWER: &str = "/sys/class/backlight/backlight/bl_power";
const BL_BRIGHTNESS: &str = "/sys/class/backlight/backlight/brightness";
const BL_MAX: &str = "/sys/class/backlight/backlight/max_brightness";
/// Shared settings file (settings.schema.json). Only the `lcd` section is used
/// here; netled handles `boards`.
const SETTINGS_FILE: &str = "/etc/status-light.json";
/// Backlight percent while the flame shows, unless set in the settings.
const DEFAULT_BRIGHTNESS: u8 = 80;
/// After a touch, give gl_screen this long to redraw before assuming it did
/// not wake.
const WAKE_CHECK: Duration = Duration::from_secs(3);
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

/// gl_screen sleeps by switching the backlight off (bl_power 1). While the
/// flame shows, this keeps it on, so 1 only appears when gl_screen goes to
/// sleep again after being woken.
fn gl_asleep() -> bool {
    fs::read_to_string(BL_POWER).is_ok_and(|s| s.trim() == "1")
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Settings {
    lcd: LcdSettings,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LcdSettings {
    brightness: Option<u8>,
    wisps: WispSettings,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct WispSettings {
    strands: Option<u8>,
    height: Option<u8>,
    sway: Option<u8>,
    speed: Option<u8>,
    glow: Option<u8>,
    width: Option<u8>,
}

impl LcdSettings {
    /// This screen's flame: the shared defaults (they scale with the screen
    /// height), then any overrides from the settings file.
    fn wisp_params(&self) -> WispParams {
        let f = &self.wisps;
        let d = WispParams::default();
        WispParams {
            strands: f.strands.unwrap_or(d.strands).clamp(1, 64),
            height: f.height.unwrap_or(d.height).clamp(10, 100),
            sway: f.sway.unwrap_or(d.sway).min(100),
            speed: f.speed.unwrap_or(d.speed).min(100),
            glow: f.glow.unwrap_or(d.glow).min(100),
            width: f.width.unwrap_or(d.width).min(100),
        }
    }

    fn brightness(&self) -> u8 {
        self.brightness.unwrap_or(DEFAULT_BRIGHTNESS).clamp(5, 100)
    }
}

fn settings_mtime() -> Option<SystemTime> {
    fs::metadata(SETTINGS_FILE).and_then(|m| m.modified()).ok()
}

/// Read the settings file. A missing file means all defaults; an unreadable or
/// invalid one keeps `previous`, so a half-written edit cannot blank the screen.
fn load_settings(previous: Option<LcdSettings>) -> LcdSettings {
    match fs::read_to_string(SETTINGS_FILE) {
        Err(e) if e.kind() == ErrorKind::NotFound => LcdSettings::default(),
        Err(e) => {
            eprintln!("flame-screen: {SETTINGS_FILE}: {e}");
            previous.unwrap_or_default()
        }
        Ok(text) => match serde_json::from_str::<Settings>(&text) {
            Ok(s) => s.lcd,
            Err(e) => {
                eprintln!("flame-screen: {SETTINGS_FILE}: {e}");
                previous.unwrap_or_default()
            }
        },
    }
}

/// Set the backlight to `percent` of its maximum (the driver's scale is 0-120).
fn set_brightness(percent: u8) {
    let max: u32 = fs::read_to_string(BL_MAX)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(120);
    let level = (max * u32::from(percent) + 50) / 100;
    let _ = fs::write(BL_BRIGHTNESS, level.to_string());
}

fn wake_panel() {
    let _ = fs::write("/sys/class/graphics/fb0/blank", "0");
    let _ = fs::write(BL_POWER, "0");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bgr = args.iter().any(|a| a == "--bgr");
    let flip = args.iter().any(|a| a == "--flip");

    let fb = OpenOptions::new()
        .read(true)
        .write(true)
        .open(FB)
        .unwrap_or_else(|e| panic!("open {FB}: {e}"));
    let mut touch = OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(TOUCH)
        .ok();

    // gl_screen owns the display until it goes to sleep.
    if !gl_screen_running() {
        gl_screen("start");
    }

    let mut settings_seen = settings_mtime();
    let mut settings = load_settings(None);
    let mut fl = Wisps::new(W, H, settings.wisp_params(), 0xC0FF_EE11);
    let mut last_frame = Instant::now();
    let mut px = vec![Rgb565::BLACK; W * H];
    let mut bytes = vec![0u8; FB_W * FB_H * 2];
    let mut readback = vec![0u8; FB_W * FB_H * 2];

    let start = Instant::now();
    let mut state = State::NoSignal;
    let mut last_check: Option<Instant> = None;
    // For static screens, only push a frame when it changes.
    let mut last_static: Option<(State, bool)> = None;
    // Some((touch time, gl_screen confirmed awake)) while gl_screen owns the
    // display.
    let mut gl: Option<(Instant, bool)> = Some((start, true));

    loop {
        let t = Instant::now();
        let touch_now = touched(&mut touch);

        if let Some((since, confirmed)) = gl {
            let resume = if gl_asleep() {
                // gl_screen went back to sleep: the flame takes over.
                true
            } else if !confirmed && t - since >= WAKE_CHECK {
                // If the panel still shows our last frame, gl_screen did not
                // wake on that touch.
                let unchanged = fb.read_exact_at(&mut readback, 0).is_ok() && readback == bytes;
                gl = Some((since, true));
                unchanged
            } else {
                false
            };
            if resume {
                wake_panel();
                set_brightness(settings.brightness());
                fl.reset();
                last_static = None;
                gl = None;
            } else {
                sleep(FRAME);
                continue;
            }
        }
        if touch_now {
            // gl_screen wakes on this touch by itself; stop drawing over it.
            gl = Some((t, false));
            continue;
        }

        if last_check.map_or(true, |c| t - c >= Duration::from_secs(1)) {
            let mtime = settings_mtime();
            if mtime != settings_seen {
                settings_seen = mtime;
                settings = load_settings(Some(settings));
                fl.set_params(settings.wisp_params());
                // Only while the flame shows; gl_screen owns it otherwise.
                set_brightness(settings.brightness());
            }
            let s = read_state();
            if s != state {
                if s == State::Online {
                    // Start the flame over so it grows in.
                    fl.reset();
                }
                state = s;
            }
            last_check = Some(t);
        }

        let draw = match state {
            State::Online => {
                fl.step((t - last_frame).as_millis() as u32);
                last_frame = t;
                fl.render(&mut px);
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
                    let c = if bgr {
                        Rgb565::new(c.b(), c.g(), c.r())
                    } else {
                        c
                    };
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
