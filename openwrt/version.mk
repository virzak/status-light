# Shared by the feed's packages: the repo root, and a version from git in
# OpenWrt's own snapshot form (as jsonfilter's 2026.03.16~b9034210), the date
# and short hash of the last commit. Included from a package directory, so
# CURDIR is openwrt/<package> in this repo (make resolves the feed's symlinks).

STATUS_LIGHT_ROOT:=$(abspath $(CURDIR)/../..)
STATUS_LIGHT_GIT:=git -c safe.directory='*' -C $(STATUS_LIGHT_ROOT)

STATUS_LIGHT_DATE:=$(shell $(STATUS_LIGHT_GIT) log -1 --format=%cd --date=format:%Y.%m.%d)
STATUS_LIGHT_HASH:=$(shell $(STATUS_LIGHT_GIT) rev-parse --short=8 HEAD)
ifeq ($(STATUS_LIGHT_DATE),)
  $(error cannot read git in $(STATUS_LIGHT_ROOT) for the version; is it a git checkout git may read?)
endif

PKG_VERSION:=$(STATUS_LIGHT_DATE)~$(STATUS_LIGHT_HASH)
PKG_RELEASE:=1
PKG_LICENSE:=MIT
PKG_MAINTAINER:=Victor Irzak <victor.irzak@zomp.com>
