# status-zero-s3

Rust (esp-hal) firmware for the Waveshare ESP32-S3-Zero: the internet status on
the board's own WS2812 and on an optional addressable LED strip, speaking the
protocol in `../../PROTOCOL.md`.

| State                 | Onboard LED     | Strip                          |
|-----------------------|-----------------|--------------------------------|
| Waiting for the router| dim white       | dim white                      |
| Online (`B`)          | breathing blue  | a blue pattern (sweep by default) |
| Reconnecting (`A`)    | amber           | amber                          |
| Offline (`R`)         | red, flashing 0.5 s on / 0.5 s off | same           |
| No commands for 60 s  | red, pulsing slowly | same                       |

## Wiring

- Onboard WS2812: GPIO21, RGB colour order.
- Strip: WS2812B-type, GRB order, data on GPIO2, ground to GND. The firmware
  drives up to 300 LEDs; `strip.leds` in the settings picks how many (default 60).

Power the board, and through its 5V pin the strip, from a powered USB hub or a
separate 5 V supply, not from a bus-powered hub on the router: the strip's
current there made the board drop off USB. With a separate supply, feed the
strip's 5V and GND from it and connect only GND and data to the board. The
firmware caps the strip at about an eighth of full power at 100% brightness.

## Strip settings

Per board in `/etc/status-light.json` (see `settings.schema.json`), or on the
Strip tab of the router's settings page: `leds`, the online `pattern` (sweep,
comet, converge, breathe, wave, heartbeat or twinkle, from `../strip`), its
`speed` and `width`, its colours (`#rrggbb` or `#rgb`: `primary` at the pattern's head or
centre, an optional `secondary` its gradient runs to, round the colour wheel
or as a straight mix per `gradient`, shaped by `balance` (how much of it is
primary) and `sharpness` (smooth to a hard edge), its `edge` (soft fades into
the background, solid is fully lit with a crisp boundary), and the `background`
outside the pattern, black (off) by default), and `identify`. A strip cannot report its length, so to find it, turn
`identify` on: the first LED lights green, every 10th red and the rest dim blue.
Count them, set `leds`, and turn `identify` off.

## Build

Needs the `esp` Rust toolchain (`espup`) with the Xtensa GCC on `PATH`:

```
cargo build --release
```

## Flash

From the PC, with the board on USB:

```
espflash flash --chip esp32s3 --port COMx target/xtensa-esp32s3-none-elf/release/status-zero-s3
```

Through the router, with the board left in place (`../router-flash.sh` stops
netled, flashes, and starts it again):

```
ROUTER=user@router BOARD=<usb serial> ./push-router.sh
```

`BOARD` is the board's USB serial as netled logs it (`netled: using ... (serial
...)`); it is needed when more than one board is plugged in.

### First flash of a board running other firmware

A board that ships with, or was given, other firmware (MicroPython, for example)
may show a different USB serial and not answer espflash. Put it in its ROM
bootloader: hold BOOT while plugging it in, or from MicroPython run `import
machine; machine.bootloader()`. Wait for it to re-enumerate as `USB JTAG/serial
debug unit` (this can take 20-30 s), then flash it as above with that serial.
Unplug and replug the board afterwards: entering the bootloader from firmware
can leave a flag that boots it back into the bootloader instead of the new
firmware.
