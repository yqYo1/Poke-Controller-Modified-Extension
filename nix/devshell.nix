{pkgs, ...}: let
  rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;

  pythonEnv = pkgs.python312.withPackages (ps:
    with ps; [
      pip
      venv
      maturin
      pygame
      pyserial
      pynput
      pillow
      opencv-python
      numpy
      pandas
      scipy
      requests
      pygubu
      paho-mqtt
      plyer
      gitpython
      imagehash
      numba
      pyocr
      pythonnet
      setuptools
      pytest
      ruff
      basedpyright
    ]);
in
  pkgs.mkShell {
    name = "pokecon-devshell";

    packages = with pkgs; [
      rustToolchain
      cargo-expand
      cargo-flamegraph
      rust-analyzer

      pythonEnv

      nodejs_20
      nodePackages.pnpm
      nodePackages.yarn

      pkg-config
      openssl
      sqlite
      curl
      wget
      git

      webkitgtk_4_1
      gtk3
      gst_all_1.gstreamer
      gst_all_1.gst-plugins-base
      gst_all_1.gst-plugins-good
      gst_all_1.gst-plugins-bad
      gst_all_1.gst-plugins-ugly
      gst_all_1.gst-libav

      appmenu-gtk-module
      nix-tree
    ];

    shellHook = ''
      export RUST_SRC_PATH="${pkgs.rustPlatform.rustLibSrc}"
      export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
      export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [
        pkgs.openssl
        pkgs.gtk3
        pkgs.webkitgtk_4_1
        pkgs.curl
      ]}:$LD_LIBRARY_PATH"

      export PYTHONPATH="${pythonEnv}/${pythonEnv.sitePackages}:$PYTHONPATH"

      echo "Poke Controller Modified Extension development environment loaded."
      echo "Rust: $(rustc --version)"
      echo "Python: $(python --version)"
      echo "Node.js: $(node --version)"
    '';
  }
