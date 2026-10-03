#!/bin/sh
# Run from the PC: build the T-Display firmware and flash it to the board while it
# stays plugged into the router. This is the compiled-firmware equivalent of the
# WS2812 board's push.sh, using espflash on the router (see ../router-flash.sh).
set -e
cd "$(dirname "$0")"
ROUTER=${1:-${ROUTER:?usage: ROUTER=user@router firmware/tdisplay-s3/push-router.sh, or pass user@router as the argument}}
# With more than one board on the router, set BOARD to this board's USB serial.
BOARD=${2:-$BOARD}
cargo build --release
ELF=target/xtensa-esp32s3-none-elf/release/status-tdisplay-s3
scp -q -O "$ELF" ../router-flash.sh "$ROUTER:/tmp/"
ssh "$ROUTER" "sudo sh /tmp/router-flash.sh /tmp/status-tdisplay-s3 $BOARD"
