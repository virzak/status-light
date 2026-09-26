#!/bin/sh
# Run from the PC: pushes firmware/main.py to the board while it stays plugged
# into the router. Prints "WROTE <bytes>" on success; compare with the local size.
set -e
cd "$(dirname "$0")/.."
ROUTER=${1:-${ROUTER:?usage: ROUTER=user@router scripts/push-firmware.sh, or pass user@router as the argument}}
base64 -w0 firmware/main.py > /tmp/main.b64
scp -q -O /tmp/main.b64 scripts/board-push.sh "$ROUTER:/tmp/"
ssh "$ROUTER" 'sudo sh /tmp/board-push.sh'
echo "local: $(wc -c < firmware/main.py) bytes"
