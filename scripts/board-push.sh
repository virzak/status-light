#!/bin/sh
# Runs on the router. Writes /tmp/main.b64 (base64 of main.py) to the board
# through the MicroPython raw REPL on the USB serial port, then hard-resets it.
#
# The router's tty has no stty, so its line discipline echoes everything the
# board prints straight back to it. The raw REPL is the only mode where that
# is harmless: it never echoes input and only prints after Ctrl-D, so we clear
# its buffer with Ctrl-C right before every Ctrl-D and never leave raw mode.
# A soft reset from the raw REPL would not run main.py (MicroPython only runs
# it in friendly-REPL mode), hence machine.reset(): the board re-enumerates on
# USB and boots normally, and netled re-finds the new tty on its own.
B64=$(cat /tmp/main.b64)
TTY=${1:-/dev/ttyACM0}
LOG=/tmp/board-push.log

/etc/init.d/netled stop 2>/dev/null
sleep 1
exec 3<>"$TTY"
# The board's Ctrl-D terminators read as EOF on a canonical tty, so keep
# reopening the reader until we are done.
( while [ -e "$TTY" ] && [ ! -e /tmp/board-push.done ]; do cat "$TTY"; done ) >> "$LOG" 2>/dev/null &
: > "$LOG"; rm -f /tmp/board-push.done
printf '\003\001' >&3; sleep 1      # interrupt main.py, enter raw REPL
printf '\003' >&3; sleep 1          # drop echoed junk from the raw buffer
printf 'import ubinascii\nf=open("main.py","wb")\nf.write(ubinascii.a2b_base64(b"%s"))\nf.close()\nprint("WROTE",len(open("main.py").read()))\n\004' "$B64" >&3
sleep 3
printf '\003' >&3; sleep 1          # drop the echoed "OK WROTE n"
printf 'import machine\nmachine.reset()\n\004' >&3 2>/dev/null
sleep 1
touch /tmp/board-push.done
exec 3>&- 2>/dev/null
sleep 5
/etc/init.d/netled start
tr -d '\r' < "$LOG" | grep -a -o 'WROTE [0-9]*' | tail -1
ls /dev/ttyACM* 2>/dev/null
