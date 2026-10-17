#!/bin/bash
# Runs inside the OpenWrt SDK container (see scripts/build-packages.sh): sets
# the SDK up on first use, adds the repo's files (in SRC, by default /tmp/src,
# writable by the SDK's own user) as the status_light feed, builds its
# packages and leaves them in /builder/out. STATUS_LIGHT_VERSION comes from
# the caller, as the files come without .git.
#
# Each package is built in its own directory rather than with
# `make package/<name>/compile`, which first builds everything the package
# depends on: luci-base, jsonfilter and the kernel packages, minutes of work
# for packages the router already has. Ours only install files, so they need
# none of it, and the dependencies are still recorded in the .apk. For the
# same reason only this feed is added, not OpenWrt's base and LuCI feeds.

set -e

SRC=${SRC:-/tmp/src}
PACKAGES="status-light luci-app-status-light status-light-flame-screen"

cd /builder
[ -f feeds.conf.default ] || bash setup.sh

sed -i '/^src-link status_light /d' feeds.conf.default
echo "src-link status_light $SRC/openwrt" >> feeds.conf.default
./scripts/feeds update status_light
./scripts/feeds install -p status_light -a
# The target and package format (apk) come from the configuration.
make defconfig

mkdir -p out
rm -f out/*.apk
for p in $PACKAGES; do
  make -C "package/feeds/status_light/$p" TOPDIR=/builder compile
  cp bin/packages/*/status_light/"$p-$STATUS_LIGHT_VERSION"-r*.apk out/
done
ls out
