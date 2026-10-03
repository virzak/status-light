#!/bin/sh
# Run from the PC: build the firmware and flash it to the ESP32-S3-Zero while it
# stays plugged into the router, using espflash on the router (see
# ../router-flash.sh). The board must already run this firmware or be in its
# ROM bootloader; see README.md for a board's first flash.
set -e
cd "$(dirname "$0")"
ROUTER=${1:-${ROUTER:?usage: ROUTER=user@router firmware/zero-s3/push-router.sh, or pass user@router as the argument}}
# With more than one board on the router, set BOARD to this board's USB serial.
BOARD=${2:-$BOARD}
cargo build --release
ELF=target/xtensa-esp32s3-none-elf/release/status-zero-s3
scp -q -O "$ELF" ../router-flash.sh "$ROUTER:/tmp/"
ssh "$ROUTER" "sudo sh /tmp/router-flash.sh /tmp/status-zero-s3 $BOARD"
