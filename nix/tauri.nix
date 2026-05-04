final: prev: {
  tauri-dev-tools = prev.buildEnv {
    name = "tauri-dev-tools";
    paths = with prev; [
      webkitgtk_4_1
      gtk3
      glib-networking
      gsettings-desktop-schemas
      libsoup_3
      libsecret
      openssl
      cairo
      pango
      gdk-pixbuf
      at-spi2-atk
      atkmm
      pangomm
      gtkmm3
    ];
  };

  tauri-shell-hook = ''
    export WEBKIT_DISABLE_COMPOSITING_MODE=1
    export GIO_MODULE_DIR="${prev.glib-networking}/lib/gio/modules"
    export XDG_DATA_DIRS="${prev.gsettings-desktop-schemas.share}/share/gsettings-schemas/${prev.gsettings-desktop-schemas.name}:${prev.gtk3.share}/share:$XDG_DATA_DIRS"
    export GI_TYPELIB_PATH="${prev.lib.makeSearchPath "lib/girepository-1.0" [
      prev.webkitgtk_4_1
      prev.gtk3
      prev.libsoup_3
    ]}:$GI_TYPELIB_PATH"
  '';
}
