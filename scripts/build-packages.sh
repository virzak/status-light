#!/bin/sh
# Build the OpenWrt packages (the feed in openwrt/) into artifacts/packages/:
#
#   sh scripts/build-packages.sh
#
# First builds what the SDK cannot: the settings page and its WebAssembly
# preview (pnpm run luci) and the Flint 4's flame-screen binary (in the
# rust-musl-cross container). Then runs the OpenWrt 25.12 SDK for
# mediatek/filogic in its container. The SDK is downloaded once into the
# status-light-sdk volume and reused, as are crates in status-light-cargo.
#
# The containers get the files as a tar stream on stdin and send results back
# as a tar stream on stdout, with build output on stderr: no bind mounts, so
# this also works where the container engine cannot see this directory, as
# under act, where it runs in a container itself.
#
# Needs pnpm, cargo with the wasm32-unknown-unknown target, git, GNU tar, and
# docker, or any compatible command in DOCKER (DOCKER=podman).

set -e

cd "$(dirname "$0")/.."
out=artifacts/packages
flame=devices/gl-inet-flint4/flame-screen
flame_bin=$flame/target/aarch64-unknown-linux-musl/release/flame-screen

SDK_IMAGE=${SDK_IMAGE:-ghcr.io/openwrt/sdk:mediatek-filogic-openwrt-25.12}
RUST_IMAGE=${RUST_IMAGE:-messense/rust-musl-cross:aarch64-musl}
DOCKER=${DOCKER:-docker}

# The version, from git here, since the containers get no .git: the last
# commit's UTC date and time, then its hash. The time keeps two builds on the
# same day in order; apk would otherwise compare the hashes.
STATUS_LIGHT_VERSION="$(TZ=UTC0 git log -1 --format=%cd --date=format-local:%Y.%m.%d.%H%M%S)~$(git rev-parse --short=8 HEAD)"

# The tracked files plus the given build outputs, as a tar stream.
sources() {
  { git ls-files -z; for f in "$@"; do printf '%s\0' "$f"; done; } |
    tar --null --ignore-failed-read -T - -cf -
}

pnpm run luci

mkdir -p "$(dirname "$flame_bin")"
sources | $DOCKER run -i --rm -v status-light-cargo:/root/.cargo/registry \
  "$RUST_IMAGE" sh -c "
    set -e
    mkdir -p /src && tar -x -C /src
    cd /src/$flame && cargo build --release --locked >&2
    tar -C /src -cf - $flame_bin
  " | tar -x
[ -s "$flame_bin" ]

rm -rf "$out"
mkdir -p "$out"
sources \
  luci/www/luci-static/resources/view/status-light.js \
  luci/www/luci-static/resources/status-light/preview.wasm \
  "$flame_bin" |
  $DOCKER run -i --rm -v status-light-sdk:/builder \
    -e STATUS_LIGHT_VERSION="$STATUS_LIGHT_VERSION" \
    "$SDK_IMAGE" bash -c '
      set -e
      mkdir -p /tmp/src && tar -x -C /tmp/src
      bash /tmp/src/openwrt/sdk-build.sh >&2
      tar -C /builder/out -cf - .
    ' | tar -x -C "$out"

ls -l "$out"
ls "$out"/*.apk >/dev/null
