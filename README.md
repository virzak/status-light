# status-light

Internet status indicator for the GL.iNet Flint 4 (GL-BE14000) router. A monitor
on the router decides the state and drives a small display board plugged into a
USB port, and the router's own built-in LCD. Two USB boards are supported, both
ESP32-S3:

- `firmware/zero-ws2812` - a single WS2812 LED on a Waveshare ESP32-S3-Zero (MicroPython).
- `firmware/tdisplay-s3` - a 170x320 colour LCD on a LilyGO T-Display-S3 (Rust).

Both speak the same serial contract in `PROTOCOL.md`, so the router side is shared.
The table below is the WS2812 board's rendering.

| Light         | Meaning                                                   |
|---------------|-----------------------------------------------------------|
| Dim white     | Board booted, waiting for the router                      |
| Breathing blue| Online                                                    |
| Amber         | 1-2 failed checks (blip or PPPoE reconnecting)            |
| Red           | 3+ failed checks (offline 15 s or more)                   |
| Blinking red  | No update from the router for 60 s (router or service down) |

## Layout

- `PROTOCOL.md` - the serial contract every display board implements.
- `router/netled` - monitor loop, installed as `/usr/bin/netled`. Finds the board
  by USB vendor ID `303a`, checks connectivity every 5 s and writes the state to
  the board and to `/tmp/netled.state`.
- `router/netled.init` - procd service, installed as `/etc/init.d/netled`.
- `router/flame-screen/` - the blue flame as the idle screen of the router's
  built-in LCD, with GL's screen UI on touch (see below).
- `firmware/zero-ws2812/main.py` - MicroPython for the single-LED board. The WS2812
  is on GPIO21 and takes RGB order, not the usual GRB. Includes `push.sh` and
  `board-push.sh` to update it through the router (see below).
- `firmware/tdisplay-s3/` - Rust (esp-hal) for the LCD board. Flashed with espflash;
  build and flash notes live in that directory.
- `firmware/flame/` - the blue-flame effect as a shared `no_std` crate, used by
  the LCD firmware and the PC preview so both run identical code.
- `firmware/flame-preview/` - a PC window that runs the flame for live tuning.

## Flashing the WS2812 board (MicroPython)

```
uvx --from esptool esptool --port COMx erase-flash
uvx --from esptool esptool --port COMx --baud 460800 write-flash 0 ESP32_GENERIC_S3-<date>-v1.29.0.bin
uvx mpremote connect COMx fs cp firmware/zero-ws2812/main.py :main.py + reset
```

Firmware: https://micropython.org/download/ESP32_GENERIC_S3/ . The COM port
number changes after flashing.

To update `main.py` later without unplugging the board from the router:

```
ROUTER=user@router firmware/zero-ws2812/push.sh
```

It stops `netled`, writes the file through the MicroPython raw REPL on the
router's serial port, soft-resets the board and starts `netled` again. It
prints `WROTE <bytes>`, which should match the local file size.

## Installing on the router

Set `ROUTER` to the SSH target of the router, e.g. `ROUTER=admin@192.168.8.1`.

```
tar cf - -C router netled netled.init | ssh "$ROUTER" 'cd /tmp && tar xf - && sudo sh -c "
  tr -d \"\\r\" < netled > /usr/bin/netled && chmod 755 /usr/bin/netled
  tr -d \"\\r\" < netled.init > /etc/init.d/netled && chmod 755 /etc/init.d/netled
  /etc/init.d/netled enable && /etc/init.d/netled restart"'
```

Add `/usr/bin/netled` and `/etc/init.d/netled` to `/etc/sysupgrade.conf` so they
survive firmware upgrades.

## The router's built-in LCD

The Flint 4's screen is a standard Linux framebuffer (`/dev/fb0`, 240x320 RGB565,
GL's `st7789p3` driver), so `flame-screen` draws the shared flame straight into
it. The flame is the idle screen; touching the panel starts GL's screen UI, and
the flame returns after 60 s without touches. It reads the state netled writes
to `/tmp/netled.state`, and blinks red if that file goes stale.

Build the static aarch64 binary in Docker, then install it and the service:

```
docker run --rm -v "$PWD:/src" -w /src/router/flame-screen \
  messense/rust-musl-cross:aarch64-musl cargo build --release
scp -O router/flame-screen/target/aarch64-unknown-linux-musl/release/flame-screen \
  router/flame-screen/flame-screen.init "$ROUTER:/tmp/"
ssh "$ROUTER" 'sudo sh -c "cp /tmp/flame-screen /usr/bin/ && chmod 755 /usr/bin/flame-screen
  tr -d \"\\r\" < /tmp/flame-screen.init > /etc/init.d/flame-screen
  chmod 755 /etc/init.d/flame-screen
  /etc/init.d/flame-screen enable && /etc/init.d/flame-screen start"'
```

To go back to GL's stock screen:

```
/etc/init.d/flame-screen stop && /etc/init.d/flame-screen disable
```

These files are deliberately not in `/etc/sysupgrade.conf`, so a firmware upgrade
also restores the stock screen.
