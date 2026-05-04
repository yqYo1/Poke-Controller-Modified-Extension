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
              pkgs.uv
            ];
            text = ''
              cd "${self}"
              # Ensure Python bindings are built
              if [ ! -f "python/pokecon/pokecon*.so" ] && [ ! -f "python/pokecon/pokecon*.pyd" ]; then
                echo "Building Python bindings..."
                uv run maturin develop --uv --manifest-path rust/pokecon-pybindings/Cargo.toml
              fi
              # Launch the application
              PYTHONPATH="${self}/python:$PYTHONPATH"
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
            settings.formatter = {
              ruff-format.options = [ "--config=pyproject.toml" ];
              ruff-check.options = [
                "--select"
                "E,W,F,I"
                "--ignore"
                "E402,E501,E722,E741,F821,F841"
              ];
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
                  pkgs.uv
                ];
                text = ''
                  cd "${self}"
                  echo "=== Building Rust workspace ==="
                  cargo build --workspace --all-features
                  echo ""
                  echo "=== Building Python maturin package ==="
                  uv run maturin build --manifest-path rust/pokecon-pybindings/Cargo.toml
                '';
              }
            }/bin/build";

            # nix run .#build-rust  — build Rust workspace only
            build-rust = mkApp "${
              pkgs.writeShellApplication {
                name = "build-rust";
                runtimeInputs = [ rustEnv ];
                text = ''
                  cd "${self}"
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
                  cd "${self}"
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
                  pkgs.uv
                ];
                text = ''
                  cd "${self}"
                  uv run maturin develop --uv --manifest-path rust/pokecon-pybindings/Cargo.toml
                '';
              }
            }/bin/maturin-develop";

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

                uv

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
              echo "  check             - full CI gate"
            '';
          };
        };
    };
}
