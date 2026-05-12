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
              npmDeps = pkgs.fetchNpmDeps {
                name = "${finalAttrs.pname}-${finalAttrs.version}-npm-deps";
                src = ./web;
                hash = "sha256-AfRizRYTwuQlzJdqFwdcStGDw1IBaceMD0tLi2WSK0E=";
              };

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
                  pkgs.libx11
                  pkgs.libxcursor
                  pkgs.libxrandr
                  pkgs.libxi
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
                  pkgs.libx11
                  pkgs.libxcursor
                  pkgs.libxrandr
                  pkgs.libxi
                ];
                text = ''
                  # Set PKG_CONFIG_PATH for all GTK/WebKit dependencies (zlib is in share/pkgconfig)
                  export PKG_CONFIG_PATH="${pkgs.glib.dev}/lib/pkgconfig:${pkgs.gtk3.dev}/lib/pkgconfig:${pkgs.pango.dev}/lib/pkgconfig:${pkgs.harfbuzz.dev}/lib/pkgconfig:${pkgs.cairo.dev}/lib/pkgconfig:${pkgs.atk.dev}/lib/pkgconfig:${pkgs.gdk-pixbuf.dev}/lib/pkgconfig:${pkgs.libsoup_3.dev}/lib/pkgconfig:${pkgs.webkitgtk_4_1.dev}/lib/pkgconfig:${pkgs.zlib.dev}/share/pkgconfig:${pkgs.dbus.dev}/lib/pkgconfig:${pkgs.libx11.dev}/lib/pkgconfig:${pkgs.libxcursor.dev}/lib/pkgconfig:${pkgs.libxrandr.dev}/lib/pkgconfig:${pkgs.libxi.dev}/lib/pkgconfig''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

                  cd "${self}/src-tauri"
                  cargo tauri dev
                '';
              }
            }/bin/tauri-dev";

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
                  echo "=== Computing new npm deps hash ==="
                  # Use nix-prefetch-url to compute the hash of npm deps
                  NEW_HASH=$(nix-prefetch-url --unpack "file://$WEB_DIR/package-lock.json" 2>&1 | tail -1 || true)

                  if [ -z "$NEW_HASH" ] || [ "$NEW_HASH" = "" ]; then
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
                  echo "  typos (spell check)"
                  echo "═══════════════════════════════════════════"
                  ${pkgs.typos}/bin/typos
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
              echo ""
              echo "Test helpers:"
              echo "  scripts/setup-v4l2-test.sh   - create virtual V4L2 camera"
              echo "  scripts/setup-serial-test.sh  - create virtual serial port pair"
            '';
          });
        };
    };
}
