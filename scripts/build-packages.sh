#!/bin/sh
# Build the OpenWrt packages (the feed in openwrt/) into artifacts/packages/:
#
#   sh scripts/build-packages.sh
#
# First builds what the SDK cannot: the settings page and its WebAssembly
# preview (pnpm run luci) and the Flint 4's flame-screen binary (in the
# rust-musl-cross container). Then runs the OpenWrt 25.12 SDK for
# mediatek/filogic in its container, with this repo as a feed. The SDK is
# downloaded once into the status-light-sdk volume and reused.
#
# Needs pnpm, cargo with the wasm32-unknown-unknown target, and docker (or
# podman as docker).

set -e

cd "$(dirname "$0")/.."
# Git Bash's /q/... form cannot be mounted; Windows needs Q:/...
root=$(pwd -W 2>/dev/null || pwd)
out="$root/artifacts/packages"

SDK_IMAGE=${SDK_IMAGE:-ghcr.io/openwrt/sdk:mediatek-filogic-openwrt-25.12}

pnpm run luci

docker run --rm -v "$root:/src" -w /src/devices/gl-inet-flint4/flame-screen \
  messense/rust-musl-cross:aarch64-musl cargo build --release --locked

rm -rf "$out"
mkdir -p "$out"
# The SDK runs as its own user, which may not own this directory (as in CI).
chmod 777 "$out"

# The repo is mounted read-only; packages come out through /out.
docker run --rm \
  -v status-light-sdk:/builder \
  -v "$root:/src:ro" \
  -v "$out:/out" \
  "$SDK_IMAGE" bash /src/openwrt/sdk-build.sh

ls -l "$out"
