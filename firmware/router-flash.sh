#!/bin/sh
# Runs on the router. Flashes an ESP32-S3 firmware image to the board over USB so
# the T-Display can be updated in place, without moving it to a PC. netled holds
# the serial port, so stop it around the flash and restart it after. Needs the
# espflash binary at /usr/bin/espflash (aarch64-musl build).
IMG=${1:?usage: router-flash.sh <firmware-elf-or-bin>}

find_tty() {
  for d in /sys/bus/usb/devices/*; do
    [ "$(cat "$d/idVendor" 2>/dev/null)" = "303a" ] || continue
    for t in "$d"/*/tty/ttyACM*; do
      [ -e "$t" ] && { echo "/dev/${t##*/}"; return 0; }
    done
  done
  return 1
}

/etc/init.d/netled stop 2>/dev/null
sleep 1
TTY=$(find_tty)
[ -n "$TTY" ] || { echo "board not found on USB"; /etc/init.d/netled start; exit 1; }
echo "flashing $IMG -> $TTY"
espflash flash --chip esp32s3 --port "$TTY" "$IMG"
rc=$?
sleep 2
/etc/init.d/netled start
echo "flash rc=$rc"
exit $rc
