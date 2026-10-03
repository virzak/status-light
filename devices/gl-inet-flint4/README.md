# GL.iNet Flint 4 (GL-BE14000)

status-light was developed on this router. Everything in the repo works on it;
this folder adds what only applies to it.

## The built-in LCD

The Flint 4's screen is a standard Linux framebuffer (`/dev/fb0`, 240x320 RGB565,
GL's `st7789p3` driver), so `flame-screen` draws the shared flame straight into
it. GL's screen UI keeps running; when it sleeps (its screen timeout) the flame
fills the panel, and a touch wakes GL's UI instantly. It reads the state netled
writes to `/tmp/netled.state`, and blinks red if that file goes stale.

Build the static aarch64 binary in Docker, then install it and the service:

```
docker run --rm -v "$PWD:/src" -w /src/devices/gl-inet-flint4/flame-screen \
  messense/rust-musl-cross:aarch64-musl cargo build --release
scp -O devices/gl-inet-flint4/flame-screen/target/aarch64-unknown-linux-musl/release/flame-screen \
  devices/gl-inet-flint4/flame-screen/flame-screen.init "$ROUTER:/tmp/"
ssh "$ROUTER" 'sudo sh -c "cp /tmp/flame-screen /usr/bin/ && chmod 755 /usr/bin/flame-screen
  tr -d \"\\r\" < /tmp/flame-screen.init > /etc/init.d/flame-screen
  chmod 755 /etc/init.d/flame-screen
  /etc/init.d/flame-screen enable && /etc/init.d/flame-screen start"'
```

To go back to GL's stock screen:

```
/etc/init.d/flame-screen stop && /etc/init.d/flame-screen disable
```

These files are deliberately not in `/etc/sysupgrade.conf`, so a firmware upgrade
also restores the stock screen.

The Docker command mounts the whole repo because flame-screen builds the shared
`firmware/flame` crate; run it from the repo root.

## Notes on GL's firmware

- GL 4.11 is OpenWrt 25.12 with `apk`; LuCI is served on port 8080, since GL's own
  admin UI has port 80.
- netled needs no extra packages here: GL's image already has the USB serial
  driver (`kmod-usb-acm`).
