#!/bin/sh
# Run flame-screen by hand (e.g. a test build from /tmp) instead of through the
# service. On exit (Ctrl-C, kill, or the binary dying) this restarts gl_screen
# so it repaints the whole panel.
BIN=${BIN:-/usr/bin/flame-screen}
pid=""

cleanup() {
  [ -n "$pid" ] && kill "$pid" 2>/dev/null
  /etc/init.d/gl_screen restart
  exit 0
}
trap cleanup INT TERM

"$BIN" "$@" &
pid=$!
wait "$pid"
pid=""
cleanup
