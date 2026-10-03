//! PC preview for the LED-strip patterns: runs the exact `strip` crate the
//! ESP32-S3-Zero uses.
//!
//! With no arguments, opens a window showing the strip as a row of LEDs.
//! Keys 1-7 pick the pattern, Up/Down change the speed, Left/Right the width;
//! every change prints the current settings.
//!
//! `--sheet <dir>` instead writes one space-time PNG per pattern (LEDs across,
//! 10 s of frames downwards), which shows a pattern's motion in a single image.
//! `--leds N` sets the strip length (default 60); `--primary`, `--secondary`
//! and `--background`, each `#rrggbb`, the colours.

use std::time::Instant;

use minifb::{Key, KeyRepeat, Window, WindowOptions};
use strip::{Params, Pattern, Rgb, parse_color, render};

const LED_PX: usize = 16;
const FRAME_MS: u64 = 33;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1));
    let leds: usize = arg("--leds").and_then(|v| v.parse().ok()).unwrap_or(60);
    let mut params = Params::default();
    let color = |name: &str| arg(name).map(|v| parse_color(v).unwrap_or_else(|| panic!("{name}: expected #rrggbb")));
    if let Some(c) = color("--primary") {
        params.primary = c;
    }
    params.secondary = color("--secondary");
    if let Some(c) = color("--background") {
        params.background = c;
    }

    if let Some(dir) = arg("--sheet") {
        sheets(dir, leds, &params);
    } else {
        window(leds, params);
    }
}

fn sheets(dir: &str, leds: usize, params: &Params) {
    std::fs::create_dir_all(dir).expect("output folder");
    let frames = (10_000 / FRAME_MS) as usize;
    let (w, row_h) = (leds * 8, 2);
    for pattern in Pattern::ALL {
        let mut img = vec![0u8; w * frames * row_h * 3];
        let mut px = vec![[0u8; 3]; leds];
        for f in 0..frames {
            render(pattern, params, f as u64 * FRAME_MS, &mut px);
            for y in f * row_h..(f + 1) * row_h {
                for x in 0..w {
                    let o = (y * w + x) * 3;
                    img[o..o + 3].copy_from_slice(&px[x / 8]);
                }
            }
        }
        let path = format!("{dir}/{}.png", pattern.name());
        let file = std::io::BufWriter::new(std::fs::File::create(&path).expect("png file"));
        let mut enc = png::Encoder::new(file, w as u32, (frames * row_h) as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.write_header().and_then(|mut wr| wr.write_image_data(&img)).expect("png");
        println!("{path}");
    }
}

fn window(leds: usize, mut params: Params) {
    let (w, h) = (leds * LED_PX, LED_PX * 3);
    let mut win = Window::new("strip preview", w, h, WindowOptions::default()).expect("window");
    win.set_target_fps(30);
    let mut buf = vec![0u32; w * h];
    let mut px = vec![[0u8; 3]; leds];
    let mut pattern = Pattern::Sweep;
    let start = Instant::now();
    let report = |p: Pattern, q: &Params| println!("pattern {} speed {} width {}", p.name(), q.speed, q.width);
    report(pattern, &params);

    while win.is_open() && !win.is_key_down(Key::Escape) {
        for key in win.get_keys_pressed(KeyRepeat::Yes) {
            let digit = [Key::Key1, Key::Key2, Key::Key3, Key::Key4, Key::Key5, Key::Key6, Key::Key7]
                .iter()
                .position(|k| *k == key);
            match (digit, key) {
                (Some(d), _) => pattern = Pattern::ALL[d],
                (_, Key::Up) => params.speed = (params.speed + 5).min(100),
                (_, Key::Down) => params.speed = params.speed.saturating_sub(5).max(1),
                (_, Key::Right) => params.width = (params.width + 1).min(50),
                (_, Key::Left) => params.width = params.width.saturating_sub(1).max(1),
                _ => continue,
            }
            report(pattern, &params);
        }
        render(pattern, &params, start.elapsed().as_millis() as u64, &mut px);
        draw(&px, &mut buf, w);
        win.update_with_buffer(&buf, w, h).expect("draw");
    }
}

/// Each LED as a round dot on a dark strip.
fn draw(px: &[Rgb], buf: &mut [u32], w: usize) {
    buf.fill(0x0010_1010);
    let r = (LED_PX / 2 - 2) as i32;
    for (i, c) in px.iter().enumerate() {
        let (cx, cy) = ((i * LED_PX + LED_PX / 2) as i32, (LED_PX * 3 / 2) as i32);
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    let (x, y) = ((cx + dx) as usize, (cy + dy) as usize);
                    buf[y * w + x] = (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
                }
            }
        }
    }
}
