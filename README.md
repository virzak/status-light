# status-light

Internet status LED for the GL.iNet Flint 4 (GL-BE14000) router, using a
Waveshare ESP32-S3-Zero plugged into the router's spare USB port.

| Light         | Meaning                                                   |
|---------------|-----------------------------------------------------------|
| Dim white     | Board booted, waiting for the router                      |
| Breathing blue| Online                                                    |
| Amber         | 1-2 failed checks (blip or PPPoE reconnecting)            |
| Red           | 3+ failed checks (offline 15 s or more)                   |
| Blinking red  | No update from the router for 60 s (router or service down) |

## Layout

- `firmware/main.py` - MicroPython program on the board. Reads one command per
  line on USB serial: `B` blue, `R` red, `A` amber, `G` green, `W` white, `O` off.
  The WS2812 LED is on GPIO21 and takes RGB order, not the usual GRB.
- `router/netled` - monitor loop, installed as `/usr/bin/netled`. Finds the board
  by USB vendor ID `303a`, pings 1.1.1.1 / 8.8.8.8 / 9.9.9.9 every 5 s and writes
  the state.
- `router/netled.init` - procd service, installed as `/etc/init.d/netled`.
- `scripts/` - push a new `main.py` to the board through the router (see below).

## Flashing the board

```
uvx --from esptool esptool --port COMx erase-flash
uvx --from esptool esptool --port COMx --baud 460800 write-flash 0 ESP32_GENERIC_S3-<date>-v1.29.0.bin
uvx mpremote connect COMx fs cp firmware/main.py :main.py + reset
```

Firmware: https://micropython.org/download/ESP32_GENERIC_S3/ . The COM port
number changes after flashing.

To update `main.py` later without unplugging the board from the router:

```
ROUTER=user@router scripts/push-firmware.sh
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
