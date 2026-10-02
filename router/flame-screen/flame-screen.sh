#!/bin/sh
# Show the blue-flame internet status on the router's built-in LCD instead of
# GL's screen UI. Stops gl_screen while running and always restarts it on exit
# (Ctrl-C, kill, or the binary dying), so the stock screen comes back.
BIN=${BIN:-/usr/bin/flame-screen}
pid=""

cleanup() {
  [ -n "$pid" ] && kill "$pid" 2>/dev/null
  /etc/init.d/gl_screen start
  exit 0
}
trap cleanup INT TERM

/etc/init.d/gl_screen stop
"$BIN" "$@" &
pid=$!
wait "$pid"
pid=""
cleanup
