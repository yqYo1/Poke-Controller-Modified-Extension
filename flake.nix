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
      flake-parts,
      git-hooks,
      rust-overlay,
      systems,
      treefmt-nix,
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
          self',
          system,
          ...
        }:
        let
          lib = pkgs.lib;
          pkgsWithOverlays = import inputs.nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rustToolchain = pkgsWithOverlays.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
          pythonEnv = pkgs.python314.withPackages (
            pythonPackages: with pythonPackages; [
              pytest
              ruff
            ]
          );
          workspaceManifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          workspaceVersion = workspaceManifest.workspace.package.version;

          source = builtins.path {
            path = inputs.self.outPath;
            name = "pokecon-source";
            filter =
              path: type:
              let
                sourcePath = toString path;
              in
              type == "directory"
              || lib.hasSuffix "/.gitignore" sourcePath
              || lib.hasSuffix ".rs" sourcePath
              || lib.hasSuffix ".toml" sourcePath
              || lib.hasSuffix ".lock" sourcePath
              || lib.hasSuffix ".json" sourcePath
              || lib.hasSuffix ".jsonl" sourcePath
              || lib.hasSuffix ".py" sourcePath
              || lib.hasSuffix ".pyi" sourcePath
              || lib.hasSuffix ".sh" sourcePath
              || lib.hasSuffix ".nix" sourcePath
              || lib.hasSuffix ".md" sourcePath
              || lib.hasSuffix ".txt" sourcePath
              || lib.hasSuffix ".yml" sourcePath
              || lib.hasSuffix ".yaml" sourcePath
              || lib.hasSuffix ".html" sourcePath
              || lib.hasSuffix ".css" sourcePath
              || lib.hasSuffix ".svelte" sourcePath
              || lib.hasSuffix ".js" sourcePath
              || lib.hasSuffix ".jsx" sourcePath
              || lib.hasSuffix ".ts" sourcePath
              || lib.hasSuffix ".tsx" sourcePath
              || lib.hasSuffix ".lua" sourcePath
              || lib.hasSuffix ".svg" sourcePath
              || lib.hasSuffix ".png" sourcePath
              || lib.hasSuffix ".ico" sourcePath
              || lib.hasSuffix ".woff" sourcePath
              || lib.hasSuffix ".woff2" sourcePath
              || lib.hasSuffix ".ttf" sourcePath
              || lib.hasSuffix ".eot" sourcePath;
          };

          mkApp = program: {
            type = "app";
            inherit program;
            meta.description = "PokeCon application or reproducible development task";
          };
          mkTask =
            {
              name,
              runtimeInputs ? [ ],
              text,
            }:
            mkApp "${
              pkgs.writeShellApplication {
                inherit name runtimeInputs text;
              }
            }/bin/${name}";

          setupWorkdir = ''
            export POKECON_BUILD_UV_PATH="${pkgs.uv}/bin/uv"
            export POKECON_BUILD_UV_VERSION="${pkgs.uv.version}"
            export PYO3_PYTHON="${pythonEnv}/bin/python"
            caller_dir="$PWD"
            if [ -w "$caller_dir" ]; then
              export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-$caller_dir/target/nix-tasks}"
            else
              export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-/tmp/pokecon-nix-tasks}"
            fi
            mkdir -p "$CARGO_TARGET_DIR"
            workdir="$(mktemp -d)"
            trap 'rm -rf "$workdir"' EXIT
            cp -R "${source}/." "$workdir/"
            chmod -R u+w "$workdir"
            cd "$workdir"
          '';

          linuxDesktopPackages = lib.optionals pkgs.stdenv.isLinux [
            pkgs.atk
            pkgs.cairo
            pkgs.gdk-pixbuf
            pkgs.glib
            pkgs.gtk3
            pkgs.harfbuzz
            pkgs.libsoup_3
            pkgs.pango
            pkgs.portaudio
            pkgs.udev
            pkgs.webkitgtk_4_1
            pkgs.zlib
          ];
          rustTaskInputs = [
            rustToolchain
            pythonEnv
            pkgs.pkg-config
            pkgs.stdenv.cc
          ]
          ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.llvmPackages.libclang ]
          ++ linuxDesktopPackages;
          desktopEnvironment = lib.optionalString pkgs.stdenv.isLinux ''
            export PKG_CONFIG_PATH="${pkgs.glib.dev}/lib/pkgconfig:${pkgs.gtk3.dev}/lib/pkgconfig:${pkgs.pango.dev}/lib/pkgconfig:${pkgs.harfbuzz.dev}/lib/pkgconfig:${pkgs.cairo.dev}/lib/pkgconfig:${pkgs.atk.dev}/lib/pkgconfig:${pkgs.gdk-pixbuf.dev}/lib/pkgconfig:${pkgs.libsoup_3.dev}/lib/pkgconfig:${pkgs.webkitgtk_4_1.dev}/lib/pkgconfig:${pkgs.udev.dev}/lib/pkgconfig:${pkgs.zlib.dev}/share/pkgconfig"
            export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib"
          '';

          pokeconPackage = rustPlatform.buildRustPackage {
            pname = "pokecon";
            version = workspaceVersion;
            src = source;
            cargoLock = {
              lockFile = ./Cargo.lock;
              allowBuiltinFetchGit = true;
            };
            cargoBuildFlags = [
              "--package"
              "pokecon-app"
            ];
            cargoTestFlags = [
              "--package"
              "pokecon-app"
            ];
            doCheck = true;
            POKECON_BUILD_UV_PATH = "${pkgs.uv}/bin/uv";
            POKECON_BUILD_UV_VERSION = pkgs.uv.version;
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
                ".venv/**"
                "__pycache__/**"
                "dist/**"
                "node_modules/**"
                "result*"
                "target/**"
              ];
              formatter = {
                rustfmt.includes = [ "*.rs" ];
                ruff-check = {
                  includes = [
                    "*.py"
                    "*.pyi"
                  ];
                  options = [
                    "--config=ruff.toml"
                    "--no-cache"
                  ];
                };
                ruff-format = {
                  includes = [
                    "*.py"
                    "*.pyi"
                  ];
                  options = [
                    "--config=ruff.toml"
                    "--no-cache"
                  ];
                };
              };
            };
          };

          packages = {
            default = pokeconPackage;
            pokecon = pokeconPackage;
          };

          checks.pokecon = pokeconPackage;

          apps = {
            default = mkApp "${self'.packages.pokecon}/bin/pokecon";
            fmt = mkApp "${config.treefmt.build.wrapper}/bin/treefmt";

            clippy = mkTask {
              name = "clippy";
              runtimeInputs = rustTaskInputs;
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
              '';
            };

            build-rust = mkTask {
              name = "build-rust";
              runtimeInputs = rustTaskInputs;
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                cargo build --locked --workspace --all-features
              '';
            };

            cargo-test = mkTask {
              name = "cargo-test";
              runtimeInputs = rustTaskInputs;
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                cargo test --locked --workspace --all-features
              '';
            };

            build = mkTask {
              name = "build";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.maturin
              ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                export PYO3_PYTHON="${pythonEnv}/bin/python"
                cargo build --locked --workspace --all-features
                maturin build --locked --release --manifest-path rust/pokecon-pybindings/Cargo.toml --out dist
              '';
            };

            contract-check = mkTask {
              name = "contract-check";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.basedpyright
                pkgs.shellcheck
              ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD/python:$PWD''${PYTHONPATH:+:$PYTHONPATH}"
                cargo run --locked --package pokecon-contracts --bin generate_contracts -- --check
                cargo test --locked --package pokecon-contracts --test contract_sync
                basedpyright
                shellcheck scripts/*.sh
                python scripts/source_filter.py
              '';
            };

            generate-contracts = mkTask {
              name = "generate-contracts";
              runtimeInputs = rustTaskInputs;
              text = ''
                export PYO3_PYTHON="${pythonEnv}/bin/python"
                exec cargo run --locked --package pokecon-contracts --bin generate_contracts -- "$@"
              '';
            };

            compatibility-inventory = mkTask {
              name = "compatibility-inventory";
              runtimeInputs = [
                pythonEnv
                pkgs.git
              ];
              text = ''
                cd "${source}"
                exec python scripts/compatibility_inventory.py --check "$@"
              '';
            };

            source-guard = mkTask {
              name = "source-guard";
              runtimeInputs = [ pythonEnv ];
              text = ''
                cd "${source}"
                exec python scripts/source_guard.py "$@"
              '';
            };

            source-filter-check = mkTask {
              name = "source-filter-check";
              runtimeInputs = [ pythonEnv ];
              text = ''
                cd "${source}"
                exec python scripts/source_filter.py "$@"
              '';
            };

            ruff-check = mkTask {
              name = "ruff-check";
              runtimeInputs = [
                pythonEnv
                pkgs.ripgrep
              ];
              text = ''
                cd "${source}"
                python_files=("$@")
                if [ "''${#python_files[@]}" -eq 0 ]; then
                  mapfile -t python_files < <(rg --files python scripts tests -g '*.py' -g '*.pyi')
                fi
                exec ruff check --config ruff.toml --no-cache "''${python_files[@]}"
              '';
            };

            ruff-format = mkTask {
              name = "ruff-format";
              runtimeInputs = [
                pythonEnv
                pkgs.ripgrep
              ];
              text = ''
                python_files=("$@")
                if [ "''${#python_files[@]}" -eq 0 ]; then
                  mapfile -t python_files < <(rg --files python scripts tests -g '*.py' -g '*.pyi')
                fi
                exec ruff format --config ruff.toml "''${python_files[@]}"
              '';
            };

            ruff-format-check = mkTask {
              name = "ruff-format-check";
              runtimeInputs = [
                pythonEnv
                pkgs.ripgrep
              ];
              text = ''
                cd "${source}"
                python_files=("$@")
                if [ "''${#python_files[@]}" -eq 0 ]; then
                  mapfile -t python_files < <(rg --files python scripts tests -g '*.py' -g '*.pyi')
                fi
                exec ruff format --config ruff.toml --no-cache --check "''${python_files[@]}"
              '';
            };

            basedpyright = mkTask {
              name = "basedpyright";
              runtimeInputs = [
                pythonEnv
                pkgs.basedpyright
              ];
              text = ''
                cd "${source}"
                export PYTHONDONTWRITEBYTECODE=1
                exec basedpyright
              '';
            };

            test = mkTask {
              name = "test";
              runtimeInputs = [ pythonEnv ];
              text = ''
                cd "${source}"
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="${source}/python:${source}''${PYTHONPATH:+:$PYTHONPATH}"
                exec python -m pytest -p no:cacheprovider tests -v --tb=short
              '';
            };

            maturin-develop = mkTask {
              name = "maturin-develop";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.maturin
              ];
              text = ''
                ${desktopEnvironment}
                export PYO3_PYTHON="${pythonEnv}/bin/python"
                exec maturin develop --locked --manifest-path rust/pokecon-pybindings/Cargo.toml "$@"
              '';
            };

            tauri-build = mkTask {
              name = "tauri-build";
              runtimeInputs = rustTaskInputs;
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                cargo build --locked --package pokecon-desktop --features tauri-shell
              '';
            };

            tauri = mkTask {
              name = "tauri";
              text = ''
                exec "${self'.packages.pokecon}/bin/pokecon" --ui desktop "$@"
              '';
            };

            typos = mkTask {
              name = "typos";
              runtimeInputs = [ pkgs.typos ];
              text = ''
                cd "${source}"
                exec typos "$@"
              '';
            };

            typos-check = mkTask {
              name = "typos-check";
              runtimeInputs = [ pkgs.typos ];
              text = ''
                cd "${source}"
                exec typos --no-exit-code "$@"
              '';
            };

            markdownlint = mkTask {
              name = "markdownlint";
              runtimeInputs = [
                pkgs.markdownlint-cli
                pkgs.ripgrep
              ];
              text = ''
                cd "${source}"
                markdown_files=("$@")
                if [ "''${#markdown_files[@]}" -eq 0 ]; then
                  mapfile -t markdown_files < <(rg --files -g '*.md')
                fi
                exec markdownlint --config .markdownlint.json "''${markdown_files[@]}"
              '';
            };

            markdownlint-check = mkTask {
              name = "markdownlint-check";
              runtimeInputs = [
                pkgs.markdownlint-cli
                pkgs.ripgrep
              ];
              text = ''
                cd "${source}"
                markdown_files=("$@")
                if [ "''${#markdown_files[@]}" -eq 0 ]; then
                  mapfile -t markdown_files < <(rg --files -g '*.md')
                fi
                exec markdownlint --config .markdownlint.json "''${markdown_files[@]}"
              '';
            };

            textlint = mkTask {
              name = "textlint";
              runtimeInputs = [
                pkgs.ripgrep
                pkgs.textlint
                pkgs.textlint-rule-no-start-duplicated-conjunction
              ];
              text = ''
                cd "${source}"
                export NODE_PATH="${pkgs.textlint-rule-no-start-duplicated-conjunction}/lib/node_modules''${NODE_PATH:+:$NODE_PATH}"
                text_files=("$@")
                if [ "''${#text_files[@]}" -eq 0 ]; then
                  mapfile -t text_files < <(rg --files -g '*.md' -g '*.txt')
                fi
                exec textlint --config .textlintrc.json "''${text_files[@]}"
              '';
            };

            textlint-check = mkTask {
              name = "textlint-check";
              runtimeInputs = [
                pkgs.ripgrep
                pkgs.textlint
                pkgs.textlint-rule-no-start-duplicated-conjunction
              ];
              text = ''
                cd "${source}"
                export NODE_PATH="${pkgs.textlint-rule-no-start-duplicated-conjunction}/lib/node_modules''${NODE_PATH:+:$NODE_PATH}"
                text_files=("$@")
                if [ "''${#text_files[@]}" -eq 0 ]; then
                  mapfile -t text_files < <(rg --files -g '*.md' -g '*.txt')
                fi
                exec textlint --config .textlintrc.json "''${text_files[@]}"
              '';
            };

            web-check = mkTask {
              name = "web-check";
              runtimeInputs = [
                pythonEnv
                pkgs.nodejs_20
              ];
              text = ''
                cd "${source}"
                if python scripts/source_guard.py web --require-applicable; then
                  workdir="$(mktemp -d)"
                  trap 'rm -rf "$workdir"' EXIT
                  cp -R web/. "$workdir/"
                  chmod -R u+w "$workdir"
                  cd "$workdir"
                  npm ci
                  npm run lint
                  npm run svelte-check
                  npm test
                else
                  guard_status=$?
                  if [ "$guard_status" -eq 3 ]; then
                    echo "web-check not applicable: the phase-eleven Web package is absent"
                    exit 0
                  fi
                  exit "$guard_status"
                fi
              '';
            };

            generate-api-types = mkTask {
              name = "generate-api-types";
              text = ''
                echo "generate-api-types not applicable: OpenAPI and Web sources arrive in phases ten and eleven"
              '';
            };

            check = mkTask {
              name = "check";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.basedpyright
                pkgs.markdownlint-cli
                pkgs.ripgrep
                pkgs.shellcheck
                pkgs.textlint
                pkgs.textlint-rule-no-start-duplicated-conjunction
                pkgs.typos
              ];
              text = ''
                if [ "''${1:-}" = "--help" ]; then
                  echo "Run all currently applicable PokeCon verification gates"
                  exit 0
                fi
                ${setupWorkdir}
                ${desktopEnvironment}
                export NODE_PATH="${pkgs.textlint-rule-no-start-duplicated-conjunction}/lib/node_modules''${NODE_PATH:+:$NODE_PATH}"
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD/python:$PWD''${PYTHONPATH:+:$PYTHONPATH}"
                python scripts/source_filter.py
                cargo run --locked --package pokecon-contracts --bin generate_contracts -- --check
                cargo test --locked --package pokecon-contracts --test contract_sync
                cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
                cargo test --locked --workspace --all-features
                cargo build --locked --workspace --all-features
                ruff check --config ruff.toml --no-cache python scripts tests
                ruff format --config ruff.toml --no-cache --check python scripts tests
                basedpyright
                python -m pytest -p no:cacheprovider tests -v --tb=short
                shellcheck scripts/*.sh
                markdownlint --config .markdownlint.json ./*.md
                textlint --config .textlintrc.json ./*.md ./*.txt
                typos
                ${config.treefmt.build.wrapper}/bin/treefmt --ci --working-dir "$PWD"
              '';
            };
          };

          pre-commit = {
            check.enable = false;
            inherit pkgs;
            settings = {
              configPath = ".pre-commit-config.yaml";
              hooks = {
                check-workspace-lock = {
                  enable = true;
                  name = "Check root Cargo.lock";
                  entry = "bash ${source}/scripts/check-workspace-lock.sh";
                  pass_filenames = false;
                };
                clippy = {
                  enable = true;
                  entry = "nix run .#clippy";
                  pass_filenames = false;
                };
                markdownlint = {
                  enable = true;
                  entry = "nix run .#markdownlint-check --";
                  files = "\\.md$";
                };
                ruff-check = {
                  enable = true;
                  entry = "nix run .#ruff-check --";
                  files = "\\.pyi?$";
                };
                ruff-format = {
                  enable = true;
                  entry = "nix run .#ruff-format --";
                  files = "\\.pyi?$";
                };
                textlint = {
                  enable = true;
                  entry = "nix run .#textlint-check --";
                  files = "\\.(md|txt)$";
                };
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
              };
            };
          };

          devShells.default = pkgs.mkShell {
            packages = rustTaskInputs ++ [
              config.treefmt.build.wrapper
              pkgs.basedpyright
              pkgs.cargo-tauri
              pkgs.curl
              pkgs.gh
              pkgs.git
              pkgs.jq
              pkgs.markdownlint-cli
              pkgs.maturin
              pkgs.nodejs_20
              pkgs.ripgrep
              pkgs.shellcheck
              pkgs.textlint
              pkgs.textlint-rule-no-start-duplicated-conjunction
              pkgs.typos
              pkgs.uv
            ];
            PYO3_PYTHON = "${pythonEnv}/bin/python";
            POKECON_BUILD_UV_PATH = "${pkgs.uv}/bin/uv";
            POKECON_BUILD_UV_VERSION = pkgs.uv.version;
            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            LIBCLANG_PATH = lib.optionalString pkgs.stdenv.isLinux "${pkgs.llvmPackages.libclang.lib}/lib";
            shellHook = config.pre-commit.shellHook + ''
              echo "PokeCon Nix development shell: Rust $(rustc --version), Python $(python --version)"
            '';
          };
        };
    };
}
