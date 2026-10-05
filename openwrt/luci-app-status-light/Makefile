include $(TOPDIR)/rules.mk

PKG_NAME:=luci-app-status-light
include $(CURDIR)/../version.mk

include $(INCLUDE_DIR)/package.mk

define Package/luci-app-status-light
  SECTION:=luci
  CATEGORY:=LuCI
  SUBMENU:=3. Applications
  TITLE:=LuCI settings page for status-light
  URL:=https://github.com/virzak/status-light
  DEPENDS:=+luci-base +status-light
  PKGARCH:=all
endef

define Package/luci-app-status-light/description
  Services > Status Light: the boards' settings, with live previews of the
  flame and the LED strip.
endef

# The page and its preview module are built outside the SDK, by
# `pnpm run luci` in the repo (TypeScript and Rust to WebAssembly).
LUCI_DIR:=$(STATUS_LIGHT_ROOT)/luci
LUCI_BUILT:=$(LUCI_DIR)/www/luci-static/resources/view/status-light.js \
	$(LUCI_DIR)/www/luci-static/resources/status-light/preview.wasm

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
	$(foreach f,$(LUCI_BUILT),[ -f $(f) ] || { echo "$(f) is missing: run pnpm run luci in the repo first" >&2; exit 1; };)
endef

define Build/Compile
endef

define Package/luci-app-status-light/install
	$(INSTALL_DIR) $(1)/www/luci-static/resources/view $(1)/www/luci-static/resources/status-light
	$(INSTALL_DATA) $(LUCI_DIR)/www/luci-static/resources/view/status-light.js $(1)/www/luci-static/resources/view/
	$(INSTALL_DATA) $(LUCI_DIR)/www/luci-static/resources/status-light/preview.wasm $(1)/www/luci-static/resources/status-light/
	$(INSTALL_DIR) $(1)/usr/share/luci/menu.d $(1)/usr/share/rpcd/acl.d
	$(INSTALL_DATA) $(LUCI_DIR)/usr/share/luci/menu.d/luci-app-status-light.json $(1)/usr/share/luci/menu.d/
	$(INSTALL_DATA) $(LUCI_DIR)/usr/share/rpcd/acl.d/luci-app-status-light.json $(1)/usr/share/rpcd/acl.d/
endef

# Pick up the new menu entry and permissions.
define Package/luci-app-status-light/postinst
#!/bin/sh
[ -n "$${IPKG_INSTROOT}" ] && exit 0
rm -f /tmp/luci-indexcache*
rm -rf /tmp/luci-modulecache
/etc/init.d/rpcd reload
exit 0
endef

$(eval $(call BuildPackage,luci-app-status-light))
