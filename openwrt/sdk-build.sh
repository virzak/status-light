#!/bin/bash
# Runs inside the OpenWrt SDK container (see scripts/build-packages.sh): sets
# the SDK up on first use, adds the repo's files (in SRC, by default /tmp/src,
# writable by the SDK's own user) as the status_light feed, builds its
# packages and leaves them in /builder/out. STATUS_LIGHT_VERSION comes from
# the caller, as the files come without .git.

set -e

SRC=${SRC:-/tmp/src}

cd /builder
[ -f feeds.conf.default ] || bash setup.sh

sed -i '/^src-link status_light /d' feeds.conf.default
echo "src-link status_light $SRC/openwrt" >> feeds.conf.default
./scripts/feeds update base luci status_light
./scripts/feeds install -p status_light -a
make defconfig
make -j"$(nproc)" \
  package/status-light/compile \
  package/luci-app-status-light/compile \
  package/status-light-flame-screen/compile

rm -rf out
mkdir out
find bin/packages -path "*/status_light/*.apk" -exec cp {} out/ \;
ls out
