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
# gl_screen ignores SIGTERM; procd SIGKILLs it after about 5 s and `stop`
# returns before that, so wait until it is really gone.
i=0
while pidof gl_screen >/dev/null && [ "$i" -lt 15 ]; do
  sleep 1
  i=$((i + 1))
done
"$BIN" "$@" &
pid=$!
wait "$pid"
pid=""
cleanup
