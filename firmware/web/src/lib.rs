//! The shared flame and strip effects as WebAssembly, so the settings page can
//! preview them with the code the boards run.
//!
//! Plain exported functions over numbers, so the page loads it with the
//! browser's own `WebAssembly` API and no generated glue. Each instance holds
//! one wisp animation and one strip; the page makes an instance per preview.
//! A numeric argument below zero means "the default", like an empty field on
//! the page. Frames are written to fixed buffers in the instance's memory and
//! the functions return where they start.

#![no_std]

use core::cell::UnsafeCell;
use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
use flame::wisps::{WispParams, Wisps};
use strip::{Edge, Gradient, Params, Pattern, Rgb};

/// The largest display previewed: the Flint 4's LCD, landscape.
const MAX_PIXELS: usize = 320 * 240;
/// The most LEDs a strip can have (the Zero's limit).
const MAX_LEDS: usize = 300;

/// A static the exports share. WebAssembly instances here are single-threaded
/// and the exports never re-enter, so there is only ever one borrow.
struct Global<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Global<T> {}
impl<T> Global<T> {
    const fn new(v: T) -> Self {
        Self(UnsafeCell::new(v))
    }
    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}

static WISPS: Global<Option<Wisps>> = Global::new(None);
static SCREEN: Global<[Rgb565; MAX_PIXELS]> = Global::new([Rgb565::BLACK; MAX_PIXELS]);
static RGBA: Global<[u8; MAX_PIXELS * 4]> = Global::new([0; MAX_PIXELS * 4]);
static STRIP: Global<(Pattern, Params)> = Global::new((Pattern::Sweep, Params {
    speed: 30,
    width: 8,
    primary: strip::DEFAULT_PRIMARY,
    secondary: None,
    gradient: Gradient::Hue,
    balance: 50,
    sharpness: 0,
    edge: Edge::Soft,
    background: [0, 0, 0],
}));
static LEDS: Global<[Rgb; MAX_LEDS]> = Global::new([[0; 3]; MAX_LEDS]);

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

/// `v` as a setting, or `default` when it is below zero.
fn or<T: TryFrom<i32>>(v: i32, default: T) -> T {
    if v < 0 { default } else { T::try_from(v).unwrap_or(default) }
}

/// `0xrrggbb` as a colour; `None` when below zero.
fn color(v: i32) -> Option<Rgb> {
    (v >= 0).then(|| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Start a wisp animation `w` by `h` pixels with the default tuning. Returns 0
/// when that is larger than the preview supports.
#[no_mangle]
pub extern "C" fn wisps_new(w: u32, h: u32) -> u32 {
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || w * h > MAX_PIXELS || w > flame::wisps::MAX_W {
        return 0;
    }
    *WISPS.get() = Some(Wisps::new(w, h, WispParams::default(), 0xC0FF_EE11));
    1
}

/// Set the wisp tuning (`WispParams`, the page's Flame tab).
#[no_mangle]
pub extern "C" fn wisps_params(strands: i32, height: i32, sway: i32, speed: i32, glow: i32, width: i32) {
    if let Some(w) = WISPS.get() {
        let d = WispParams::default();
        w.set_params(WispParams {
            strands: or(strands, d.strands),
            height: or(height, d.height),
            sway: or(sway, d.sway),
            speed: or(speed, d.speed),
            glow: or(glow, d.glow),
            width: or(width, d.width),
        });
    }
}

/// Advance the animation by `dt_ms` and render it; returns the frame as RGBA,
/// `w * h * 4` bytes, or 0 before [`wisps_new`].
#[no_mangle]
pub extern "C" fn wisps_frame(dt_ms: u32) -> *const u8 {
    let Some(w) = WISPS.get() else { return core::ptr::null() };
    let (screen, rgba) = (SCREEN.get(), RGBA.get());
    w.step(dt_ms);
    w.render(screen);
    // RGB565 back to 8 bits a channel, repeating the top bits into the bottom.
    for (c, out) in screen.iter().zip(rgba.chunks_exact_mut(4)) {
        let (r, g, b) = (c.r(), c.g(), c.b());
        out.copy_from_slice(&[r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2, 255]);
    }
    rgba.as_ptr()
}

/// Set the strip's pattern and tuning (`strip::Params`, the page's Strip tab).
/// `pattern`, `gradient` and `edge` index `Pattern::ALL`, hue/mix and
/// soft/solid; colours are `0xrrggbb`.
#[no_mangle]
pub extern "C" fn strip_params(
    pattern: i32, speed: i32, width: i32, primary: i32, secondary: i32,
    gradient: i32, balance: i32, sharpness: i32, edge: i32, background: i32,
) {
    let d = Params::default();
    *STRIP.get() = (
        Pattern::ALL.get(or(pattern, 0usize)).copied().unwrap_or(Pattern::Sweep),
        Params {
            speed: or(speed, d.speed),
            width: or(width, d.width),
            primary: color(primary).unwrap_or(d.primary),
            secondary: color(secondary),
            gradient: if gradient == 1 { Gradient::Mix } else { Gradient::Hue },
            balance: or(balance, d.balance),
            sharpness: or(sharpness, d.sharpness),
            edge: if edge == 1 { Edge::Solid } else { Edge::Soft },
            background: color(background).unwrap_or(d.background),
        },
    );
}

/// Render `leds` LEDs at time `ms`; returns them as RGB, `leds * 3` bytes.
#[no_mangle]
pub extern "C" fn strip_frame(leds: u32, ms: f64) -> *const u8 {
    let (pattern, params) = *STRIP.get();
    let px = &mut LEDS.get()[..(leds as usize).min(MAX_LEDS)];
    strip::render(pattern, &params, ms as u64, px);
    px.as_ptr().cast()
}
