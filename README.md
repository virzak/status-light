# status-light

Internet status indicator for the GL.iNet Flint 4 (GL-BE14000) router. A monitor
on the router decides the state and drives a small display board plugged into a
USB port, and the router's own built-in LCD. Two USB boards are supported, both
ESP32-S3:

- `firmware/zero-s3` - a Waveshare ESP32-S3-Zero: its WS2812 LED and an optional
  addressable LED strip (Rust). `firmware/zero-ws2812` is the older MicroPython
  firmware for the same board, without strip support.
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
- `router/flame-screen/` - the blue flame on the router's built-in LCD while GL's
  screen UI sleeps (see below).
- `firmware/zero-ws2812/main.py` - MicroPython for the single-LED board. The WS2812
  is on GPIO21 and takes RGB order, not the usual GRB. Includes `push.sh` and
  `board-push.sh` to update it through the router (see below).
- `firmware/zero-s3/` - Rust (esp-hal) for the ESP32-S3-Zero and its LED strip;
  build, wiring and flash notes live in that directory.
- `firmware/tdisplay-s3/` - Rust (esp-hal) for the LCD board. Flashed with espflash;
  build and flash notes live in that directory.
- `firmware/status-protocol/` - the `PROTOCOL.md` parser shared by the Rust boards.
- `firmware/router-flash.sh` - flashes a board through the router by its USB
  serial; used by each firmware's `push-router.sh`.
- `firmware/flame/` - the blue-flame effects as a shared `no_std` crate, used by
  the LCD firmwares and the PC preview so all run identical code: `wisps`
  (glowing ribbons, what the LCDs show) and a heat-field flame (preview only).
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
it. GL's screen UI keeps running; when it sleeps (its screen timeout) the flame
fills the panel, and a touch wakes GL's UI instantly. It reads the state netled
writes to `/tmp/netled.state`, and blinks red if that file goes stale.

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

## Settings

Settings live on the router in `/etc/status-light.json`, described by
`settings.schema.json`: the LCD's brightness and flame (`wisps`), and per-board
brightness and flame keyed by each board's USB serial number. Every key is optional. With
`"$schema"` set (as in `router/status-light.json`), VS Code validates the file and
documents each key on hover.

```json
{
  "$schema": "https://raw.githubusercontent.com/virzak/status-light/master/settings.schema.json",
  "lcd": { "brightness": 60, "wisps": { "strands": 56 } },
  "boards": {
    "A0:F2:62:E1:35:58": { "name": "tdisplay", "brightness": 40, "wisps": { "sway": 70 } }
  }
}
```

flame-screen re-reads the file when it changes. netled sends each board its
values as `S` lines (see `PROTOCOL.md`) when the board connects and whenever the
file changes; the T-Display applies brightness and flame tuning live, and the
WS2812 board ignores them for now. Install the default file once and keep it
across firmware upgrades:

```
scp -O router/status-light.json "$ROUTER:/tmp/" && ssh "$ROUTER" 'sudo sh -c "
  [ -f /etc/status-light.json ] || cp /tmp/status-light.json /etc/status-light.json
  grep -qx /etc/status-light.json /etc/sysupgrade.conf || echo /etc/status-light.json >> /etc/sysupgrade.conf"'
```

### Web UI

`luci/` adds Services > Status Light to the router's LuCI (port 8080 on the
Flint 4): a form for the same file, with boards currently plugged in offered by
serial number. Copy the files (only files, so existing directories keep their
owner and permissions) onto the router and reload rpcd:

```
(cd luci && tar cf - $(find . -type f)) | ssh "$ROUTER" 'sudo sh -c "tar xof - -C /
  rm -f /tmp/luci-indexcache*; rm -rf /tmp/luci-modulecache
  /etc/init.d/rpcd reload"'
```

The page can only read and write `/etc/status-light.json` (plus read-only USB
device info), per `usr/share/rpcd/acl.d/luci-app-status-light.json`.
