# T-Display-S3 firmware (Rust)

Internet-status display for the LilyGO T-Display-S3. Reads the one-letter status
protocol in [`../../PROTOCOL.md`](../../PROTOCOL.md) from the router over USB
serial and shows it on the 320x170 ST7789 LCD:

- **Online** - an animated blue flame.
- **Reconnecting / Offline / boot / watchdog** - a static colour screen with a label.

Built on the `lilygo-t-display-s3` board crate and its `esp-lcd-i8080` DMA driver
(published from https://github.com/zompinc/esp-lcd-i8080). The router side
(`router/netled`) is shared with the WS2812 board and does not change: the display
enumerates under USB vendor `303a` as a `ttyACM`, which netled already finds.

## Build and flash

One-time toolchain setup (Xtensa ESP32-S3 target):

```
cargo install espup espflash
espup install
# then source the export file espup prints, e.g. on a POSIX shell:
. $HOME/export-esp.sh
```

Build and flash with the board on USB (the cargo runner calls espflash):

```
cargo run --release
```

`cargo run` without `--release` also works but is larger and slower.

## Deploy

Once flashed, plug the board into the router's USB port. netled drives it with no
extra step; unlike the MicroPython board there is no serial push script, because
this is a compiled binary flashed with espflash.

## Status

Builds clean (`cargo build --release`) against esp-hal 1.2.2 on the `esp`
toolchain, flashed to a board, and the blue flame renders when fed `B`.

Note when testing from a PC: opening/closing the USB serial port toggles the
reset line and drops the ESP32-S3 into ROM download mode (blank screen), so a
persistent feed is best done from the router, where netled drives it like the
WS2812 board with no resets.

The flame is a Doom-style fire on a heat grid with a blue palette (black, navy,
blue, cyan, white tip), drawn into the board's `FrameBuffer` and flushed over DMA.

## Tuning the flame

The flame lives in the shared `firmware/flame` crate, which this firmware and the
PC preview both use, so tuning on the PC changes exactly what ships. From
`firmware/flame-preview/`:

```
cargo run --release
```

A scaled window opens. Adjust with the keys it prints (cooling, drift, flicker,
and the blue/green/white palette thresholds); every change prints the current
`FlameParams` as a Rust literal. When it looks right, paste that into
`FlameParams::default()` in `firmware/flame/src/lib.rs`, then rebuild and reflash
this firmware.
