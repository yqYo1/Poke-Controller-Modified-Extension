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
  };

  outputs =
    inputs@{
      self,
      flake-parts,
      systems,
      treefmt-nix,
      rust-overlay,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = import systems;

      imports = [
        treefmt-nix.flakeModule
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

          # ── Poke-Controller application package ────────────────────────
          pokeconApp = pkgs.writeShellApplication {
            name = "pokecon";
            runtimeInputs = [
              rustEnv
              pythonEnv
              pkgs.maturin
            ];
            text = ''
                            # Run from current directory (writable) instead of nix store
                            workdir="$PWD"
                            # Ensure Python bindings are built
                            if [ ! -f "$workdir/python/pokecon/pokecon"*.so ] && [ ! -f "$workdir/python/pokecon/pokecon"*.pyd ]; then
                              echo "Building Python bindings..."
                              maturin build --release \
                                --manifest-path "$workdir/rust/pokecon-pybindings/Cargo.toml" \
                                --out /tmp/pokecon-wheels 2>&1
                              wheel=$(ls /tmp/pokecon-wheels/pokecon-*.whl 2>/dev/null | head -1)
                              if [ -n "$wheel" ]; then
                                mkdir -p /tmp/pokecon-extracted
                                python -c "
              import zipfile, sys
              with zipfile.ZipFile('$wheel', 'r') as z:
                  z.extractall('/tmp/pokecon-extracted')
              "
                                cp /tmp/pokecon-extracted/pokecon*.so "$workdir/python/pokecon/" 2>/dev/null || true
                              fi
                            fi
                            # Launch the application
                            PYTHONPATH="$workdir/python:$PYTHONPATH"
                            export PYTHONPATH
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
                  options = [ "--config=pyproject.toml" ];
                };
                ruff-check = {
                  includes = [ "*.py" ];
                  options = [
                    "--select"
                    "E,W,F,I"
                    "--ignore"
                    "E402,E501,E722,E741,F821,F841"
                  ];
                };
              };
            };
          };

          apps = {
            # nix run .  — launch Poke-Controller application
            default = mkApp "${pokeconApp}/bin/pokecon";
            fmt = mkApp "${config.treefmt.build.wrapper}/bin/treefmt";

            # nix run .#clippy  — run Rust linter
            clippy = mkApp "${
              pkgs.writeShellApplication {
                name = "clippy";
                runtimeInputs = [ rustEnv ];
                text = ''
                  workdir="$(mktemp -d)"
                  trap 'rm -rf "$workdir"' EXIT
                  cp -r "${self}/." "$workdir/"
                  chmod -R +w "$workdir"
                  cd "$workdir"
                  cargo clippy --all-targets --all-features -- -D warnings
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
                  workdir="$(mktemp -d)"
                  trap 'rm -rf "$workdir"' EXIT
                  cp -r "${self}/." "$workdir/"
                  chmod -R +w "$workdir"
                  cd "$workdir"
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
                  workdir="$(mktemp -d)"
                  trap 'rm -rf "$workdir"' EXIT
                  cp -r "${self}/." "$workdir/"
                  chmod -R +w "$workdir"
                  cd "$workdir"
                  cargo build --workspace --all-features
                '';
              }
            }/bin/build-rust";

            # nix run .#cargo-test  — run Rust tests
            cargo-test = mkApp "${
              pkgs.writeShellApplication {
                name = "cargo-test";
                runtimeInputs = [ rustEnv ];
                text = ''
                  workdir="$(mktemp -d)"
                  trap 'rm -rf "$workdir"' EXIT
                  cp -r "${self}/." "$workdir/"
                  chmod -R +w "$workdir"
                  cd "$workdir"
                  cargo test --workspace --all-features
                '';
              }
            }/bin/cargo-test";

            # nix run .#maturin-develop  — build + install Python package in dev mode
            maturin-develop = mkApp "${
              pkgs.writeShellApplication {
                name = "maturin-develop";
                runtimeInputs = [
                  rustEnv
                  pythonEnv
                  pkgs.maturin
                ];
                text = ''
                                    cd "${self}"
                                    echo "=== Building Python bindings in-place ==="
                                    maturin build --release \
                                      --manifest-path rust/pokecon-pybindings/Cargo.toml \
                                      --out /tmp/pokecon-wheels 2>&1
                                    wheel=$(ls /tmp/pokecon-wheels/pokecon-*.whl 2>/dev/null | head -1)
                                    if [ -n "$wheel" ]; then
                                      mkdir -p /tmp/pokecon-extracted
                                      python -c "
                  import zipfile
                  with zipfile.ZipFile('$wheel', 'r') as z:
                      z.extractall('/tmp/pokecon-extracted')
                  "
                                      mkdir -p python/pokecon
                                      cp /tmp/pokecon-extracted/pokecon*.so python/pokecon/ 2>/dev/null || true
                                      echo "✓ Python bindings built and installed to python/pokecon/"
                                    else
                                      echo "Error: No wheel was built" >&2
                                      exit 1
                                    fi
                '';
              }
            }/bin/maturin-develop";

            # nix run .#tauri-build  — build Tauri app using nix's buildRustPackage
            tauri-build = mkApp "${
              (buildRustPackage {
                pname = "pokecon-tauri";
                version = "0.1.0";
                src = self;

                cargoLock = {
                  lockFile = self + "/Cargo.lock";
                  allowBuiltinFetchGit = true;
                };

                buildAndTestSubdir = "src-tauri";

                nativeBuildInputs = [
                  pkgs.pkg-config
                  pkgs.wrapGAppsHook4
                ];

                buildInputs = [
                  pkgs.glib
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
                  pkgs.xorg.libX11
                  pkgs.xorg.libXcursor
                  pkgs.xorg.libXrandr
                  pkgs.xorg.libXi
                ];

                # Tauri requires icons during build
                preBuild = ''
                  mkdir -p src-tauri/icons
                  # Generate minimal valid PNG: 1x1 transparent pixel
                  printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\nIDATx\x9cc\x60\x00\x00\x00\x02\x00\x01\xe2!\xbc\x33\x00\x00\x00\x00IEND\xaeB`\x82' > src-tauri/icons/icon.png
                  cp src-tauri/icons/icon.png src-tauri/icons/32x32.png
                  cp src-tauri/icons/icon.png src-tauri/icons/128x128.png
                  cp src-tauri/icons/icon.png src-tauri/icons/128x128@2x.png

                  # Skip tauri-build's runtime validation in nix sandbox
                  export TAURI_SKIP_BUILD=1
                '';

                # Skip tests — Tauri app requires display/GTK which is not available in nix build sandbox
                doCheck = false;

                # Override buildPhase to skip tauri-build's default behavior
                buildPhase = ''
                  # Create placeholder icons (Tauri requires them during build)
                  mkdir -p src-tauri/icons
                  printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\nIDATx\x9cc\x60\x00\x00\x00\x02\x00\x01\xe2!\xbc\x33\x00\x00\x00\x00IEND\xaeB`\x82' > src-tauri/icons/icon.png
                  cp src-tauri/icons/icon.png src-tauri/icons/32x32.png
                  cp src-tauri/icons/icon.png src-tauri/icons/128x128.png
                  cp src-tauri/icons/icon.png src-tauri/icons/128x128@2x.png

                  export TAURI_SKIP_BUILD=1
                  cd src-tauri
                  cargo build --release --offline
                '';

                # Override checkPhase to prevent cargoCheckHook from running tests
                checkPhase = "true";
                cargoCheckFlags = "--no-run";

                # Disable cargoCheckHook completely
                dontUseCargoCheckHook = true;

                # Disable cargoBuildHook's test execution
                cargoBuildFlags = [
                  "--release"
                  "--offline"
                ];

                installPhase = ''
                  mkdir -p $out/bin
                  find . -name "pokecon-tauri" -type f -executable -print0 | head -z -n 1 | xargs -0 -I {} cp {} $out/bin/
                '';

                meta = {
                  description = "Poke-Controller Modified Extension Tauri UI";
                  license = pkgs.lib.licenses.mit;
                };
              })
            }/bin/pokecon-tauri";

            # nix run .#tauri-dev  — run Tauri dev server
            tauri-dev = mkApp "${
              pkgs.writeShellApplication {
                name = "tauri-dev";
                runtimeInputs = [
                  rustEnv
                  pkgs.pkg-config
                  pkgs.glib
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
                  pkgs.xorg.libX11
                  pkgs.xorg.libXcursor
                  pkgs.xorg.libXrandr
                  pkgs.xorg.libXi
                ];
                text = ''
                  # Set PKG_CONFIG_PATH for all GTK/WebKit dependencies (zlib is in share/pkgconfig)
                  export PKG_CONFIG_PATH="${pkgs.glib.dev}/lib/pkgconfig:${pkgs.gtk3.dev}/lib/pkgconfig:${pkgs.pango.dev}/lib/pkgconfig:${pkgs.harfbuzz.dev}/lib/pkgconfig:${pkgs.cairo.dev}/lib/pkgconfig:${pkgs.atk.dev}/lib/pkgconfig:${pkgs.gdk-pixbuf.dev}/lib/pkgconfig:${pkgs.libsoup_3.dev}/lib/pkgconfig:${pkgs.webkitgtk_4_1.dev}/lib/pkgconfig:${pkgs.zlib.dev}/share/pkgconfig:${pkgs.dbus.dev}/lib/pkgconfig:${pkgs.xorg.libX11.dev}/lib/pkgconfig:${pkgs.xorg.libXcursor.dev}/lib/pkgconfig:${pkgs.xorg.libXrandr.dev}/lib/pkgconfig:${pkgs.xorg.libXi.dev}/lib/pkgconfig''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

                  cd "${self}/src-tauri"
                  cargo tauri dev
                '';
              }
            }/bin/tauri-dev";

            # nix run .#check  — run ALL checks (CI gate)
            check = mkApp "${
              pkgs.writeShellApplication {
                name = "check";
                runtimeInputs = [
                  rustEnv
                  pythonEnv
                ];
                text = ''
                  workdir="$(mktemp -d)"
                  trap 'rm -rf "$workdir"' EXIT
                  cp -r "${self}/." "$workdir/"
                  chmod -R +w "$workdir"
                  cd "$workdir"

                  echo "═══════════════════════════════════════════"
                  echo "  clippy"
                  echo "═══════════════════════════════════════════"
                  cargo clippy --all-targets --all-features -- -D warnings
                  echo ""
                  echo "═══════════════════════════════════════════"
                  echo "  ruff check"
                  echo "═══════════════════════════════════════════"
                  export PYTHONPATH="$workdir/python''${PYTHONPATH:+:$PYTHONPATH}"
                  ruff check --no-cache --select E,W,F --ignore E402,E501,E722,E741,F821,F841 .
                  echo ""
                  echo "═══════════════════════════════════════════"
                  echo "  pytest"
                  echo "═══════════════════════════════════════════"
                  export PYTHONPATH="$workdir/python''${PYTHONPATH:+:$PYTHONPATH}"
                  pytest -p no:cacheprovider tests/ -v --tb=short
                  echo ""
                  echo "═══════════════════════════════════════════"
                  echo "  formatting (check mode)"
                  echo "═══════════════════════════════════════════"
                  ${config.treefmt.build.wrapper}/bin/treefmt --ci
                  echo ""
                  echo "✓ All checks passed"
                '';
              }
            }/bin/check";
          };

          devShells.default = pkgs.mkShell {
            name = "pokecon-devshell";

            packages =
              with pkgs;
              [
                rustToolchain
                cargo-expand
                cargo-flamegraph
                rust-analyzer
                clippy
                rustfmt

                pkgs.python314
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
              ];

            shellHook = ''
              export RUST_SRC_PATH="${pkgs.rustPlatform.rustLibSrc}"
              export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
              export LD_LIBRARY_PATH="${
                pkgs.lib.makeLibraryPath [
                  pkgs.openssl
                  pkgs.gtk3
                  pkgs.webkitgtk_4_1
                  pkgs.curl
                ]
              }:$LD_LIBRARY_PATH"

              echo "Poke Controller Modified Extension development environment loaded."
              echo "Rust: $(rustc --version)"
              echo "Python: $(python3.14 --version 2>/dev/null || python3 --version)"
              echo "Node.js: $(node --version)"
              echo ""
              echo "Available task apps: nix run .#<task>"
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
              echo "  check             - full CI gate"
            '';
          };
        };
    };
}
