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

/// How the gradient from the primary to the secondary colour is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gradient {
    /// Round the colour wheel the short way, keeping colours vivid: blue to
    /// yellow passes cyan and green.
    Hue,
    /// A straight mix of the two: blue to yellow passes grey.
    Mix,
}

impl Gradient {
    pub fn name(self) -> &'static str {
        match self {
            Gradient::Hue => "hue",
            Gradient::Mix => "mix",
        }
    }

    pub fn from_name(name: &str) -> Option<Gradient> {
        [Gradient::Hue, Gradient::Mix].into_iter().find(|g| g.name().eq_ignore_ascii_case(name))
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
    /// How the primary turns into the secondary.
    pub gradient: Gradient,
    /// How much of the pattern is primary, 0 to 100: where along it the
    /// gradient is half way (50, the default, is the middle).
    pub balance: u32,
    /// How abrupt the change is, 0 (a smooth gradient over the whole
    /// pattern, the default) to 100 (a hard edge between two solid colours).
    pub sharpness: u32,
    /// The LEDs outside the pattern; black (the default) leaves them off.
    pub background: Rgb,
}

/// The default primary colour, a strong blue.
pub const DEFAULT_PRIMARY: Rgb = [0x00, 0x40, 0xff];

impl Default for Params {
    fn default() -> Self {
        Self { speed: 30, width: 8, primary: DEFAULT_PRIMARY, secondary: None, gradient: Gradient::Hue, balance: 50, sharpness: 0, background: [0, 0, 0] }
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
        let toward = shape(place, params.balance, params.sharpness);
        let color = match (params.secondary, params.gradient) {
            (Some(secondary), Gradient::Hue) => blend_hue(params.primary, secondary, toward),
            (Some(secondary), Gradient::Mix) => blend(params.primary, secondary, toward),
            (None, _) => params.primary,
        };
        *p = blend(params.background, color, glow);
    }
}

/// How far a place within the pattern (0 to 1) is toward the secondary colour.
/// Balance moves the half-way point along the pattern, like a gradient's
/// midpoint in an image editor (the ends stay put); sharpness then narrows the
/// blend around that point, down to a step at 100. At the defaults (50, 0) the
/// place passes through unchanged.
fn shape(place: f32, balance: u32, sharpness: u32) -> f32 {
    let t = place.clamp(0.0, 1.0);
    let mid = (balance.min(100) as f32 / 100.0).clamp(0.01, 0.99);
    let u = if t < mid { 0.5 * t / mid } else { 0.5 + 0.5 * (t - mid) / (1.0 - mid) };
    let width = (1.0 - sharpness.min(100) as f32 / 100.0).max(0.01);
    ((u - 0.5) / width + 0.5).clamp(0.0, 1.0)
}

/// From `from` at 0 to `to` at 1.
fn blend(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    let v = amount.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * v + 0.5) as u8;
    [mix(from[0], to[0]), mix(from[1], to[1]), mix(from[2], to[2])]
}

/// From `from` at 0 to `to` at 1 round the colour wheel the short way, with
/// saturation and brightness changing linearly, so a gradient between distant
/// hues stays vivid. A grey end has no hue of its own and takes the other's.
fn blend_hue(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    let v = amount.clamp(0.0, 1.0);
    let (h0, s0, v0) = to_hsv(from);
    let (h1, s1, v1) = to_hsv(to);
    let (h0, h1) = match (s0 > 0.0, s1 > 0.0) {
        (false, true) => (h1, h1),
        (true, false) => (h0, h0),
        _ => (h0, h1),
    };
    // The shorter way round: a difference of more than half a turn goes the
    // other way.
    let mut dh = h1 - h0;
    if dh > 180.0 {
        dh -= 360.0;
    } else if dh < -180.0 {
        dh += 360.0;
    }
    from_hsv((h0 + dh * v + 360.0) % 360.0, s0 + (s1 - s0) * v, v0 + (v1 - v0) * v)
}

/// RGB to (hue in degrees, saturation 0-1, value 0-1).
fn to_hsv(c: Rgb) -> (f32, f32, f32) {
    let [r, g, b] = c.map(|x| f32::from(x) / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (if h < 0.0 { h + 360.0 } else { h }, if max == 0.0 { 0.0 } else { d / max }, max)
}

fn from_hsv(h: f32, s: f32, v: f32) -> Rgb {
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let byte = |u: f32| ((u + m) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
    [byte(r), byte(g), byte(b)]
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
    fn hue_gradient_stays_vivid() {
        let (blue, yellow) = ([0x00, 0x40, 0xff], [0xff, 0xff, 0x00]);
        // Both ends come back as given.
        assert_eq!(blend_hue(blue, yellow, 0.0), blue);
        assert_eq!(blend_hue(blue, yellow, 1.0), yellow);
        // Half way is a saturated green-cyan, where a straight mix is grey.
        let hue_mid = blend_hue(blue, yellow, 0.5);
        let mix_mid = blend(blue, yellow, 0.5);
        let spread = |c: Rgb| c.iter().max().unwrap() - c.iter().min().unwrap();
        assert!(hue_mid[1] > 200 && spread(hue_mid) > 150, "hue midpoint {hue_mid:?}");
        assert!(spread(mix_mid) < 100, "mix midpoint {mix_mid:?}");
        // Red to white keeps red's hue and loses saturation: pink, not a rainbow.
        let pink = blend_hue([255, 0, 0], [255, 255, 255], 0.5);
        assert!(pink[0] == 255 && pink[1] == pink[2] && pink[1] > 100, "red to white {pink:?}");
        assert_eq!(Gradient::from_name("MIX"), Some(Gradient::Mix));
        assert_eq!(Gradient::from_name("rainbow"), None);
    }

    #[test]
    fn gradient_shape() {
        // The defaults change nothing.
        for k in 0..=20 {
            let t = k as f32 / 20.0;
            assert!((shape(t, 50, 0) - t).abs() < 1e-6, "{t}");
        }
        // Balance moves the half-way point; the ends stay put.
        assert!((shape(0.8, 80, 0) - 0.5).abs() < 1e-6);
        assert_eq!((shape(0.0, 80, 0), shape(1.0, 80, 0)), (0.0, 1.0));
        assert!(shape(0.5, 80, 0) < 0.5, "more primary at balance 80");
        // Full sharpness is a step at the balance point.
        assert_eq!((shape(0.69, 70, 100), shape(0.71, 70, 100)), (0.0, 1.0));
        // In between, sharper means a steeper change around the midpoint.
        assert!(shape(0.6, 50, 60) > shape(0.6, 50, 0));
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
