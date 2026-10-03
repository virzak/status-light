//! Wispy blue flame: thin glowing ribbons that rise, sway and curl, like
//! burning gas.
//!
//! Each strand is a ribbon whose centre line sways with two travelling waves
//! that run upwards, so the shapes rise. The ribbon's two edges sit either side
//! of the centre at a half-width that itself oscillates through zero, so the
//! ribbon twists: its edges cross and the sheet between them narrows to a line.
//! Edges are drawn as thin glowing lines and the sheet as a faint fill, all
//! added together, so where strands overlap they brighten towards cyan and
//! white. Strands live a few seconds, fade out and respawn elsewhere; the
//! tallest grow near the middle, giving a mound.
//!
//! Rendering goes row by row into a one-row accumulator, so the caller only
//! supplies the output pixels. Pure `no_std`, no allocation, no float library.

use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};

/// Most strands a [`Wisps`] can hold.
pub const MAX_STRANDS: usize = 64;
/// Widest image a [`Wisps`] can render.
pub const MAX_W: usize = 512;

/// Tunable parameters, all 0-100 style integers so they travel as `S` lines.
/// `Default` is the look that ships.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WispParams {
    /// Number of ribbons, 1 to [`MAX_STRANDS`].
    pub strands: u8,
    /// Height of the tallest ribbons, percent of the screen height.
    pub height: u8,
    /// How far ribbons swing sideways, 0 (straight) to 100.
    pub sway: u8,
    /// Animation speed, 50 is normal.
    pub speed: u8,
    /// Brightness of the glow, 50 is normal.
    pub glow: u8,
    /// Ribbon width, 50 is normal.
    pub width: u8,
}

impl Default for WispParams {
    fn default() -> Self {
        Self {
            strands: 48,
            height: 90,
            sway: 50,
            speed: 50,
            glow: 50,
            width: 50,
        }
    }
}

const PI: f32 = core::f32::consts::PI;
const TAU: f32 = core::f32::consts::TAU;

fn floor(v: f32) -> f32 {
    let i = v as i32 as f32;
    if i > v {
        i - 1.0
    } else {
        i
    }
}

/// Sine to about 0.1%, without libm.
fn sin(x: f32) -> f32 {
    let x = x - TAU * floor(x * (1.0 / TAU) + 0.5);
    let y = (4.0 / PI) * x - (4.0 / (PI * PI)) * x * x.abs();
    0.225 * (y * y.abs() - y) + y
}

/// 1/x for x > 0, without a divide: the ESP32-S3's FPU has none, so `/`
/// becomes a slow library call in the per-row loops.
fn recip(x: f32) -> f32 {
    let r = f32::from_bits(0x7EF3_11C7u32.wrapping_sub(x.to_bits()));
    let r = r * (2.0 - x * r);
    r * (2.0 - x * r)
}

fn smoothstep(e0: f32, e1: f32, v: f32) -> f32 {
    let t = ((v - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Black, deep blue, blue, cyan, white: the colour for accumulated glow.
fn palette_color(i: u32) -> Rgb565 {
    // (glow index, r, g, b) keyframes, linearly interpolated.
    const KEYS: [(u32, u32, u32, u32); 6] = [
        (0, 0, 0, 0),
        (30, 0, 8, 70),
        (80, 0, 60, 200),
        (130, 0, 130, 255),
        (190, 30, 220, 255),
        (255, 210, 255, 255),
    ];
    let mut k = 1;
    while k < KEYS.len() - 1 && i > KEYS[k].0 {
        k += 1;
    }
    let (i0, r0, g0, b0) = KEYS[k - 1];
    let (i1, r1, g1, b1) = KEYS[k];
    let t = i.clamp(i0, i1) - i0;
    let span = i1 - i0;
    let mix = |a: u32, b: u32| (a * (span - t) + b * t) / span;
    let (r, g, b) = (mix(r0, r1), mix(g0, g1), mix(b0, b1));
    Rgb565::new((r >> 3) as u8, (g >> 2) as u8, (b >> 3) as u8)
}

#[derive(Clone, Copy)]
struct Strand {
    /// Base x, pixels.
    x0: f32,
    /// Full height, pixels.
    height: f32,
    /// Sideways lean at the top, pixels.
    lean: f32,
    /// Two travelling waves: amplitude (px), wavenumber (rad/px),
    /// angular speed (rad/s), phase.
    a1: f32,
    k1: f32,
    w1: f32,
    p1: f32,
    a2: f32,
    k2: f32,
    w2: f32,
    p2: f32,
    /// Ribbon half-width (px) and its twist wave.
    rw: f32,
    kt: f32,
    wt: f32,
    pt: f32,
    /// Peak brightness, 1.0 = one full glow step.
    bright: f32,
    /// Seconds since spawn (negative while waiting to appear) and lifetime.
    age: f32,
    life: f32,
    /// sin and cos of each wavenumber (k1, k2, kt): one row's phase step.
    rot: [f32; 6],
}

const EMPTY: Strand = Strand {
    x0: 0.0,
    height: 0.0,
    lean: 0.0,
    a1: 0.0,
    k1: 0.0,
    w1: 0.0,
    p1: 0.0,
    a2: 0.0,
    k2: 0.0,
    w2: 0.0,
    p2: 0.0,
    rw: 0.0,
    kt: 0.0,
    wt: 0.0,
    pt: 0.0,
    bright: 0.0,
    age: 0.0,
    life: 1.0,
    rot: [0.0; 6],
};

/// Seconds a strand takes to fade in and to fade out.
const FADE_IN: f32 = 1.0;
const FADE_OUT: f32 = 1.5;
/// Glow units per 1.0 of brightness in the row accumulator.
const UNIT: f32 = 1024.0;

/// The wisp animation: strand state, parameters and the RNG. The caller owns
/// the pixel buffer.
pub struct Wisps {
    w: usize,
    h: usize,
    params: WispParams,
    palette: [Rgb565; 256],
    strands: [Strand; MAX_STRANDS],
    /// Animation time, seconds.
    t: f32,
    rng: u32,
    row: [u32; MAX_W],
    /// Line profile by squared distance, see [`add_line`].
    kernel: [u16; KERNEL_LEN],
}

impl Wisps {
    /// `w * h` is the size of the pixel buffer passed to [`Wisps::render`];
    /// `w` is at most [`MAX_W`]. `seed` seeds the RNG (any value).
    pub fn new(w: usize, h: usize, params: WispParams, seed: u32) -> Self {
        assert!(w <= MAX_W);
        let mut palette = [Rgb565::BLACK; 256];
        for (i, c) in palette.iter_mut().enumerate() {
            *c = palette_color(i as u32);
        }
        let mut s = Self {
            w,
            h,
            params,
            palette,
            strands: [EMPTY; MAX_STRANDS],
            t: 0.0,
            rng: seed | 1,
            row: [0; MAX_W],
            kernel: [0; KERNEL_LEN],
        };
        for (i, k) in s.kernel.iter_mut().enumerate() {
            // A sharp core with a soft halo.
            let f = 1.0 / (1.0 + i as f32 / KERNEL_STEPS);
            *k = (f * f * 65535.0) as u16;
        }
        // Past the reach: nothing, so clamped indices add no glow.
        s.kernel[KERNEL_LEN - 1] = 0;
        s.reset();
        s
    }

    pub fn params(&self) -> &WispParams {
        &self.params
    }

    /// Replace the parameters. Strands pick up shape changes as they respawn;
    /// speed, glow and sway apply at once.
    pub fn set_params(&mut self, params: WispParams) {
        let respawn = params.strands != self.params.strands
            || params.height != self.params.height
            || params.width != self.params.width;
        self.params = params;
        if respawn {
            self.reset();
        }
    }

    fn next(&mut self) -> u32 {
        // xorshift32: deterministic and identical on board and PC.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// Uniform in `lo..hi`.
    fn rand(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * ((self.next() >> 8) as f32 / (1u32 << 24) as f32)
    }

    fn count(&self) -> usize {
        (self.params.strands as usize).clamp(1, MAX_STRANDS)
    }

    /// Start over with every strand fading in, so the flame grows in. Call
    /// when the flame (re)starts.
    pub fn reset(&mut self) {
        for i in 0..self.count() {
            self.spawn(i);
            // Stagger the first appearance and the first respawns.
            let s = &mut self.strands[i];
            s.age = -(i as f32 % 7.0) * 0.15;
            s.life *= 0.6 + 0.4 * ((i * 37 % 11) as f32 / 10.0);
        }
    }

    fn spawn(&mut self, i: usize) {
        let (w, h) = (self.w as f32, self.h as f32);
        let p = self.params;
        // Screens of different sizes look alike: shapes scale with the height.
        let scale = h / 170.0;
        // Spread evenly across the width, with jitter, so no gaps open up.
        let n = self.count() as f32;
        let slot = (i * 29 % self.count()) as f32;
        let x0 = (slot + self.rand(-0.8, 1.8)) / n * 1.08 * w - 0.04 * w;
        // The mound: full height in the middle, about a third at the edges.
        let off = (x0 - 0.5 * w) / (0.62 * w);
        let mound = (1.0 - off * off).max(0.0);
        // The faint tips run past the top of the tallest ones' `height`.
        let tall = 1.2 * h * p.height.min(100) as f32 / 100.0;
        let height = tall * (0.25 + 0.75 * mound) * self.rand(0.7, 1.0);
        let wave = |s: &mut Self, len_lo: f32, len_hi: f32| {
            let len = s.rand(len_lo, len_hi) * scale;
            (TAU / len, s.rand(0.0, TAU))
        };
        let (k1, p1) = wave(self, 100.0, 220.0);
        let (k2, p2) = wave(self, 40.0, 80.0);
        let (kt, pt) = wave(self, 90.0, 220.0);
        let rw = self.rand(3.0, 12.0) * scale * p.width.min(100) as f32 / 50.0;
        let s = Strand {
            x0,
            height,
            // Tips lean in towards the middle, as in a real flame.
            lean: (0.5 * w - x0) * self.rand(0.0, 0.35) + self.rand(-0.15, 0.15) * height,
            a1: self.rand(14.0, 32.0) * scale,
            k1,
            // The waves run upwards at about 25-60 px/s on a 170-row screen.
            w1: k1 * self.rand(25.0, 50.0) * scale,
            p1,
            a2: self.rand(2.0, 6.0) * scale,
            k2,
            w2: k2 * self.rand(35.0, 60.0) * scale,
            p2,
            rw,
            kt,
            wt: self.rand(-1.5, 1.5),
            pt,
            bright: self.rand(0.35, 1.0) * (0.6 + 0.4 * mound),
            age: 0.0,
            life: self.rand(3.0, 7.0),
            rot: [
                sin(k1),
                sin(k1 + 0.5 * PI),
                sin(k2),
                sin(k2 + 0.5 * PI),
                sin(kt),
                sin(kt + 0.5 * PI),
            ],
        };
        self.strands[i] = s;
    }

    /// Advance the animation by `dt_ms` milliseconds of wall time.
    pub fn step(&mut self, dt_ms: u32) {
        let dt = dt_ms.min(200) as f32 / 1000.0 * self.params.speed as f32 / 50.0;
        self.t += dt;
        // Keep the phase arguments small so f32 keeps its precision.
        if self.t > 3600.0 {
            self.t -= 3600.0;
        }
        for i in 0..self.count() {
            self.strands[i].age += dt;
            if self.strands[i].age > self.strands[i].life {
                self.spawn(i);
            }
        }
    }

    /// Draw the current frame into `out`, `w * h` pixels, row-major.
    #[inline(never)]
    pub fn render(&mut self, out: &mut [Rgb565]) {
        let (w, h) = (self.w, self.h);
        let n = self.count();
        let t = self.t;
        let sway = self.params.sway.min(100) as f32 / 50.0;
        let glow = self.params.glow as f32 / 50.0;

        // Per-frame strand state: fade, 1/height, the phase of each wave at
        // t, and the waves' sin/cos (s1, c1, s2, c2, st, ct) on the current
        // row. Rows run top-down, so each wave steps back by its wavenumber
        // per row: a rotation instead of three sines per strand per row.
        let mut fade = [0.0f32; MAX_STRANDS];
        let mut inv_h = [0.0f32; MAX_STRANDS];
        let mut ph = [[0.0f32; 3]; MAX_STRANDS];
        let mut wave = [[0.0f32; 6]; MAX_STRANDS];
        let mut started = [false; MAX_STRANDS];
        // The edges on the row above, for the slope.
        let mut prev = [[0.0f32; 2]; MAX_STRANDS];
        for i in 0..n {
            let s = &self.strands[i];
            fade[i] = smoothstep(0.0, FADE_IN, s.age)
                * smoothstep(s.life, s.life - FADE_OUT, s.age)
                * s.bright
                * glow
                * UNIT;
            inv_h[i] = 1.0 / s.height.max(1.0);
            ph[i] = [s.p1 - s.w1 * t, s.p2 - s.w2 * t, s.pt - s.wt * t];
        }

        for row in 0..h {
            self.row[..w].fill(0);
            // Height above the bottom edge, pixels.
            let yu = (h - 1 - row) as f32;
            for i in 0..n {
                if fade[i] <= 1.0 {
                    continue;
                }
                let s = &self.strands[i];
                let v = yu * inv_h[i];
                if v >= 1.0 {
                    continue;
                }
                let wv = &mut wave[i];
                if started[i] {
                    let r = &s.rot;
                    for k in 0..3 {
                        let (sn, cs) = (wv[2 * k], wv[2 * k + 1]);
                        wv[2 * k] = sn * r[2 * k + 1] - cs * r[2 * k];
                        wv[2 * k + 1] = cs * r[2 * k + 1] + sn * r[2 * k];
                    }
                } else {
                    // The strand's first row: start the waves, and find the
                    // edges on the row above it for the slope.
                    let ks = [s.k1, s.k2, s.kt];
                    for k in 0..3 {
                        let a = ks[k] * yu + ph[i][k];
                        wv[2 * k] = sin(a);
                        wv[2 * k + 1] = sin(a + 0.5 * PI);
                    }
                    let a = |k: usize| ks[k] * (yu + 1.0) + ph[i][k];
                    prev[i] = s.edges((yu + 1.0) * inv_h[i], sway, sin(a(0)), sin(a(1)), sin(a(2)));
                    started[i] = true;
                }
                let cur = s.edges(v, sway, wv[0], wv[2], wv[4]);
                let above = prev[i];
                prev[i] = cur;
                // Bright at the base, thinning out towards the top.
                let top = (2.0 * v - 1.0).max(0.0);
                let b = fade[i] * (1.0 - top * top * (3.0 - 2.0 * top));
                for e in 0..2 {
                    let slope = (cur[e] - above[e]).abs();
                    add_line(&mut self.row[..w], &self.kernel, cur[e], slope, b);
                }
                // The faint sheet between the edges.
                add_span(&mut self.row[..w], cur[0], cur[1], b * 0.22);
            }
            let line = &mut out[row * w..(row + 1) * w];
            for (o, &a) in line.iter_mut().zip(self.row[..w].iter()) {
                *o = self.palette[(a >> 3).min(255) as usize];
            }
        }
    }
}

impl Strand {
    /// The ribbon's two edges at relative height `v`, given the sines of its
    /// two sway waves and its twist wave there.
    #[inline]
    fn edges(&self, v: f32, sway: f32, s1: f32, s2: f32, st: f32) -> [f32; 2] {
        let env = v * (0.35 + 0.65 * v);
        let x = self.x0 + self.lean * v * v + sway * env * (self.a1 * s1 + self.a2 * s2);
        let half = self.rw * (0.4 + v) * st;
        [x - half, x + half]
    }
}

/// Kernel entries per unit of squared distance (in sigmas), out to three
/// sigmas, past which the glow rounds to nothing.
const KERNEL_STEPS: f32 = 16.0;
const KERNEL_LEN: usize = 16 * 9 + 1;

/// Add a glowing line crossing this row at `x`. `slope` is the horizontal run
/// per row: a flat line covers more of the row, so its profile stretches.
/// The profile comes from `kernel`, as the ESP32-S3 has no float divide.
fn add_line(row: &mut [u32], kernel: &[u16; KERNEL_LEN], x: f32, slope: f32, b: f32) {
    // Approximate hypot(1, slope), capped: an almost flat line would smear
    // across the whole row.
    let stretch = if slope > 1.0 {
        (slope + 0.41).min(6.0)
    } else {
        1.0 + 0.41 * slope
    };
    let sigma = 0.9 * stretch;
    let reach = (3.0 * sigma) as i32 + 1;
    // Fixed point from here: positions in 1/16 px.
    let xq = (x * 16.0) as i32;
    let xi = xq >> 4;
    let lo = (xi - reach).max(0);
    let hi = (xi + reach).min(row.len() as i32 - 1);
    if lo > hi {
        return;
    }
    // Kernel index = d^2 * KERNEL_STEPS / sigma^2, d in 1/16 px, as 16.16.
    // d^2 is quadratic in px, so step it with constant second differences.
    let inv = recip(sigma);
    let scale = (KERNEL_STEPS * inv * inv * 256.0) as i32;
    let d = lo * 16 + 8 - xq;
    let mut q = d * d * scale;
    let mut dq = (32 * d + 256) * scale;
    let ddq = 512 * scale;
    let b = b as u32;
    for a in &mut row[lo as usize..hi as usize + 1] {
        let k = ((q >> 16) as usize).min(KERNEL_LEN - 1);
        *a += (b * kernel[k] as u32) >> 16;
        q += dq;
        dq += ddq;
    }
}

/// Add a flat fill from `x0` to `x1` (either order).
fn add_span(row: &mut [u32], x0: f32, x1: f32, b: f32) {
    let (a, z) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
    let lo = (a as i32 + 1).max(0);
    let hi = (z as i32).min(row.len() as i32 - 1);
    if lo > hi {
        return;
    }
    let v = b as u32;
    for a in &mut row[lo as usize..hi as usize + 1] {
        *a += v;
    }
}
