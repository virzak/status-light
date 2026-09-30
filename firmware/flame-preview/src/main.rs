//! PC preview and tuner for the shared blue-flame effect.
//!
//! Runs the exact `flame` crate the board firmware uses, in a scaled window, so
//! the flame can be tuned live. Adjust with the keys below; every change prints
//! the current `FlameParams` as a Rust literal. When it looks right, paste that
//! into `FlameParams::default()` in `firmware/flame/src/lib.rs` and reflash.

use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
use flame::{Flame, FlameParams};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

// Match the board's display so what you see is what ships.
const W: usize = 320;
const H: usize = 170;

/// RGB565 -> 0x00RRGGBB for minifb, expanding each channel to 8 bits.
fn to_argb(c: Rgb565) -> u32 {
    let r = c.r() as u32; // 0..=31
    let g = c.g() as u32; // 0..=63
    let b = c.b() as u32; // 0..=31
    let r8 = (r << 3) | (r >> 2);
    let g8 = (g << 2) | (g >> 4);
    let b8 = (b << 3) | (b >> 2);
    (r8 << 16) | (g8 << 8) | b8
}

fn print_params(p: &FlameParams) {
    println!(
        "FlameParams {{ cooling: {}, drift: {}, seed_min: {}, seed_max: {}, \
flicker: {}, blue_full: {}, green_start: {}, white_start: {} }}",
        p.cooling, p.drift, p.seed_min, p.seed_max, p.flicker, p.blue_full, p.green_start,
        p.white_start
    );
}

fn main() {
    println!("flame preview - keys:");
    println!("  cooling   Q/A      drift      W/S");
    println!("  flicker   E/D      blue_full  R/F");
    println!("  grn_start T/G      wht_start  Y/H");
    println!("  space reset   P print params   Esc quit");

    let mut params = FlameParams::default();
    let mut fl = Flame::new(W, H, params, 0xC0FF_EE11);
    let mut heat = vec![0u8; W * H];
    let mut px = vec![Rgb565::BLACK; W * H];
    let mut fb = vec![0u32; W * H];

    let mut win = Window::new(
        "flame preview (Esc to quit, P to print params)",
        W,
        H,
        WindowOptions {
            scale: Scale::X4,
            ..WindowOptions::default()
        },
    )
    .expect("open window");
    win.set_target_fps(30);
    print_params(&params);

    while win.is_open() && !win.is_key_down(Key::Escape) {
        let mut changed = false;
        for k in win.get_keys_pressed(KeyRepeat::Yes) {
            match k {
                Key::Q => params.cooling = params.cooling.saturating_add(1),
                Key::A => params.cooling = params.cooling.saturating_sub(1),
                Key::W => params.drift = (params.drift + 1).min(3),
                Key::S => params.drift = params.drift.saturating_sub(1),
                Key::E => params.flicker = params.flicker.saturating_add(1),
                Key::D => params.flicker = params.flicker.saturating_sub(1).max(1),
                Key::R => params.blue_full = params.blue_full.saturating_add(5),
                Key::F => params.blue_full = params.blue_full.saturating_sub(5).max(1),
                Key::T => params.green_start = params.green_start.saturating_add(5),
                Key::G => params.green_start = params.green_start.saturating_sub(5),
                Key::Y => params.white_start = params.white_start.saturating_add(5),
                Key::H => params.white_start = params.white_start.saturating_sub(5),
                Key::Space => fl.reset(&mut heat),
                Key::P => print_params(&params),
                _ => continue,
            }
            changed = true;
        }
        if changed && *fl.params() != params {
            fl.set_params(params);
            print_params(&params);
        }

        fl.step(&mut heat);
        fl.render(&heat, &mut px);
        for (o, &c) in fb.iter_mut().zip(px.iter()) {
            *o = to_argb(c);
        }
        win.update_with_buffer(&fb, W, H).expect("update window");
    }
}
