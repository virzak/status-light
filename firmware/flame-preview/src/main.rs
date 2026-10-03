//! PC preview and tuner for the shared blue-flame effects.
//!
//! Runs the exact `flame` crate the board firmware uses, in a scaled window, so
//! the effects can be tuned live. Tab switches between the wisps (what the LCDs
//! show) and the heat-field flame. Adjust with the keys below; every change
//! prints the current parameters as a Rust literal. When it looks right, paste
//! that into the `Default` impl in `firmware/flame/src` and reflash.
//!
//! `--size 320x240` previews the router's LCD instead of the T-Display.
//! `--snap out.png` renders a 2x2 sheet of wisp frames (at 3, 4, 5 and 6 s)
//! to a PNG without opening a window, for side-by-side comparison.

use std::time::Instant;

use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
use flame::wisps::{WispParams, Wisps};
use flame::{Flame, FlameParams};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

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
        p.cooling,
        p.drift,
        p.seed_min,
        p.seed_max,
        p.flicker,
        p.blue_full,
        p.green_start,
        p.white_start
    );
}

fn print_wisp_params(p: &WispParams) {
    println!(
        "WispParams {{ strands: {}, height: {}, sway: {}, speed: {}, glow: {}, width: {} }}",
        p.strands, p.height, p.sway, p.speed, p.glow, p.width
    );
}

fn bump(v: &mut u8, up: bool, step: u8, lo: u8, hi: u8) {
    *v = if up {
        v.saturating_add(step).min(hi)
    } else {
        v.saturating_sub(step).max(lo)
    };
}

/// Render wisp frames at 3, 4, 5 and 6 s into a 2x2 PNG sheet.
fn snap(path: &str, w: usize, h: usize) {
    let mut wisps = Wisps::new(w, h, WispParams::default(), 0xC0FF_EE11);
    let mut px = vec![Rgb565::BLACK; w * h];
    let mut sheet = vec![0u8; 4 * w * h * 3];
    let started = Instant::now();
    let mut frames = 0u32;
    let mut ms = 0;
    for (i, at) in [3000, 4000, 5000, 6000].into_iter().enumerate() {
        while ms < at {
            wisps.step(33);
            ms += 33;
        }
        wisps.render(&mut px);
        frames += 1;
        let (ox, oy) = ((i % 2) * w, (i / 2) * h);
        for y in 0..h {
            for x in 0..w {
                let c = to_argb(px[y * w + x]);
                let o = ((oy + y) * 2 * w + ox + x) * 3;
                sheet[o] = (c >> 16) as u8;
                sheet[o + 1] = (c >> 8) as u8;
                sheet[o + 2] = c as u8;
            }
        }
    }
    let per = started.elapsed() / frames;
    println!("{frames} frames, {per:?} per render (including steps)");
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 2 * w as u32, 2 * h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut wr| wr.write_image_data(&sheet))
        .expect("write png");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
    };
    // Default to the board's display so what you see is what ships.
    let (w, h) = arg("--size")
        .and_then(|s| s.split_once('x'))
        .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
        .unwrap_or((320usize, 170usize));
    if let Some(path) = arg("--snap") {
        snap(path, w, h);
        return;
    }

    println!("flame preview - Tab switches wisps / heat flame");
    println!("wisps keys:");
    println!("  strands Q/A   height W/S   sway  E/D");
    println!("  speed   R/F   glow   T/G   width Y/H");
    println!("heat flame keys:");
    println!("  cooling   Q/A      drift      W/S");
    println!("  flicker   E/D      blue_full  R/F");
    println!("  grn_start T/G      wht_start  Y/H");
    println!("  space reset   P print params   Esc quit");

    let mut wisp_params = WispParams::default();
    let mut wisps = Wisps::new(w, h, wisp_params, 0xC0FF_EE11);
    let mut params = FlameParams::default();
    let mut fl = Flame::new(w, h, params, 0xC0FF_EE11);
    let mut heat = vec![0u8; w * h];
    let mut px = vec![Rgb565::BLACK; w * h];
    let mut fb = vec![0u32; w * h];
    let mut show_wisps = true;

    let mut win = Window::new(
        "flame preview (Tab switches effect, P prints params, Esc quits)",
        w,
        h,
        WindowOptions {
            scale: Scale::X4,
            ..WindowOptions::default()
        },
    )
    .expect("open window");
    win.set_target_fps(30);
    print_wisp_params(&wisp_params);
    let mut last = Instant::now();

    while win.is_open() && !win.is_key_down(Key::Escape) {
        let mut changed = false;
        for k in win.get_keys_pressed(KeyRepeat::Yes) {
            match k {
                Key::Tab => show_wisps = !show_wisps,
                Key::Space if show_wisps => wisps.reset(),
                Key::Space => fl.reset(&mut heat),
                Key::P if show_wisps => print_wisp_params(&wisp_params),
                Key::P => print_params(&params),
                _ if show_wisps => {
                    let p = &mut wisp_params;
                    match k {
                        Key::Q => bump(&mut p.strands, true, 2, 1, 64),
                        Key::A => bump(&mut p.strands, false, 2, 1, 64),
                        Key::W => bump(&mut p.height, true, 5, 10, 100),
                        Key::S => bump(&mut p.height, false, 5, 10, 100),
                        Key::E => bump(&mut p.sway, true, 5, 0, 100),
                        Key::D => bump(&mut p.sway, false, 5, 0, 100),
                        Key::R => bump(&mut p.speed, true, 5, 0, 100),
                        Key::F => bump(&mut p.speed, false, 5, 0, 100),
                        Key::T => bump(&mut p.glow, true, 5, 0, 100),
                        Key::G => bump(&mut p.glow, false, 5, 0, 100),
                        Key::Y => bump(&mut p.width, true, 5, 0, 100),
                        Key::H => bump(&mut p.width, false, 5, 0, 100),
                        _ => continue,
                    }
                }
                _ => match k {
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
                    _ => continue,
                },
            }
            changed = true;
        }
        if changed && *fl.params() != params {
            fl.set_params(params);
            print_params(&params);
        }
        if changed && *wisps.params() != wisp_params {
            wisps.set_params(wisp_params);
            print_wisp_params(&wisp_params);
        }

        if show_wisps {
            let now = Instant::now();
            wisps.step((now - last).as_millis() as u32);
            last = now;
            wisps.render(&mut px);
        } else {
            fl.step(&mut heat);
            fl.render(&heat, &mut px);
        }
        for (o, &c) in fb.iter_mut().zip(px.iter()) {
            *o = to_argb(c);
        }
        win.update_with_buffer(&fb, w, h).expect("update window");
    }
}
