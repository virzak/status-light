#!/bin/sh
# Run from the PC: pushes this board's main.py to the ESP32-S3-Zero while it
# stays plugged into the router. Prints "WROTE <bytes>"; compare with local size.
# The T-Display-S3 board uses espflash instead; see firmware/tdisplay-s3.
set -e
cd "$(dirname "$0")"
ROUTER=${1:-${ROUTER:?usage: ROUTER=user@router firmware/zero-ws2812/push.sh, or pass user@router as the argument}}
base64 -w0 main.py > /tmp/main.b64
scp -q -O /tmp/main.b64 board-push.sh "$ROUTER:/tmp/"
ssh "$ROUTER" 'sudo sh /tmp/board-push.sh'
echo "local: $(wc -c < main.py) bytes"
