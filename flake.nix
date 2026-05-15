{
  description = "Poke Controller Modified Extension development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";
    systems.url = "github:nix-systems/default";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    git-hooks.url = "github:cachix/git-hooks.nix";
    git-hooks.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    inputs@{
      self,
      flake-parts,
      systems,
      treefmt-nix,
      rust-overlay,
      git-hooks,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = import systems;

      imports = [
        treefmt-nix.flakeModule
        git-hooks.flakeModule
      ];

      perSystem =
        {
          config,
          pkgs,
          system,
          ...
        }:
        let
          pkgsWithOverlays = import inputs.nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rustToolchain = pkgsWithOverlays.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

          # buildRustPackage is available via rustPlatform in nixpkgs
          buildRustPackage = pkgs.rustPlatform.buildRustPackage;

          # ── App helpers ────────────────────────────────────────────────
          mkApp = program: {
            type = "app";
            inherit program;
          };

          # ── Shared workdir setup for apps needing a writable source copy ──
          setupWorkdir = ''
            workdir="$(mktemp -d)"
            trap 'rm -rf "$workdir"' EXIT
            cp -r "${self}/." "$workdir/"
            chmod -R +w "$workdir"
            cd "$workdir"
          '';

          # ── Poke-Controller application package ────────────────────────
          pokeconApp = pkgs.writeShellApplication {
            name = "pokecon";
            runtimeInputs = [
              rustEnv
              pythonEnv
              pkgs.maturin
            ];
            text = ''
                            # When run remotely, self is in nix store (read-only).
                            # Use a writable temp dir for build artifacts.
                            workdir="$(mktemp -d)"
                            trap 'rm -rf "$workdir"' EXIT
                            
                            # Copy source to writable location for maturin build
                            cp -r "${self}/." "$workdir/"
                            chmod -R +w "$workdir"
                            
                            # Build Python bindings in the writable copy
                            found_so=
                            for f in "$workdir/python/pokecon/pokecon"*.so "$workdir/python/pokecon/pokecon"*.pyd; do
                              if [ -f "$f" ]; then
                                found_so=1
                                break
                              fi
                            done
                            if [ -z "$found_so" ]; then
                              echo "Building Python bindings..."
                              maturin build --release \
                                --manifest-path "$workdir/rust/pokecon-pybindings/Cargo.toml" \
                                --out "$workdir/dist/" 2>&1
                              wheel=$(find "$workdir/dist" -name 'pokecon-*.whl' 2>/dev/null | head -1)
                              if [ -n "$wheel" ]; then
                                python -c "
              import zipfile
              import os
              wheel = '$wheel'
              out_dir = '$workdir/python/pokecon'
              os.makedirs(out_dir, exist_ok=True)
              with zipfile.ZipFile(wheel, 'r') as z:
                  for name in z.namelist():
                      if name.endswith('.so') or name.endswith('.pyd'):
                          z.extract(name, out_dir)
                          basename = os.path.basename(name)
                          os.rename(os.path.join(out_dir, name), os.path.join(out_dir, basename))
              "
                              fi
                            fi
                            
                            # Launch the application from the writable copy
                            PYTHONPATH="$workdir/python''${PYTHONPATH:+:$PYTHONPATH}"
                            export PYTHONPATH
                            cd "$workdir"
                            exec python -m pokecon "$@"
            '';
          };

          rustEnv = rustToolchain; # includes cargo, rustc, clippy-driver, rustfmt
          pythonEnv = pkgs.python314.withPackages (
            ps: with ps; [
              pytest
              numpy
              scipy
              ruff
              pillow
            ]
          );

          # ── Python test deps ─────────────────────────────────────────────
          pythonPkgs = with pkgs.python314Packages; [
            pytest
            numpy
            scipy
            ruff
            pillow
          ];

          # ── npm dependencies (shared between Tauri build and check) ──
          npmDeps = pkgs.fetchNpmDeps {
            name = "pokecon-npm-deps";
            src = ./web;
            hash = "sha256-2H2CeQurs9PyXTAJg9ber7m/7Sad3v29UhNXPMYzWAY=";
            makeCacheWritable = true;
            npmFlags = [ "--legacy-peer-deps" ];
          };
        in
        {
          treefmt = {
            projectRootFile = "flake.nix";
            programs = {
              nixfmt.enable = true;
              rustfmt.enable = true;
              ruff-check.enable = true;
              ruff-format.enable = true;
            };
            settings = {
              global.excludes = [
                "*.lock"
                ".git/**"
                "target/**"
                "result*"
                ".venv/**"
                "__pycache__/**"
                ".mypy_cache/**"
                ".ruff_cache/**"
                "node_modules/**"
              ];
              formatter = {
                nixfmt = {
                  includes = [ "*.nix" ];
                };
                rustfmt = {
                  includes = [ "*.rs" ];
                };
                ruff-format = {
                  includes = [ "*.py" ];
                  options = [
                    "--config=pyproject.toml"
                    "--no-cache"
                  ];
                };
                ruff-check = {
                  includes = [ "*.py" ];
                  options = [
                    "--no-cache"
                    "--select"
                    "E,W,F,I"
                    "--ignore"
                    "E402,E501,E722,E741,F821,F841"
                  ];
                };
              };
            };
          };

          packages = {
            # Tauri package built with nixpkgs best practices
            pokecon-tauri = pkgs.rustPlatform.buildRustPackage (finalAttrs: {
              pname = "pokecon-tauri";
              version = "0.1.0";
              src = builtins.path {
                path = self.outPath;
                name = "pokecon-source";
                filter =
                  path: type:
                  let
                    p = toString path;
                  in
                  type == "directory"
                  || pkgs.lib.hasSuffix ".rs" p
                  || pkgs.lib.hasSuffix ".toml" p
                  || pkgs.lib.hasSuffix ".json" p
                  || pkgs.lib.hasSuffix ".html" p
                  || pkgs.lib.hasSuffix ".css" p
                  || pkgs.lib.hasSuffix ".js" p
                  || pkgs.lib.hasSuffix ".jsx" p
                  || pkgs.lib.hasSuffix ".ts" p
                  || pkgs.lib.hasSuffix ".tsx" p
                  || pkgs.lib.hasSuffix ".png" p
                  || pkgs.lib.hasSuffix ".lock" p
                  || pkgs.lib.hasSuffix ".md" p
                  || pkgs.lib.hasSuffix ".py" p
                  || pkgs.lib.hasSuffix ".nix" p
                  || pkgs.lib.hasSuffix ".conf" p
                  || pkgs.lib.hasSuffix ".yml" p
                  || pkgs.lib.hasSuffix ".yaml" p
                  || pkgs.lib.hasSuffix ".txt" p
                  || pkgs.lib.hasSuffix ".svg" p
                  || pkgs.lib.hasSuffix ".woff" p
                  || pkgs.lib.hasSuffix ".woff2" p
                  || pkgs.lib.hasSuffix ".ttf" p
                  || pkgs.lib.hasSuffix ".eot" p;
              };

              cargoLock = {
                lockFile = self + "/src-tauri/Cargo.lock";
                allowBuiltinFetchGit = true;
              };

              cargoRoot = "src-tauri";
              buildAndTestSubdir = finalAttrs.cargoRoot;

              # npm frontend dependencies
              inherit npmDeps;

              # Copy package-lock.json and package.json to root for npmConfigHook
              # Copy actual icons for Tauri build
              postPatch = ''
                cp ${self}/web/package-lock.json ./package-lock.json
                cp ${self}/web/package.json ./package.json

                # Copy actual icons from source
                mkdir -p src-tauri/icons
                cp ${self}/src-tauri/icons/* src-tauri/icons/ 2>/dev/null || true
              '';

              nativeBuildInputs = with pkgs; [
                cargo-tauri.hook
                nodejs_20
                npmHooks.npmConfigHook
                pkg-config
                wrapGAppsHook4
              ];

              buildInputs = with pkgs; [
                glib
                glib-networking
                gtk3
                pango
                harfbuzz
                cairo
                atk
                gdk-pixbuf
                libsoup_3
                webkitgtk_4_1
                librsvg
                dbus
                libx11
                libcanberra-gtk3
              ];

              # Skip tauri-build runtime validation in sandbox
              TAURI_SKIP_BUILD = "1";

              # Disable tauri bundling, we just need the binary
              tauriBundleType = "";

              # Disable default tauri build hook and build manually
              dontTauriBuild = true;

              # Build web UI and Tauri binary manually
              buildPhase = ''
                runHook preBuild

                # Build web UI (npm already configured by npmHooks.npmConfigHook)
                cd web
                npx vite build
                cd ..

                # Build Tauri binary
                cd src-tauri
                cargo build --release --offline
                cd ..

                runHook postBuild
              '';

              installPhase = ''
                runHook preInstall
                mkdir -p $out/bin
                cp src-tauri/target/release/pokecon-tauri $out/bin/
                # Copy built web assets for runtime serving
                mkdir -p $out/web/dist
                cp -r web/dist/* $out/web/dist/ 2>/dev/null || true
                runHook postInstall
              '';

              doCheck = false;
            });
          };

          apps = {
            # nix run .  — launch Poke-Controller application (Tauri or Web UI)
            default =
              let
                pokecon-tauri = config.packages.pokecon-tauri;

                # Wrapper script for runtime behavior (cache, UI mode detection)
                pokecon-launcher = pkgs.writeShellScriptBin "pokecon" ''
                  # Set GSettings backend to memory to avoid D-Bus dependency
                  export GSETTINGS_BACKEND=memory

                  # Force X11 backend for WebKitGTK EGL compatibility
                  export GDK_BACKEND=x11

                  # Allow user to override WebKitGTK compositing mode via environment
                  # Default: enabled (hardware acceleration). Set POKECON_DISABLE_COMPOSITING=1 to disable.
                  # NOTE: WebKitGTK reads this environment variable at process startup only,
                  # so it must be set before launch. Command-line arguments cannot be used
                  # because WebKitGTK's compositing mode is initialized before application code runs.
                  if [ -n "''${POKECON_DISABLE_COMPOSITING:-}" ]; then
                    export WEBKIT_DISABLE_COMPOSITING_MODE=1
                    export WEBKIT_DISABLE_DMABUF_RENDERER=1
                  fi

                  # Detect GUI environment for default mode
                  UI_MODE="web"
                  if [ -n "''${DISPLAY:-}" ] || [ -n "''${WAYLAND_DISPLAY:-}" ]; then
                    UI_MODE="tauri"
                  fi

                  # Check if user explicitly specified --ui
                  if [[ "$*" == *"--ui"* ]]; then
                    # User specified mode explicitly — parse it for logging
                    USER_MODE="$UI_MODE"
                    for arg in "$@"; do
                      if [ "$arg" = "--ui" ]; then
                        NEXT_IS_UI=1
                      elif [ "''${NEXT_IS_UI:-}" = "1" ]; then
                        USER_MODE="$arg"
                        NEXT_IS_UI=0
                      fi
                    done
                    echo "=== Launching in $USER_MODE mode (explicit) ==="
                    exec "${pokecon-tauri}/bin/pokecon-tauri" \
                      --web-dir "${pokecon-tauri}/web/dist" \
                      "$@"
                  else
                    # Default: auto-detect based on GUI environment
                    echo "=== Launching in $UI_MODE mode (auto-detected) ==="
                    exec "${pokecon-tauri}/bin/pokecon-tauri" \
                      --ui "$UI_MODE" \
                      --web-dir "${pokecon-tauri}/web/dist" \
                      "$@"
                  fi
                '';
              in
              mkApp "${pokecon-launcher}/bin/pokecon";

            # nix run .#python-app  — launch Python-based entry point (legacy)
            python-app = mkApp "${pokeconApp}/bin/pokecon";

            fmt = mkApp "${config.treefmt.build.wrapper}/bin/treefmt";

            # nix run .#clippy  — run Rust linter
            clippy = mkApp "${
              pkgs.writeShellApplication {
                name = "clippy";
                runtimeInputs = [ rustEnv ];
                text = ''
                  ${setupWorkdir}
                  cargo clippy --workspace --all-targets --all-features --exclude pokecon-pybindings -- -D warnings
                '';
              }
            }/bin/clippy";

            # nix run .#ruff-check  — run Python linter
            ruff-check = mkApp "${
              pkgs.writeShellApplication {
                name = "ruff-check";
                runtimeInputs = [ pythonEnv ];
                text = ''
                  cd "${self}"
                  export PYTHONPATH="${self}/python''${PYTHONPATH:+:$PYTHONPATH}"
                  ruff check --no-cache --select E,W,F --ignore E402,E501,E722,E741,F821,F841 .
                '';
              }
            }/bin/ruff-check";

            # nix run .#ruff-format  — format Python files
            ruff-format = mkApp "${
              pkgs.writeShellApplication {
                name = "ruff-format";
                runtimeInputs = [ pythonEnv ];
                text = ''
                  cd "${self}"
                  ruff format .
                '';
              }
            }/bin/ruff-format";

            # nix run .#ruff-format-check  — check Python formatting (CI)
            ruff-format-check = mkApp "${
              pkgs.writeShellApplication {
                name = "ruff-format-check";
                runtimeInputs = [ pythonEnv ];
                text = ''
                  cd "${self}"
                  ruff format --no-cache --check .
                '';
              }
            }/bin/ruff-format-check";

            # nix run .#basedpyright  — run Python type checker
            basedpyright = mkApp "${
              pkgs.writeShellApplication {
                name = "basedpyright";
                runtimeInputs = [
                  pythonEnv
                  pkgs.basedpyright
                ];
                text = ''
                  cd "${self}"
                  exec basedpyright python/
                '';
              }
            }/bin/basedpyright";

            # nix run .#test  — run pytest
            test = mkApp "${
              pkgs.writeShellApplication {
                name = "test";
                runtimeInputs = [ pythonEnv ];
                text = ''
                  cd "${self}"
                  export PYTHONPATH="${self}/python''${PYTHONPATH:+:$PYTHONPATH}"
                  exec pytest -p no:cacheprovider tests/ -v --tb=short
                '';
              }
            }/bin/test";

            # nix run .#build  — build Rust workspace + Python maturin package
            build = mkApp "${
              pkgs.writeShellApplication {
                name = "build";
                runtimeInputs = [
                  rustEnv
                  pythonEnv
                  pkgs.maturin
                ];
                text = ''
                  ${setupWorkdir}
                  echo "=== Building Rust workspace ==="
                  cargo build --workspace --all-features
                  echo ""
                  echo "=== Building Python maturin package ==="
                  maturin build --release --manifest-path rust/pokecon-pybindings/Cargo.toml --out dist/
                '';
              }
            }/bin/build";

            # nix run .#build-rust  — build Rust workspace only
            build-rust = mkApp "${
              pkgs.writeShellApplication {
                name = "build-rust";
                runtimeInputs = [ rustEnv ];
                text = ''
                  ${setupWorkdir}
                  cargo build --workspace --all-features
                '';
              }
            }/bin/build-rust";

            # nix run .#cargo-test  — run Rust tests
            cargo-test =
              let
                cargoTestScript = pkgs.writeShellApplication {
                  name = "cargo-test";
                  runtimeInputs = [
                    rustEnv
                    pkgs.libclang
                  ];
                  text = ''
                    ${setupWorkdir}

                    # libclang is required for v4l2-sys-mit (bindgen)
                    export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"

                    echo "=== Running cargo test ==="
                    cargo test --workspace --all-features --exclude pokecon-pybindings
                  '';
                };
              in
              mkApp "${
                pkgs.buildFHSEnv {
                  name = "cargo-test-fhs";
                  targetPkgs = pkgs: [
                    rustEnv
                    pkgs.libclang
                    pkgs.gcc
                    pkgs.linuxHeaders
                    pkgs.glibc.dev
                  ];
                  runScript = "${cargoTestScript}/bin/cargo-test";
                }
              }/bin/cargo-test-fhs";

            # nix run .#typos  — run spell checker
            typos = mkApp "${
              pkgs.writeShellApplication {
                name = "typos";
                runtimeInputs = [ pkgs.typos ];
                text = ''
                  cd "${self}"
                  exec typos "$@"
                '';
              }
            }/bin/typos";

            # nix run .#typos-check  — check typos (CI, no fixes)
            typos-check = mkApp "${
              pkgs.writeShellApplication {
                name = "typos-check";
                runtimeInputs = [ pkgs.typos ];
                text = ''
                  cd "${self}"
                  exec typos --no-exit-code "$@"
                '';
              }
            }/bin/typos-check";
            maturin-develop = mkApp "${
              pkgs.writeShellApplication {
                name = "maturin-develop";
                runtimeInputs = [
                  rustEnv
                  pythonEnv
                  pkgs.maturin
                ];
                text = ''
                                    # When run remotely, self is in nix store (read-only).
                                    # Use a writable temp dir for build artifacts.
                                    workdir="$(mktemp -d)"
                                    trap 'rm -rf "$workdir"' EXIT
                                    
                                    # Copy source to writable location
                                    cp -r "${self}/." "$workdir/"
                                    chmod -R +w "$workdir"
                                    
                                    echo "=== Building Python bindings in-place ==="
                                    cd "$workdir"
                                    maturin build --release \
                                      --manifest-path "$workdir/rust/pokecon-pybindings/Cargo.toml" \
                                      --out "$workdir/dist/" 2>&1
                                    wheel=$(find "$workdir/dist" -name 'pokecon-*.whl' 2>/dev/null | head -1)
                                    if [ -n "$wheel" ]; then
                                      python -c "
                  import zipfile
                  import os
                  wheel = '$wheel'
                  out_dir = '$workdir/python/pokecon'
                  os.makedirs(out_dir, exist_ok=True)
                  with zipfile.ZipFile(wheel, 'r') as z:
                      for name in z.namelist():
                          if name.endswith('.so') or name.endswith('.pyd'):
                              z.extract(name, out_dir)
                              basename = os.path.basename(name)
                              os.rename(os.path.join(out_dir, name), os.path.join(out_dir, basename))
                  "
                                      echo "✓ Python bindings built and installed to $workdir/python/pokecon/"
                                    else
                                      echo "Error: No wheel was built" >&2
                                      exit 1
                                    fi
                '';
              }
            }/bin/maturin-develop";

            # nix run .#tauri-build  — build Tauri app (release mode)
            # Wrapped in buildFHSEnv for non-NixOS compatibility (GLIBC/GCC/libclang ABI)
            tauri-build =
              let
                tauriBuildScript = pkgs.writeShellApplication {
                  name = "tauri-build";
                  runtimeInputs = [
                    rustEnv
                    pkgs.pkg-config
                    pkgs.libclang
                    pkgs.glib
                    pkgs.glib-networking
                    pkgs.gtk3
                    pkgs.pango
                    pkgs.harfbuzz
                    pkgs.cairo
                    pkgs.atk
                    pkgs.gdk-pixbuf
                    pkgs.libsoup_3
                    pkgs.webkitgtk_4_1
                    pkgs.librsvg
                    pkgs.dbus
                    pkgs.libx11
                    pkgs.libxcursor
                    pkgs.libxrandr
                    pkgs.libxi
                    pkgs.libcanberra-gtk3
                    pkgs.gst_all_1.gstreamer
                    pkgs.gst_all_1.gst-plugins-base
                    pkgs.gst_all_1.gst-plugins-good
                  ];
                  text = ''
                    # Set PKG_CONFIG_PATH for all GTK/WebKit dependencies
                    export PKG_CONFIG_PATH="${pkgs.glib.dev}/lib/pkgconfig:${pkgs.gtk3.dev}/lib/pkgconfig:${pkgs.pango.dev}/lib/pkgconfig:${pkgs.harfbuzz.dev}/lib/pkgconfig:${pkgs.cairo.dev}/lib/pkgconfig:${pkgs.atk.dev}/lib/pkgconfig:${pkgs.gdk-pixbuf.dev}/lib/pkgconfig:${pkgs.libsoup_3.dev}/lib/pkgconfig:${pkgs.webkitgtk_4_1.dev}/lib/pkgconfig:${pkgs.zlib.dev}/share/pkgconfig:${pkgs.dbus.dev}/lib/pkgconfig:${pkgs.libx11.dev}/lib/pkgconfig:${pkgs.libxcursor.dev}/lib/pkgconfig:${pkgs.libxrandr.dev}/lib/pkgconfig:${pkgs.libxi.dev}/lib/pkgconfig''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

                    # libclang is required for v4l2-sys-mit (bindgen)
                    export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"

                    # Skip tauri-build's runtime validation (no display in build)
                    export TAURI_SKIP_BUILD=1

                    # Copy source to writable temp dir
                    workdir="$(mktemp -d)"
                    trap 'rm -rf "$workdir"' EXIT
                    cp -r "${self}/." "$workdir/"
                    chmod -R +w "$workdir"

                    cd "$workdir/src-tauri"
                    echo "=== Building Tauri app (release) ==="
                    cargo build --release --all-features
                    echo ""
                    echo "✓ Build complete. Binary at $workdir/src-tauri/target/release/pokecon-tauri"
                  '';
                };
              in
              mkApp "${
                pkgs.buildFHSEnv {
                  name = "tauri-build-fhs";
                  targetPkgs = pkgs: [
                    rustEnv
                    pkgs.pkg-config
                    pkgs.libclang
                    pkgs.glib
                    pkgs.glib-networking
                    pkgs.gtk3
                    pkgs.pango
                    pkgs.harfbuzz
                    pkgs.cairo
                    pkgs.atk
                    pkgs.gdk-pixbuf
                    pkgs.libsoup_3
                    pkgs.webkitgtk_4_1
                    pkgs.librsvg
                    pkgs.dbus
                    pkgs.libx11
                    pkgs.libxcursor
                    pkgs.libxrandr
                    pkgs.libxi
                    pkgs.libcanberra-gtk3
                    pkgs.gst_all_1.gstreamer
                    pkgs.gst_all_1.gst-plugins-base
                    pkgs.gst_all_1.gst-plugins-good
                    pkgs.gcc
                    pkgs.linuxHeaders
                    pkgs.glibc.dev
                    pkgs.zlib.dev
                    pkgs.openssl.dev
                  ];
                  runScript = "${tauriBuildScript}/bin/tauri-build";
                }
              }/bin/tauri-build-fhs";

            # nix run .#tauri-dev  — run Tauri dev server
            # Wrapped in buildFHSEnv for non-NixOS compatibility (GLIBC/GCC/libclang ABI)
            tauri-dev =
              let
                tauriDevScript = pkgs.writeShellApplication {
                  name = "tauri-dev-script";
                  runtimeInputs = [
                    rustEnv
                    pkgs.cargo-tauri
                    pkgs.pkg-config
                    pkgs.libclang
                    pkgs.glib
                    pkgs.glib-networking
                    pkgs.gtk3
                    pkgs.pango
                    pkgs.harfbuzz
                    pkgs.cairo
                    pkgs.atk
                    pkgs.gdk-pixbuf
                    pkgs.libsoup_3
                    pkgs.webkitgtk_4_1
                    pkgs.librsvg
                    pkgs.dbus
                    pkgs.libx11
                    pkgs.libxcursor
                    pkgs.libxrandr
                    pkgs.libxi
                    pkgs.libcanberra-gtk3
                    pkgs.gst_all_1.gstreamer
                    pkgs.gst_all_1.gst-plugins-base
                    pkgs.gst_all_1.gst-plugins-good
                  ];
                  text = ''
                    # Set PKG_CONFIG_PATH for all GTK/WebKit dependencies (zlib is in share/pkgconfig)
                    export PKG_CONFIG_PATH="${pkgs.glib.dev}/lib/pkgconfig:${pkgs.gtk3.dev}/lib/pkgconfig:${pkgs.pango.dev}/lib/pkgconfig:${pkgs.harfbuzz.dev}/lib/pkgconfig:${pkgs.cairo.dev}/lib/pkgconfig:${pkgs.atk.dev}/lib/pkgconfig:${pkgs.gdk-pixbuf.dev}/lib/pkgconfig:${pkgs.libsoup_3.dev}/lib/pkgconfig:${pkgs.webkitgtk_4_1.dev}/lib/pkgconfig:${pkgs.zlib.dev}/share/pkgconfig:${pkgs.dbus.dev}/lib/pkgconfig:${pkgs.libx11.dev}/lib/pkgconfig:${pkgs.libxcursor.dev}/lib/pkgconfig:${pkgs.libxrandr.dev}/lib/pkgconfig:${pkgs.libxi.dev}/lib/pkgconfig''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

                    # libclang is required for v4l2-sys-mit (bindgen)
                    export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"

                    # Copy source to writable temp dir
                    workdir="$(mktemp -d)"
                    trap 'rm -rf "$workdir"' EXIT
                    cp -r "${self}/." "$workdir/"
                    chmod -R +w "$workdir"

                    cd "$workdir/src-tauri"
                    exec cargo tauri dev
                  '';
                };
              in
              mkApp "${
                pkgs.buildFHSEnv {
                  name = "tauri-dev-fhs";
                  targetPkgs = pkgs: [
                    rustEnv
                    pkgs.cargo-tauri
                    pkgs.pkg-config
                    pkgs.libclang
                    pkgs.glib
                    pkgs.glib-networking
                    pkgs.gtk3
                    pkgs.pango
                    pkgs.harfbuzz
                    pkgs.cairo
                    pkgs.atk
                    pkgs.gdk-pixbuf
                    pkgs.libsoup_3
                    pkgs.webkitgtk_4_1
                    pkgs.librsvg
                    pkgs.dbus
                    pkgs.libx11
                    pkgs.libxcursor
                    pkgs.libxrandr
                    pkgs.libxi
                    pkgs.libcanberra-gtk3
                    pkgs.gst_all_1.gstreamer
                    pkgs.gst_all_1.gst-plugins-base
                    pkgs.gst_all_1.gst-plugins-good
                    pkgs.gcc
                    pkgs.linuxHeaders
                    pkgs.glibc.dev
                    pkgs.zlib.dev
                    pkgs.openssl.dev
                  ];
                  runScript = "${tauriDevScript}/bin/tauri-dev-script";
                }
              }/bin/tauri-dev-fhs";

            # nix run .#tauri  — build Tauri app (verify Cargo.lock is present)
            tauri = mkApp "${config.packages.pokecon-tauri}/bin/pokecon-tauri";

            # nix run .#npm-update  — update npm deps and sync flake.nix hash
            # Usage: nix run .#npm-update
            # This updates package-lock.json and patches flake.nix with the new hash.
            npm-update = mkApp "${
              pkgs.writeShellApplication {
                name = "npm-update";
                runtimeInputs = [
                  pkgs.nodejs_20
                  pkgs.nix
                  pkgs.gnused
                  pkgs.gnugrep
                ];
                text = ''
                  set -euo pipefail

                  WEB_DIR="${self}/web"
                  FLAKE="${self}/flake.nix"

                  echo "=== Updating npm dependencies ==="
                  cd "$WEB_DIR"
                  npm install "$@"

                  echo ""
                  echo "=== Computing new npm deps hash === "
                  echo "NOTE: fetchNpmDeps expects a recursive NAR hash (SRI format)"
                  echo "of the node_modules directory, NOT a flat hash of package-lock.json."
                  echo ""

                  # Create a temp directory to generate the npm cache
                  TMPDIR="$(mktemp -d)"
                  cd "$TMPDIR"
                  cp "$WEB_DIR/package.json" .
                  cp "$WEB_DIR/package-lock.json" .

                  # Install dependencies to generate node_modules
                  echo "Installing npm dependencies to compute hash..."
                  npm ci --legacy-peer-deps 2>&1

                  # Compute the correct recursive NAR hash in SRI format
                  echo "Computing recursive hash..."
                  B64_HASH=$(nix-hash --type sha256 --base64 node_modules 2>&1 | tail -1)
                  NEW_HASH="sha256-''${B64_HASH}"

                  if [ -z "$NEW_HASH" ] || [ "$NEW_HASH" = "sha256-" ]; then
                    echo "ERROR: Failed to compute npm deps hash" >&2
                    echo "You may need to update the hash manually in flake.nix" >&2
                    exit 1
                  fi

                  echo "New hash: $NEW_HASH"

                  # Update the hash in flake.nix
                  sed -i "s|hash = \"sha256-.*\";|hash = \"$NEW_HASH\";|" "$FLAKE"

                  echo ""
                  echo "=== Updated flake.nix with new npm deps hash ==="
                  echo "Please commit both package-lock.json and flake.nix changes"
                '';
              }
            }/bin/npm-update";

            # nix run .#check  — run ALL checks (CI gate)
            # Wrapped in buildFHSEnv to isolate libclang from host glibc
            check =
              let
                checkScript = pkgs.writeShellApplication {
                  name = "check";
                  runtimeInputs = [
                    rustEnv
                    pkgs.libclang
                  ];
                  text = ''
                    cd "${self}"

                    # libclang is required for v4l2-sys-mit (bindgen)
                    export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"

                    echo "═══════════════════════════════════════════"
                    echo "  web-check (eslint + svelte-check + vitest)"
                    echo "═══════════════════════════════════════════"
                    (cd "${self}/web"
                      echo "=== Installing npm dependencies (pre-fetched) ==="
                      export npm_config_cache="${npmDeps}"
                      npm ci --offline --legacy-peer-deps 2>&1
                      echo ""
                      echo "=== Running svelte-kit sync ==="
                      npx svelte-kit sync 2>&1
                      echo ""
                      echo "--- eslint ---"
                      npm run lint 2>&1
                      echo ""
                      echo "--- svelte-check ---"
                      npm run svelte-check 2>&1
                      echo ""
                      echo "--- vitest ---"
                      npm test 2>&1
                    ) || { echo "✗ web-check failed"; exit 1; }
                    echo ""
                    echo "═══════════════════════════════════════════"
                    echo "  clippy"
                    echo "═══════════════════════════════════════════"
                    cargo clippy --workspace --all-targets --all-features --exclude pokecon-pybindings -- -D warnings
                    echo ""
                    echo "═══════════════════════════════════════════"
                    echo "  cargo test"
                    echo "═══════════════════════════════════════════"
                    cargo test --workspace --all-features --exclude pokecon-pybindings
                    echo ""
                    echo "═══════════════════════════════════════════"
                    echo "  ruff check"
                    echo "═══════════════════════════════════════════"
                    export PYTHONPATH="${self}/python''${PYTHONPATH:+:$PYTHONPATH}"
                    ruff check --no-cache --select E,W,F --ignore E402,E501,E722,E741,F821,F841 .
                    echo ""
                    echo "═══════════════════════════════════════════"
                    echo "  pytest"
                    echo "═══════════════════════════════════════════"
                    export PYTHONPATH="${self}/python''${PYTHONPATH:+:$PYTHONPATH}"
                    pytest -p no:cacheprovider tests/ -v --tb=short
                    echo ""
                    echo "═══════════════════════════════════════════"
                    echo "  typos (spell check)"
                    echo "═══════════════════════════════════════════"
                    ${pkgs.typos}/bin/typos
                    echo ""
                    echo "═══════════════════════════════════════════"
                    echo "  formatting (check mode)"
                    echo "═══════════════════════════════════════════"
                    ${config.treefmt.build.wrapper}/bin/treefmt --ci --working-dir "${self}"
                    echo ""
                    echo "✓ All checks passed"
                  '';
                };
              in
              mkApp "${
                pkgs.buildFHSEnv {
                  name = "check-fhs";
                  targetPkgs = pkgs: [
                    rustEnv
                    pythonEnv
                    pkgs.libclang
                    pkgs.typos
                    config.treefmt.build.wrapper
                    pkgs.gcc
                    pkgs.linuxHeaders
                    pkgs.glibc.dev
                    pkgs.nodejs_20
                  ];
                  runScript = "${checkScript}/bin/check";
                }
              }/bin/check-fhs";

            # nix run .#web-check  — run TypeScript/JS checks (eslint + svelte-check + vitest)
            web-check = mkApp "${
              pkgs.writeShellApplication {
                name = "web-check";
                runtimeInputs = [
                  pkgs.nodejs_20
                ];
                text = ''
                  workdir="$(mktemp -d)"
                  trap 'rm -rf "$workdir"' EXIT
                  cp -r "${self}/web/." "$workdir/"
                  chmod -R +w "$workdir"
                  cd "$workdir"

                  echo "=== Installing npm dependencies ==="
                  npm ci --legacy-peer-deps 2>&1

                  echo ""
                  echo "=== Running svelte-kit sync ==="
                  npx svelte-kit sync 2>&1

                  echo ""
                  echo "═══════════════════════════════════════════"
                  echo "  eslint"
                  echo "═══════════════════════════════════════════"
                  npm run lint 2>&1 || EXIT_CODE=$?

                  echo ""
                  echo "═══════════════════════════════════════════"
                  echo "  svelte-check"
                  echo "═══════════════════════════════════════════"
                  npm run svelte-check 2>&1 || EXIT_CODE=$?

                  echo ""
                  echo "═══════════════════════════════════════════"
                  echo "  vitest"
                  echo "═══════════════════════════════════════════"
                  npm test 2>&1 || EXIT_CODE=$?

                  echo ""
                  if [ -n "''${EXIT_CODE:-}" ]; then
                    echo "✗ Some checks failed"
                    exit 1
                  else
                    echo "✓ All web checks passed"
                  fi
                '';
              }
            }/bin/web-check";

            # nix run .#generate-api-types  — generate TypeScript types from OpenAPI schema
            generate-api-types = mkApp "${
              pkgs.writeShellApplication {
                name = "generate-api-types";
                runtimeInputs = [
                  pkgs.nodejs_20
                  pkgs.curl
                ];
                text = ''
                  set -euo pipefail

                  WEB_DIR="${self}/web"
                  OPENAPI_URL="http://127.0.0.1:8020/api/openapi.json"
                  OUTPUT="$WEB_DIR/src/lib/api/types.ts"
                  TMP_JSON="$(mktemp).json"
                  trap 'rm -f "$TMP_JSON"' EXIT

                  echo "=== Fetching OpenAPI schema from $OPENAPI_URL ==="
                  if curl -sf "$OPENAPI_URL" > "$TMP_JSON"; then
                    echo "✓ OpenAPI schema fetched"
                  else
                    echo ""
                    echo "ERROR: Backend server is not running at $OPENAPI_URL" >&2
                    echo "" >&2
                    echo "Please start the backend server first:" >&2
                    echo "  nix run .#tauri-dev" >&2
                    echo "" >&2
                    echo "Or build and run the Rust server in web mode:" >&2
                    echo "  nix run .#default -- --ui web" >&2
                    echo "" >&2
                    exit 1
                  fi

                  echo ""
                  echo "=== Generating TypeScript types ==="
                  cd "$WEB_DIR"
                  npm ci --legacy-peer-deps 2>&1
                  npx openapi-typescript "$TMP_JSON" --output "$OUTPUT" 2>&1

                  echo ""
                  if [ -f "$OUTPUT" ]; then
                    LINES=$(wc -l < "$OUTPUT")
                    echo "✓ TypeScript types generated: $OUTPUT ($LINES lines)"
                  fi
                '';
              }
            }/bin/generate-api-types";

            # nix run .#setup-bwrap  — setup bubblewrap for non-NixOS systems
            # Required on Ubuntu 24.04+ where AppArmor blocks unprivileged user namespaces.
            # Run once with sudo: sudo nix run .#setup-bwrap
            setup-bwrap = mkApp "${
              pkgs.writeShellApplication {
                name = "setup-bwrap";
                runtimeInputs = [ pkgs.coreutils ];
                text = ''
                  set -euo pipefail

                  APPARMOR_DIR="/etc/apparmor.d"
                  PROFILE_FILE="$APPARMOR_DIR/bwrap"

                  echo "=== Bubblewrap Setup for Non-NixOS Systems ==="
                  echo ""

                  # Check if running as root
                  if [ "$EUID" -ne 0 ]; then
                    echo "ERROR: This script must be run as root (use sudo)" >&2
                    echo "  sudo nix run .#setup-bwrap" >&2
                    exit 1
                  fi

                  # Check if AppArmor is active
                  if ! command -v aa-status >/dev/null 2>&1; then
                    echo "AppArmor is not installed. No setup needed."
                    echo "You may need to install uidmap:"
                    echo "  sudo apt install uidmap"
                    exit 0
                  fi

                  # Check if profile already exists
                  if [ -f "$PROFILE_FILE" ]; then
                    echo "AppArmor profile for bwrap already exists:"
                    echo "  $PROFILE_FILE"
                    echo ""
                    echo "To recreate it, delete the file first:"
                    echo "  sudo rm $PROFILE_FILE"
                    exit 0
                  fi

                  # Create AppArmor profile
                  echo "Creating AppArmor profile for bwrap..."
                  mkdir -p "$APPARMOR_DIR"
                  cat > "$PROFILE_FILE" << 'PROFILE_EOF'
                  abi <abi/4.0>,
                  include <tunables/global>

                  profile bwrap /usr/bin/bwrap flags=(unconfined) {
                    userns,

                    # Site-specific additions and overrides. See local/README for details.
                    include if exists <local/bwrap>
                  }
                  PROFILE_EOF

                  # Remove leading whitespace from heredoc
                  sed -i 's/^[[:space:]]*//' "$PROFILE_FILE"

                  echo "Profile created: $PROFILE_FILE"
                  echo ""

                  # Load the profile
                  if command -v apparmor_parser >/dev/null 2>&1; then
                    echo "Loading AppArmor profile..."
                    apparmor_parser -r "$PROFILE_FILE"
                    echo "Profile loaded successfully."
                  else
                    echo "WARNING: apparmor_parser not found. Please restart AppArmor:"
                    echo "  sudo systemctl restart apparmor"
                  fi

                  # Disable AppArmor restriction on unprivileged userns (Ubuntu 24.04+)
                  echo ""
                  echo "Checking kernel.apparmor_restrict_unprivileged_userns..."
                  CURRENT_VAL=$(sysctl -n kernel.apparmor_restrict_unprivileged_userns 2>/dev/null || echo "unknown")
                  if [ "$CURRENT_VAL" = "1" ]; then
                    echo "Current value: 1 (restricted)"
                    echo "Disabling restriction..."
                    sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
                    echo "Restriction disabled."
                    echo ""
                    echo "NOTE: To make this change persistent across reboots, add to /etc/sysctl.conf:"
                    echo "  kernel.apparmor_restrict_unprivileged_userns=0"
                  elif [ "$CURRENT_VAL" = "0" ]; then
                    echo "Current value: 0 (already unrestricted)"
                  else
                    echo "Could not determine current value (got: $CURRENT_VAL)"
                  fi

                  echo ""
                  echo "=== Setup Complete ==="
                  echo ""
                  echo "You can now run: nix run .#check"
                  echo ""
                  echo "NOTE: If bwrap still fails, ensure uidmap is installed:"
                  echo "  sudo apt install uidmap"
                '';
              }
            }/bin/setup-bwrap";
          };

          # ── git-hooks (pre-commit) configuration ───────────────────────
          pre-commit = {
            check.enable = false; # skip in nix flake check (sandbox limitation)
            settings = {
              hooks = {
                treefmt = {
                  enable = true;
                  entry = "${config.treefmt.build.wrapper}/bin/treefmt";
                  pass_filenames = false;
                };
                typos = {
                  enable = true;
                  entry = "${pkgs.typos}/bin/typos";
                  pass_filenames = false;
                };
                "check-tauri-lock" = {
                  enable = true;
                  name = "Check src-tauri/Cargo.lock";
                  description = "Verify src-tauri/Cargo.lock exists and is tracked by git";
                  entry = "bash ${self}/scripts/check-tauri-lock.sh";
                  pass_filenames = false;
                  stages = [ "pre-commit" ];
                };
              };
            };
          };

          devShells.default = config.pre-commit.devShell.overrideAttrs (old: {
            name = "pokecon-devshell";

            nativeBuildInputs =
              old.nativeBuildInputs
              ++ (
                with pkgs;
                [
                  rustToolchain
                  cargo-expand
                  cargo-flamegraph
                  rust-analyzer
                  clippy
                  rustfmt

                  python314

                  # libclang is required to build v4l2-sys-mit (v4l dependency)
                  libclang

                  # libva for VAAPI hardware encoding (via FFmpeg)
                  libva
                  libva-utils
                ]
                ++ pythonPkgs
                ++ [

                  maturin

                  nodejs_20
                  pnpm
                  yarn

                  pkg-config
                  openssl
                  sqlite
                  curl
                  wget
                  git
                  just

                  webkitgtk_4_1
                  gtk3
                  gst_all_1.gstreamer
                  gst_all_1.gst-plugins-base
                  gst_all_1.gst-plugins-good
                  gst_all_1.gst-plugins-bad
                  gst_all_1.gst-plugins-ugly
                  gst_all_1.gst-libav

                  cargo-tauri

                  nix-tree
                  typos

                  # Test infrastructure: virtual V4L2 and serial devices
                  v4l-utils
                  ffmpeg-headless
                  socat
                ]
              );

            shellHook = ''
              export RUST_SRC_PATH="${pkgs.rustPlatform.rustLibSrc}"
              export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
              # LIBCLANG_PATH must be set for v4l2-sys-mit (bindgen)
              export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"

              ${config.pre-commit.installationScript}

              # Auto-setup node_modules from nix store if missing or outdated
              # This ensures reproducible npm deps without running npm install
              WEB_DIR="$PWD/web"
              if [ -f "$WEB_DIR/package-lock.json" ]; then
                LOCK_HASH=$(nix-hash --type sha256 --flat "$WEB_DIR/package-lock.json" 2>&1 | head -1)
                HASH_FILE="$WEB_DIR/.node_modules.hash"
                
                if [ ! -d "$WEB_DIR/node_modules" ] || [ ! -f "$HASH_FILE" ] || [ "$(cat "$HASH_FILE" 2>&1)" != "$LOCK_HASH" ]; then
                  echo "=== Setting up node_modules from nix store ==="
                  # Use npmHooks.npmConfigHook approach: copy from nix store
                  # The npm deps are already fetched by nix, we just need to link them
                  if [ -d "${self}/web/node_modules" ]; then
                    rm -rf "$WEB_DIR/node_modules"
                    cp -r "${self}/web/node_modules" "$WEB_DIR/node_modules"
                    chmod -R +w "$WEB_DIR/node_modules"
                    echo "$LOCK_HASH" > "$HASH_FILE"
                    echo "✓ node_modules synced from nix store"
                  else
                    echo "⚠ node_modules not in nix store. Run: nix run .#npm-update"
                  fi
                fi
              fi

              echo "Poke Controller Modified Extension development environment loaded."
              echo "Rust: $(rustc --version)"
              echo "Python: $(python3.14 --version 2>/dev/null || python3 --version)"
              echo "Node.js: $(node --version)"
              echo ""
              echo "Available task apps: nix run .#<task>"
              echo "  (default)       - run Tauri/Web UI (GUI auto-detection)"
              echo "  python-app      - launch Python entry point (legacy)"
              echo "  fmt               - format all files (treefmt)"
              echo "  clippy            - Rust linter"
              echo "  ruff-check        - Python linter"
              echo "  ruff-format       - format Python files"
              echo "  test              - run pytest"
              echo "  build             - build Rust + Python"
              echo "  build-rust        - build Rust workspace only"
              echo "  cargo-test        - run Rust tests"
              echo "  maturin-develop   - dev-install Python bindings"
              echo "  tauri-dev         - run Tauri dev server"
              echo "  typos             - run spell checker"
              echo "  typos-check       - check typos (CI)"
              echo "  check             - full CI gate"
              echo "  web-check         - run TS/JS checks (eslint + svelte-check + vitest)"
              echo "  generate-api-types - generate TS types from OpenAPI schema"
              echo ""
              echo "Test helpers:"
              echo "  scripts/setup-v4l2-test.sh   - create virtual V4L2 camera"
              echo "  scripts/setup-serial-test.sh  - create virtual serial port pair"
            '';
          });
        };
    };
}
