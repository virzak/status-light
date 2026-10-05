#!/bin/bash
# Runs inside the OpenWrt SDK container (see scripts/build-packages.sh): sets
# the SDK up on first use, adds this repo (mounted at /src) as the
# status_light feed, builds its packages and copies them to /out.

set -e

cd /builder
[ -f feeds.conf.default ] || bash setup.sh

# The repo belongs to another user in here, and this image's git ignores
# `-c safe.directory`, so allow it in the container's own git config.
git config --global --get-all safe.directory | grep -qx /src ||
  git config --global --add safe.directory /src

grep -q "^src-link status_light " feeds.conf.default ||
  echo "src-link status_light /src/openwrt" >> feeds.conf.default
./scripts/feeds update base luci status_light
./scripts/feeds install -p status_light -a
make defconfig
make -j"$(nproc)" \
  package/status-light/compile \
  package/luci-app-status-light/compile \
  package/status-light-flame-screen/compile
find bin/packages -path "*/status_light/*.apk" -exec cp {} /out/ \;
