#!/bin/sh
# Runs on the router. Writes /tmp/main.b64 (base64 of main.py) to the board
# through the MicroPython raw REPL on the USB serial port, then soft-resets it.
#
# The router's tty has no stty, so its line discipline echoes everything the
# board prints straight back to it. The raw REPL is the only mode where that
# is harmless: it never echoes input and only prints after Ctrl-D, so we clear
# its buffer with Ctrl-C right before every Ctrl-D and never leave raw mode.
set -e
B64=$(cat /tmp/main.b64)
TTY=${1:-/dev/ttyACM0}

/etc/init.d/netled stop 2>/dev/null || true
sleep 1
exec 3<>"$TTY"
cat <&3 > /tmp/board-push.log &
RD=$!
printf '\003\001' >&3; sleep 1      # interrupt main.py, enter raw REPL
printf '\003' >&3; sleep 1          # drop echoed junk from the raw buffer
printf 'import ubinascii\nf=open("main.py","wb")\nf.write(ubinascii.a2b_base64(b"%s"))\nf.close()\nprint("WROTE",len(open("main.py").read()))\n\004' "$B64" >&3
sleep 3
printf '\003' >&3; sleep 1          # drop the echoed "OK WROTE n"
printf '\004' >&3; sleep 4          # empty buffer + Ctrl-D = soft reset, runs main.py
kill $RD 2>/dev/null || true
exec 3>&-
sleep 1
/etc/init.d/netled start
tr -d '\r' < /tmp/board-push.log | grep -a -o 'WROTE [0-9]*' | tail -1
