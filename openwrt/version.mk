# Shared by the feed's packages: the repo root, and the version, close to
# OpenWrt's own snapshot form (jsonfilter's 2026.03.16~b9034210): the last
# commit's UTC date and time, then its short hash, as
# 2026.10.05.170720~321ea8c6. The time keeps same-day builds in order.
# Included from a package directory, so CURDIR is openwrt/<package> (make
# resolves the feed's symlinks).
#
# scripts/build-packages.sh works it out from git and passes it in as
# STATUS_LIGHT_VERSION, since the SDK container gets the files without .git.
# Building in a checkout without it, git is asked directly.

STATUS_LIGHT_ROOT:=$(abspath $(CURDIR)/../..)

ifeq ($(STATUS_LIGHT_VERSION),)
  STATUS_LIGHT_GIT:=git -C $(STATUS_LIGHT_ROOT)
  STATUS_LIGHT_VERSION:=$(shell TZ=UTC0 $(STATUS_LIGHT_GIT) log -1 --format=%cd --date=format-local:%Y.%m.%d.%H%M%S)~$(shell $(STATUS_LIGHT_GIT) rev-parse --short=8 HEAD)
endif
ifeq ($(filter-out ~,$(STATUS_LIGHT_VERSION)),)
  $(error no version: set STATUS_LIGHT_VERSION, or build in a git checkout of the repo)
endif

PKG_VERSION:=$(STATUS_LIGHT_VERSION)
PKG_RELEASE:=1
PKG_LICENSE:=MIT
PKG_MAINTAINER:=Victor Irzak <victor.irzak@zomp.com>
