//! LED-strip patterns for the online state, shared by the ESP32-S3-Zero
//! firmware and the PC preview so both draw identical frames.
//!
//! Pure `no_std`, no allocation and no state: a frame is a function of the time,
//! the strip length and the [`Params`], so switching pattern takes effect on the
//! next frame. Each pattern gives every LED a glow (how much it takes part in
//! the pattern, 0 to 1) and a place within the pattern (0 to 1, such as a
//! comet's head to the end of its tail). The place picks a colour on a gradient
//! from the primary to the secondary; the glow blends that over the background,
//! which is black (off) unless set.
//! Colours are full scale; the caller scales them to its brightness.

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
    /// The pattern's colour: at its head, centre or crest.
    pub primary: Rgb,
    /// Where the pattern's gradient ends: its tail or edges. `None` keeps the
    /// whole pattern in the primary.
    pub secondary: Option<Rgb>,
    /// The LEDs outside the pattern; black (the default) leaves them off.
    pub background: Rgb,
}

/// The default primary colour, a strong blue.
pub const DEFAULT_PRIMARY: Rgb = [0x00, 0x40, 0xff];

impl Default for Params {
    fn default() -> Self {
        Self { speed: 30, width: 8, primary: DEFAULT_PRIMARY, secondary: None, background: [0, 0, 0] }
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

/// Fill `px` (one entry per LED) with the pattern at time `ms`.
pub fn render(pattern: Pattern, params: &Params, ms: u64, px: &mut [Rgb]) {
    let n = px.len();
    if n == 0 {
        return;
    }
    let phase = params.phase(ms);
    let last = (n - 1) as f32;
    let width = params.width();
    // Along the strip, for the patterns that fill all of it.
    let along = |x: f32| if last > 0.0 { x / last } else { 0.0 };
    for (i, p) in px.iter_mut().enumerate() {
        let x = i as f32;
        // (glow, place within the pattern)
        let (glow, place) = match pattern {
            Pattern::Sweep => {
                let d = (x - last * ease(phase)) / width;
                (bump(d), d.abs())
            }
            Pattern::Comet => {
                // The head runs over n + tail positions so the tail clears the
                // far end before the head reappears at the start.
                let tail = 2.0 * width;
                let d = phase * (n as f32 + tail) - x;
                if (0.0..tail).contains(&d) { ((1.0 - d / tail) * (1.0 - d / tail), d / tail) } else { (0.0, 1.0) }
            }
            Pattern::Converge => {
                // Both glows reach the middle together, then return to the ends;
                // each LED belongs to the nearer one.
                let a = last / 2.0 * ease(phase);
                let d = ((x - a) / width).abs().min(((x - (last - a)) / width).abs());
                (bump(d), d)
            }
            Pattern::Breathe => (breathe(phase), along(x)),
            Pattern::Wave => {
                let s = (1.0 + sin(2.0 * PI * (x / (2.0 * width) - phase))) / 2.0;
                (s * s, 1.0 - s)
            }
            Pattern::Heartbeat => {
                // Two beats early in the cycle, the second smaller, then rest.
                let beat = |t: f32, at: f32, len: f32| bump((t - at) / len);
                let b = params.beat_phase(ms);
                (beat(b, 0.08, 0.06).max(0.6 * beat(b, 0.26, 0.06)), along(x))
            }
            // Each sparkle keeps its own fixed place on the gradient.
            Pattern::Twinkle => (twinkle(i, ms, params), unit(hash(i as u32).rotate_left(8))),
        };
        let color = match params.secondary {
            Some(secondary) => blend(params.primary, secondary, place),
            None => params.primary,
        };
        *p = blend(params.background, color, glow);
    }
}

/// From `from` at 0 to `to` at 1.
fn blend(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    let v = amount.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * v + 0.5) as u8;
    [mix(from[0], to[0]), mix(from[1], to[1]), mix(from[2], to[2])]
}

/// Parse `#rrggbb`, or the short `#rgb` where each digit doubles (`#f80` is
/// `#ff8800`), into a colour. The `#` is optional.
pub fn parse_color(s: &str) -> Option<Rgb> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok();
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17]),
        6 => Some([byte(0)?, byte(2)?, byte(4)?]),
        _ => None,
    }
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
    fn every_pattern_lights_some_leds_over_time() {
        let params = Params::default();
        for p in Pattern::ALL {
            let mut px = [[0u8; 3]; 60];
            let mut lit = 0;
            for ms in (0..10_000).step_by(37) {
                render(p, &params, ms, &mut px);
                // The default primary has no red.
                assert!(px.iter().all(|c| c[0] == 0), "{p:?} at {ms}");
                lit += px.iter().filter(|c| c[2] > 128).count();
            }
            assert!(lit > 0, "{p:?} never lit");
        }
    }

    #[test]
    fn blend_mixes_linearly() {
        let (white, purple) = ([255, 255, 255], [128, 0, 128]);
        assert_eq!(blend(purple, white, 0.0), purple);
        assert_eq!(blend(purple, white, 1.0), white);
        assert_eq!(blend(purple, white, 0.5), [192, 128, 192]);
    }

    #[test]
    fn comet_runs_from_primary_at_the_head_to_secondary_in_the_tail() {
        let (red, green) = ([255, 0, 0], [0, 255, 0]);
        let params = Params { primary: red, secondary: Some(green), ..Params::default() };
        let mut px = [[0u8; 3]; 60];
        // A third of the way through the cycle the head is mid-strip.
        render(Pattern::Comet, &params, params.period_ms() / 3, &mut px);
        let head = (0..60).max_by_key(|&i| px[i][0] as u32 + px[i][1] as u32).unwrap();
        assert!(px[head][0] > 200 && px[head][1] < 40, "head {:?}", px[head]);
        // Three quarters of the way down the 16-LED tail, the secondary leads.
        let tail = px[head - 12];
        assert!(tail[1] > tail[0], "tail {tail:?}");
    }

    #[test]
    fn background_shows_exactly_outside_the_pattern() {
        let purple = [80, 0, 160];
        let mut px = [[0u8; 3]; 60];
        for background in [purple, [0, 0, 0]] {
            let params = Params { background, ..Params::default() };
            for p in [Pattern::Sweep, Pattern::Comet, Pattern::Converge] {
                render(p, &params, 1234, &mut px);
                assert!(px.iter().filter(|&&c| c == background).count() > 20, "{p:?} on {background:?}");
            }
        }
    }

    #[test]
    fn colors_parse() {
        assert_eq!(parse_color("#0040ff"), Some([0x00, 0x40, 0xff]));
        assert_eq!(parse_color("FF8000"), Some([0xff, 0x80, 0x00]));
        assert_eq!(parse_color("#f80"), Some([0xff, 0x88, 0x00]));
        assert_eq!(parse_color("#FFF"), Some([0xff, 0xff, 0xff]));
        for bad in ["", "#ff", "#ffff", "#12345g", "#1234567", "#fgf", "blue"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn empty_strip_is_fine() {
        render(Pattern::Comet, &Params::default(), 1234, &mut []);
    }
}
