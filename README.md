# status-light

Internet status indicator for the GL.iNet Flint 4 (GL-BE14000) router. A monitor
on the router decides the state and drives a small display board plugged into a
USB port. Two display boards are supported, both ESP32-S3:

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
  by USB vendor ID `303a`, checks connectivity every 5 s and writes the state.
- `router/netled.init` - procd service, installed as `/etc/init.d/netled`.
- `firmware/zero-ws2812/main.py` - MicroPython for the single-LED board. The WS2812
  is on GPIO21 and takes RGB order, not the usual GRB. Includes `push.sh` and
  `board-push.sh` to update it through the router (see below).
- `firmware/tdisplay-s3/` - Rust (esp-hal) for the LCD board. Flashed with espflash;
  build and flash notes live in that directory.

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
