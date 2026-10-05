include $(TOPDIR)/rules.mk

PKG_NAME:=status-light
include $(CURDIR)/../version.mk

include $(INCLUDE_DIR)/package.mk

define Package/status-light
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Internet status light on USB boards
  URL:=https://github.com/virzak/status-light
  DEPENDS:=+jsonfilter +kmod-usb-acm
  PKGARCH:=all
endef

define Package/status-light/description
  netled checks the internet connection and sends the state to ESP32-S3
  status boards on USB (an LCD flame, an LED, an LED strip). Settings live in
  /etc/status-light.json.
endef

define Package/status-light/conffiles
/etc/status-light.json
endef

# Nothing to fetch or build: the files are the repo's own.
define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
endef

define Build/Compile
endef

define Package/status-light/install
	$(INSTALL_DIR) $(1)/usr/bin $(1)/etc/init.d
	$(INSTALL_BIN) $(STATUS_LIGHT_ROOT)/router/netled $(1)/usr/bin/netled
	$(INSTALL_BIN) $(STATUS_LIGHT_ROOT)/router/netled.init $(1)/etc/init.d/netled
	$(INSTALL_DATA) $(STATUS_LIGHT_ROOT)/router/status-light.json $(1)/etc/status-light.json
endef

$(eval $(call BuildPackage,status-light))
