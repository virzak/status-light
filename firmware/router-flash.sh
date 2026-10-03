#!/bin/sh
# Runs on the router. Flashes an ESP32-S3 firmware image to a board over USB so
# boards can be updated in place, without moving them to a PC. netled holds the
# serial ports, so stop it around the flash and restart it after. Needs the
# espflash binary at /usr/bin/espflash (aarch64-musl build).
#
#   router-flash.sh <firmware-elf-or-bin> [usb-serial]
#
# With several Espressif boards plugged in, pass the board's USB serial (as
# netled logs it); without one, the script only flashes when there is exactly
# one board, and otherwise lists the serials and stops.
IMG=${1:?usage: router-flash.sh <firmware-elf-or-bin> [usb-serial]}
SERIAL=$2
[ -f "$IMG" ] || { echo "no such image: $IMG"; exit 1; }

# "<tty> <usb serial>" for every Espressif (vendor 303a) board, as netled does.
boards() {
  for d in /sys/bus/usb/devices/*; do
    [ "$(cat "$d/idVendor" 2>/dev/null)" = "303a" ] || continue
    for t in "$d"/*/tty/ttyACM*; do
      [ -e "$t" ] && echo "/dev/${t##*/} $(cat "$d/serial" 2>/dev/null)"
    done
  done
}

if [ -n "$SERIAL" ]; then
  TTY=$(boards | awk -v s="$SERIAL" '$2 == s { print $1; exit }')
  [ -n "$TTY" ] || { echo "no board with serial $SERIAL; found:"; boards; exit 1; }
else
  [ "$(boards | wc -l)" -eq 1 ] || { echo "pass the USB serial of the board to flash; found:"; boards; exit 1; }
  TTY=$(boards | cut -d' ' -f1)
fi

/etc/init.d/netled stop 2>/dev/null
sleep 1
echo "flashing $IMG -> $TTY"
espflash flash --chip esp32s3 --port "$TTY" "$IMG"
rc=$?
sleep 2
/etc/init.d/netled start
echo "flash rc=$rc"
exit $rc
