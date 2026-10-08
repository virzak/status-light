include $(TOPDIR)/rules.mk

PKG_NAME:=status-light-flame-screen
include $(CURDIR)/../version.mk

include $(INCLUDE_DIR)/package.mk

define Package/status-light-flame-screen
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=status-light flame on the GL.iNet Flint 4's LCD
  URL:=https://github.com/virzak/status-light
  DEPENDS:=@aarch64 +status-light
endef

define Package/status-light-flame-screen/description
  Shows the internet status as a blue flame on the GL.iNet Flint 4's
  built-in LCD while GL's screen UI sleeps. For that router only.
endef

# A static aarch64 binary built outside the SDK, as in
# devices/gl-inet-flint4/README.md (cargo in the rust-musl-cross container).
FLAME_DIR:=$(STATUS_LIGHT_ROOT)/devices/gl-inet-flint4/flame-screen
FLAME_BIN:=$(FLAME_DIR)/target/aarch64-unknown-linux-musl/release/flame-screen

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
	[ -f $(FLAME_BIN) ] || { echo "$(FLAME_BIN) is missing: build flame-screen first" >&2; exit 1; }
endef

define Build/Compile
endef

define Package/status-light-flame-screen/install
	$(INSTALL_DIR) $(1)/usr/bin $(1)/etc/init.d
	$(INSTALL_BIN) $(FLAME_BIN) $(1)/usr/bin/flame-screen
	$(INSTALL_BIN) $(FLAME_DIR)/flame-screen.init $(1)/etc/init.d/flame-screen
endef

$(eval $(call BuildPackage,status-light-flame-screen))
