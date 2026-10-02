#!/bin/sh
# Run flame-screen by hand (e.g. a test build from /tmp) instead of through the
# service. flame-screen stops gl_screen itself; this restarts it on exit
# (Ctrl-C, kill, or the binary dying), so the stock screen comes back.
BIN=${BIN:-/usr/bin/flame-screen}
pid=""

cleanup() {
  [ -n "$pid" ] && kill "$pid" 2>/dev/null
  /etc/init.d/gl_screen start
  exit 0
}
trap cleanup INT TERM

"$BIN" "$@" &
pid=$!
wait "$pid"
pid=""
cleanup
