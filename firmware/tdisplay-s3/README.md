# T-Display-S3 firmware (Rust)

Internet-status display for the LilyGO T-Display-S3. Reads the one-letter status
protocol in [`../../PROTOCOL.md`](../../PROTOCOL.md) from the router over USB
serial and shows it on the 320x170 ST7789 LCD:

- **Online** - an animated blue flame of glowing, swaying ribbons.
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

The flame is `flame::wisps`: thin ribbons whose edges glow and add up where they
cross, through a blue palette (black, deep blue, blue, cyan, white), drawn into
the board's `FrameBuffer` and flushed over DMA. With the default 48 ribbons a
frame takes about 31 ms to draw and 8 ms to flush, so it runs at about 25 fps;
fewer ribbons (`S strands`) run faster.

## Tuning the flame

The flame lives in the shared `firmware/flame` crate, which this firmware and the
PC preview both use, so tuning on the PC changes exactly what ships. From
`firmware/flame-preview/`:

```
cargo run --release
```

A scaled window opens on the wisps. Adjust with the keys it prints (ribbons,
height, sway, speed, glow, width); every change prints the current `WispParams`
as a Rust literal. When it looks right, paste that into `WispParams::default()`
in `firmware/flame/src/wisps.rs`, then rebuild and reflash this firmware. The
same values can also be set per board in the router's settings, with no reflash.
Tab switches to the heat-field flame, `--size 320x240` previews the router's
LCD, and `--snap out.png` writes a sheet of frames to a PNG without a window.

## Flashing in place via the router

The board can be reflashed without unplugging it, the same convenience the WS2812
board has. You still build on a PC (the Xtensa build cannot run on the router),
but the image is pushed to the router, which flashes the board over USB with its
own `espflash`.

One-time: build `espflash` for the router (aarch64 musl) and install it. The
19 MB binary is not committed; rebuild it with Docker:

```
docker run --rm -v "$PWD/out:/out" messense/rust-musl-cross:aarch64-musl \
  cargo install espflash --version 4.6.0 --root /out --locked --no-track
scp -O out/bin/espflash "$ROUTER:/tmp/" && \
  ssh "$ROUTER" 'sudo install -m 755 /tmp/espflash /usr/bin/espflash'
```

`espflash` builds static and udev-free here because 4.6.0 sets the `serialport`
dependency to `default-features = false`.

Then, to build and flash in place:

```
ROUTER=user@router firmware/tdisplay-s3/push-router.sh
```

It builds release, copies the ELF and `router-flash.sh` to the router, and runs
the router-side script, which stops `netled`, flashes over the board's `ttyACM`,
resets it, and restarts `netled`.
