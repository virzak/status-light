//! LED-strip patterns for the online state, shared by the ESP32-S3-Zero
//! firmware and the PC preview so both draw identical frames.
//!
//! Pure `no_std`, no allocation and no state: a frame is a function of the time,
//! the strip length and the [`Params`], so switching pattern takes effect on the
//! next frame. Each pattern gives every LED a level from 0 to 1, drawn as a
//! blend from the secondary colour (level 0) to the primary (level 1). Colours
//! are full scale; the caller scales them to its brightness.

#![no_std]

use core::f32::consts::PI;

/// An 8-bit `[r, g, b]` colour at full scale.
pub type Rgb = [u8; 3];

/// The patterns, in the order the settings page lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// A glow sliding from end to end and back, easing at the ends.
    Sweep,
    /// A bright head running one way with a fading tail, wrapping round.
    Comet,
    /// Two glows starting at the ends, meeting in the middle, then parting.
    Converge,
    /// The whole strip fading in and out together.
    Breathe,
    /// A blue wave travelling along the strip.
    Wave,
    /// A double pulse, ba-dum, then a pause.
    Heartbeat,
    /// Random LEDs gently brightening and fading.
    Twinkle,
}

impl Pattern {
    pub const ALL: [Pattern; 7] = [
        Pattern::Sweep,
        Pattern::Comet,
        Pattern::Converge,
        Pattern::Breathe,
        Pattern::Wave,
        Pattern::Heartbeat,
        Pattern::Twinkle,
    ];

    /// The name used in settings and `S strip_pattern` lines.
    pub fn name(self) -> &'static str {
        match self {
            Pattern::Sweep => "sweep",
            Pattern::Comet => "comet",
            Pattern::Converge => "converge",
            Pattern::Breathe => "breathe",
            Pattern::Wave => "wave",
            Pattern::Heartbeat => "heartbeat",
            Pattern::Twinkle => "twinkle",
        }
    }

    pub fn from_name(name: &str) -> Option<Pattern> {
        Pattern::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(name))
    }
}

/// Tuning shared by the patterns; each uses what applies to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// 1 (slow) to 100 (fast).
    pub speed: u32,
    /// In LEDs: the glow's half-width (sweep, converge), the tail (comet) or
    /// half the wavelength (wave).
    pub width: u32,
    /// The colour at full level: the moving or pulsing part.
    pub primary: Rgb,
    /// The colour at level 0; black (the default) means none, so the pattern
    /// fades from dark to the primary.
    pub secondary: Rgb,
}

/// The default primary colour, a strong blue.
pub const DEFAULT_PRIMARY: Rgb = [0x00, 0x40, 0xff];

impl Default for Params {
    fn default() -> Self {
        Self { speed: 30, width: 8, primary: DEFAULT_PRIMARY, secondary: [0, 0, 0] }
    }
}

impl Params {
    /// One cycle of the pattern, in ms: about 20 s at speed 1, 5 s at the
    /// default 30, 2 s at 100.
    pub fn period_ms(&self) -> u64 {
        200_000 / u64::from(self.speed.clamp(1, 100) + 9)
    }

    /// Position in the current cycle, 0 to 1.
    fn phase(&self, ms: u64) -> f32 {
        let period = self.period_ms();
        (ms % period) as f32 / period as f32
    }

    /// A heartbeat runs 4 times faster than the other cycles, so speed 1 to
    /// 100 spans about 12 to 120 beats a minute (47 at the default).
    fn beat_phase(&self, ms: u64) -> f32 {
        let period = self.period_ms() / 4;
        (ms % period) as f32 / period as f32
    }

    fn width(&self) -> f32 {
        self.width.max(1) as f32
    }
}

/// Share of full brightness the moving patterns keep everywhere, so the strip
/// reads as one lit strip with a moving highlight rather than a lone dot.
const BASE: f32 = 0.08;

/// Fill `px` (one entry per LED) with the pattern at time `ms`.
pub fn render(pattern: Pattern, params: &Params, ms: u64, px: &mut [Rgb]) {
    let n = px.len();
    if n == 0 {
        return;
    }
    let phase = params.phase(ms);
    let last = (n - 1) as f32;
    let width = params.width();
    for (i, p) in px.iter_mut().enumerate() {
        let x = i as f32;
        let level = match pattern {
            Pattern::Sweep => BASE + (1.0 - BASE) * bump((x - last * ease(phase)) / width),
            Pattern::Comet => {
                // The head runs over n + tail positions so the tail clears the
                // far end before the head reappears at the start.
                let tail = 2.0 * width;
                let head = phase * (n as f32 + tail);
                let d = head - x;
                let glow = if (0.0..tail).contains(&d) { (1.0 - d / tail) * (1.0 - d / tail) } else { 0.0 };
                BASE + (1.0 - BASE) * glow
            }
            Pattern::Converge => {
                // Both glows reach the middle together, then return to the ends.
                let a = last / 2.0 * ease(phase);
                let glow = bump((x - a) / width).max(bump((x - (last - a)) / width));
                BASE + (1.0 - BASE) * glow
            }
            Pattern::Breathe => 0.1 + 0.9 * breathe(phase),
            Pattern::Wave => {
                let s = (1.0 + sin(2.0 * PI * (x / (2.0 * width) - phase))) / 2.0;
                BASE + (1.0 - BASE) * s * s
            }
            Pattern::Heartbeat => {
                // Two beats early in the cycle, the second smaller, then rest.
                let beat = |t: f32, at: f32, len: f32| bump((t - at) / len);
                let b = params.beat_phase(ms);
                let h = beat(b, 0.08, 0.06).max(0.6 * beat(b, 0.26, 0.06));
                0.1 + 0.9 * h
            }
            Pattern::Twinkle => BASE + (1.0 - BASE) * twinkle(i, ms, params),
        };
        *p = blend(params.secondary, params.primary, level);
    }
}

/// From `from` at level 0 to `to` at level 1.
fn blend(from: Rgb, to: Rgb, level: f32) -> Rgb {
    let v = level.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * v + 0.5) as u8;
    [mix(from[0], to[0]), mix(from[1], to[1]), mix(from[2], to[2])]
}

/// Parse `#rrggbb` (the `#` is optional) into a colour.
pub fn parse_color(s: &str) -> Option<Rgb> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// A smooth bump: 1 at 0, falling to 0 at +/- 1 and staying there.
fn bump(d: f32) -> f32 {
    if d.abs() < 1.0 { (1.0 - d * d) * (1.0 - d * d) } else { 0.0 }
}

/// 0 -> 1 -> 0 over one cycle, easing at both ends like a pendulum.
fn ease(phase: f32) -> f32 {
    (1.0 - cos(2.0 * PI * phase)) / 2.0
}

/// A breathing level over one cycle: eased, then squared so the fade looks
/// even to the eye.
fn breathe(phase: f32) -> f32 {
    let e = ease(phase);
    e * e
}

/// Each LED twinkles on its own cycle, between 0.6 and 1.4 times the base
/// period with a random offset, and lights for the first fifth of it.
fn twinkle(i: usize, ms: u64, params: &Params) -> f32 {
    let h = hash(i as u32);
    let period = params.period_ms() as f32 * (0.6 + 0.8 * unit(h));
    let t = (ms as f32 / period + unit(h.rotate_left(16))) % 1.0;
    if t < 0.2 { bump((t - 0.1) / 0.1) } else { 0.0 }
}

/// A well-mixed 32-bit hash of the LED index (fixed per LED).
fn hash(mut x: u32) -> u32 {
    x = x.wrapping_add(0x9E37_79B9);
    x = (x ^ (x >> 16)).wrapping_mul(0x85EB_CA6B);
    x = (x ^ (x >> 13)).wrapping_mul(0xC2B2_AE35);
    x ^ (x >> 16)
}

/// The top 24 bits of a hash as 0 to 1.
fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// cos() without std or libm: a Taylor series near 0, after folding x into
/// [-pi/2, pi/2]. Plenty for LED levels.
pub fn cos(x: f32) -> f32 {
    let mut x = x % (2.0 * PI);
    if x > PI {
        x -= 2.0 * PI;
    } else if x < -PI {
        x += 2.0 * PI;
    }
    // cos is even, and cos(x) = -cos(pi - x) keeps the series near 0.
    let (x, sign) = if x.abs() > PI / 2.0 { (PI - x.abs(), -1.0) } else { (x, 1.0) };
    let x2 = x * x;
    sign * (1.0 - x2 / 2.0 + x2 * x2 / 24.0 - x2 * x2 * x2 / 720.0)
}

pub fn sin(x: f32) -> f32 {
    cos(x - PI / 2.0)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn names_round_trip() {
        for p in Pattern::ALL {
            assert_eq!(Pattern::from_name(p.name()), Some(p));
        }
        assert_eq!(Pattern::from_name("SWEEP"), Some(Pattern::Sweep));
        assert_eq!(Pattern::from_name("disco"), None);
    }

    #[test]
    fn trig_is_close() {
        for k in -40..=40 {
            let x = k as f32 * 0.25;
            assert!((cos(x) - std::primitive::f32::cos(x)).abs() < 2e-3, "cos({x})");
            assert!((sin(x) - std::primitive::f32::sin(x)).abs() < 2e-3, "sin({x})");
        }
    }

    #[test]
    fn every_pattern_lights_every_led_and_stays_in_range() {
        let params = Params::default();
        for p in Pattern::ALL {
            let mut px = [[0u8; 3]; 60];
            for ms in (0..10_000).step_by(37) {
                render(p, &params, ms, &mut px);
                // The default primary has no red, and every pattern keeps a
                // floor of light.
                assert!(px.iter().all(|c| c[0] == 0 && c[2] > 0), "{p:?} at {ms}");
            }
        }
    }

    #[test]
    fn levels_blend_from_secondary_to_primary() {
        let (white, purple) = ([255, 255, 255], [128, 0, 128]);
        assert_eq!(blend(purple, white, 0.0), purple);
        assert_eq!(blend(purple, white, 1.0), white);
        assert_eq!(blend(purple, white, 0.5), [192, 128, 192]);
        // With both colours set, no LED is darker than the secondary.
        let params = Params { primary: white, secondary: purple, ..Params::default() };
        let mut px = [[0u8; 3]; 60];
        for p in Pattern::ALL {
            render(p, &params, 1234, &mut px);
            assert!(px.iter().all(|c| c[0] >= 128 && c[2] >= 128), "{p:?}");
        }
    }

    #[test]
    fn colors_parse() {
        assert_eq!(parse_color("#0040ff"), Some([0x00, 0x40, 0xff]));
        assert_eq!(parse_color("FF8000"), Some([0xff, 0x80, 0x00]));
        for bad in ["", "#fff", "#12345g", "#1234567", "blue"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn empty_strip_is_fine() {
        render(Pattern::Comet, &Params::default(), 1234, &mut []);
    }
}
