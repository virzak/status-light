//! Shared blue-flame effect for the internet-status display.
//!
//! Pure `no_std`, no allocation: the caller owns the heat and pixel buffers.
//! Runs identically on the ESP32-S3 board and in the PC preview, so the flame
//! can be tuned on the PC and the same code ships to hardware.
//!
//! The heat field is a Doom-style fire: the bottom row is seeded hot each frame
//! and heat rises, cooling and drifting, until it fades to black. A blue palette
//! maps heat to colour (black, navy, blue, cyan, white tip).

#![no_std]

use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};

/// Tunable flame parameters. `Default` is the look that ships on the board.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlameParams {
    /// Extra heat lost per upward step. Higher = shorter flames.
    pub cooling: u8,
    /// Horizontal drift while rising, 0 (straight up) to 3 (lively).
    pub drift: u8,
    /// Bottom-row heat for a "cool" cell (the flicker low value).
    pub seed_min: u8,
    /// Bottom-row heat for a "hot" cell (the flicker high value).
    pub seed_max: u8,
    /// One in `flicker` bottom cells uses `seed_min`; the rest use `seed_max`.
    pub flicker: u8,
    /// Heat at which blue reaches full brightness.
    pub blue_full: u8,
    /// Heat at which green starts to join (cyan as it gets hotter).
    pub green_start: u8,
    /// Heat at which red starts to join (the white flame tip).
    pub white_start: u8,
}

impl Default for FlameParams {
    fn default() -> Self {
        Self {
            cooling: 0,
            drift: 3,
            seed_min: 170,
            seed_max: 255,
            flicker: 8,
            blue_full: 170,
            green_start: 100,
            white_start: 200,
        }
    }
}

fn palette_color(p: &FlameParams, h: u8) -> Rgb565 {
    let hv = h as u32;
    let blue_full = p.blue_full.max(1) as u32;
    let b = (hv * 255 / blue_full).min(255);
    let g = if hv > p.green_start as u32 {
        ((hv - p.green_start as u32) * 255 / (255 - p.green_start as u32).max(1) as u32).min(255)
    } else {
        0
    };
    let r = if hv > p.white_start as u32 {
        ((hv - p.white_start as u32) * 255 / (255 - p.white_start as u32).max(1) as u32).min(255)
    } else {
        0
    };
    // 8-bit channels down to RGB565.
    Rgb565::new((r >> 3) as u8, (g >> 2) as u8, (b >> 3) as u8)
}

/// The flame simulator: owns the parameters, the derived colour palette, and the
/// RNG state. The heat and pixel buffers stay with the caller (PSRAM on the
/// board, a `Vec` on the PC), so this needs no allocator.
pub struct Flame {
    w: usize,
    h: usize,
    params: FlameParams,
    palette: [Rgb565; 256],
    rng: u32,
}

impl Flame {
    /// `w * h` is the size of the heat and pixel buffers passed to [`Flame::step`]
    /// and [`Flame::render`]. `seed` seeds the internal RNG (any non-zero value).
    pub fn new(w: usize, h: usize, params: FlameParams, seed: u32) -> Self {
        let mut f = Self {
            w,
            h,
            params,
            palette: [Rgb565::BLACK; 256],
            rng: seed | 1,
        };
        f.rebuild_palette();
        f
    }

    pub fn params(&self) -> &FlameParams {
        &self.params
    }

    /// Replace the parameters and rebuild the palette (used by the PC tuner).
    pub fn set_params(&mut self, params: FlameParams) {
        self.params = params;
        self.rebuild_palette();
    }

    fn rebuild_palette(&mut self) {
        let mut i = 0;
        while i < 256 {
            self.palette[i] = palette_color(&self.params, i as u8);
            i += 1;
        }
    }

    #[inline]
    fn next(&mut self) -> u32 {
        // xorshift32: deterministic and identical on board and PC.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// Cool the whole field to black. Call when the flame (re)starts.
    pub fn reset(&mut self, heat: &mut [u8]) {
        for v in heat.iter_mut() {
            *v = 0;
        }
    }

    /// Advance one frame. `heat` is `w * h`, row-major; the bottom row is the base.
    pub fn step(&mut self, heat: &mut [u8]) {
        let (w, h) = (self.w, self.h);
        let base = (h - 1) * w;
        let flicker = self.params.flicker.max(1) as u32;
        for x in 0..w {
            heat[base + x] = if self.next() % flicker == 0 {
                self.params.seed_min
            } else {
                self.params.seed_max
            };
        }
        let drift_max = self.params.drift as usize;
        for x in 0..w {
            for y in 1..h {
                let src = y * w + x;
                let pixel = heat[src];
                if pixel == 0 {
                    heat[src - w] = 0;
                    continue;
                }
                let r = self.next();
                let drift = ((r & 3) as usize).min(drift_max);
                let decay = (r & 1) as u8 + self.params.cooling;
                let dst = src + 1;
                if dst >= drift + w {
                    heat[dst - drift - w] = pixel.saturating_sub(decay);
                }
            }
        }
    }

    /// Map the heat field into RGB565 pixels. `out` is `w * h`, row-major.
    pub fn render(&self, heat: &[u8], out: &mut [Rgb565]) {
        for (o, &hh) in out.iter_mut().zip(heat.iter()) {
            *o = self.palette[hh as usize];
        }
    }
}
