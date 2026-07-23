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
              icecream
              loguru
              numpy
              opencv4
              pyaudio
              pytest
              ruff
              scipy
            ]
          );
          workspaceManifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          workspaceVersion = workspaceManifest.workspace.package.version;
          portableUvVersion = "0.11.8";
          portableUv =
            if system == "x86_64-linux" then
              pkgs.fetchzip {
                url = "https://github.com/astral-sh/uv/releases/download/${portableUvVersion}/uv-x86_64-unknown-linux-gnu.tar.gz";
                hash = "sha256-LWnCnwmLdeJIV4ytqFqWwwFPlTs+FlODuPJSTgTFngY=";
              }
            else
              pkgs.uv;
          portableUvBinary = if system == "x86_64-linux" then "${portableUv}/uv" else "${pkgs.uv}/bin/uv";

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
              || lib.hasSuffix ".ps1" sourcePath
              || lib.hasSuffix ".rules" sourcePath
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
              || lib.hasSuffix ".icns" sourcePath
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
            export POKECON_INTERNAL_SCRIPT_SITE_PACKAGES="${pythonEnv}/${pkgs.python314.sitePackages}"
            export PYO3_PYTHON="${pythonEnv}/bin/python"
            export POKECON_BUILD_PYTHON="${pythonEnv}/bin/python"
            caller_dir="$PWD"
            if [ -w "$caller_dir" ]; then
              export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-$caller_dir/target/nix-tasks}"
            else
              export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-/tmp/pokecon-nix-tasks}"
            fi
            mkdir -p "$CARGO_TARGET_DIR/debug/uv" "$CARGO_TARGET_DIR/release/uv"
            ln -sfn "${pkgs.uv}/bin/uv" "$CARGO_TARGET_DIR/debug/uv/uv"
            ln -sfn "${pkgs.uv}/bin/uv" "$CARGO_TARGET_DIR/release/uv/uv"
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
            pkgs.linuxHeaders
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
            pkgs.nasm
            pkgs.stdenv.cc
          ]
          ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.llvmPackages.libclang ]
          ++ linuxDesktopPackages;
          linuxBindgenArgs = lib.optionalString pkgs.stdenv.isLinux "-I${pkgs.stdenv.cc.libc.dev}/include -I${pkgs.linuxHeaders}/include";
          desktopEnvironment = lib.optionalString pkgs.stdenv.isLinux ''
            export BINDGEN_EXTRA_CLANG_ARGS="${linuxBindgenArgs}"
            export PKG_CONFIG_PATH="${pkgs.glib.dev}/lib/pkgconfig:${pkgs.gtk3.dev}/lib/pkgconfig:${pkgs.pango.dev}/lib/pkgconfig:${pkgs.harfbuzz.dev}/lib/pkgconfig:${pkgs.cairo.dev}/lib/pkgconfig:${pkgs.atk.dev}/lib/pkgconfig:${pkgs.gdk-pixbuf.dev}/lib/pkgconfig:${pkgs.libsoup_3.dev}/lib/pkgconfig:${pkgs.webkitgtk_4_1.dev}/lib/pkgconfig:${pkgs.udev.dev}/lib/pkgconfig:${pkgs.zlib.dev}/share/pkgconfig"
            export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib"
          '';

          buildNpmPackage = pkgs.buildNpmPackage.override { nodejs = pkgs.nodejs_20; };
          webPackage = buildNpmPackage {
            pname = "pokecon-web";
            version = workspaceVersion;
            src = source;
            sourceRoot = "pokecon-source/web";
            npmDepsHash = "sha256-4YUu0CTgj8cEBMhAU+MU6jUVcqijpefiDMMAvye+zco=";
            installPhase = ''
              runHook preInstall
              mkdir -p "$out"
              cp -R dist/. "$out/"
              runHook postInstall
            '';
          };

          pokeconPackage = rustPlatform.buildRustPackage {
            pname = "pokecon";
            version = workspaceVersion;
            src = source;
            nativeBuildInputs = [
              pkgs.nasm
              pkgs.pkg-config
            ]
            ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.llvmPackages.libclang ];
            buildInputs = [ pythonEnv ] ++ linuxDesktopPackages;
            cargoLock = {
              lockFile = ./Cargo.lock;
              allowBuiltinFetchGit = true;
            };
            cargoBuildFlags = [
              "--features"
              "tauri-shell"
              "--package"
              "pokecon-app"
              "--package"
              "pokecon-worker"
              "--bin"
              "pokecon"
              "--bin"
              "pokecon-worker"
            ];
            cargoTestFlags = [
              "--features"
              "tauri-shell"
              "--package"
              "pokecon-app"
              "--package"
              "pokecon-worker"
            ];
            doCheck = true;
            preCheck = ''
              runtimeRoot="target/${pkgs.stdenv.targetPlatform.rust.cargoShortTarget}/$cargoCheckType"
              mkdir -p "$runtimeRoot/uv"
              ln -sfn "${pkgs.uv}/bin/uv" "$runtimeRoot/uv/uv"
            '';
            POKECON_BUILD_UV_PATH = "${pkgs.uv}/bin/uv";
            POKECON_BUILD_UV_VERSION = pkgs.uv.version;
            POKECON_INTERNAL_SCRIPT_SITE_PACKAGES = "${pythonEnv}/${pkgs.python314.sitePackages}";
            PYO3_PYTHON = "${pythonEnv}/bin/python";
            POKECON_BUILD_PYTHON = "${pythonEnv}/bin/python";
            BINDGEN_EXTRA_CLANG_ARGS = linuxBindgenArgs;
            LIBCLANG_PATH = lib.optionalString pkgs.stdenv.isLinux "${pkgs.llvmPackages.libclang.lib}/lib";
            postInstall = ''
              mkdir -p "$out/web/dist"
              cp -R "${webPackage}/." "$out/web/dist/"
              ln -s ../web "$out/bin/web"
              mkdir -p "$out/bin/uv"
              cp "${pkgs.uv}/bin/uv" "$out/bin/uv/uv"
            '';
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
                "web/.svelte-kit/**"
                "web/dist/**"
                "web/node_modules/**"
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
            pokecon-server = pokeconPackage;
            web = webPackage;
          };

          checks.pokecon = pokeconPackage;
          checks.web = webPackage;

          apps = {
            default = mkApp "${self'.packages.pokecon}/bin/pokecon";
            fmt = mkApp "${config.treefmt.build.wrapper}/bin/treefmt";

            actionlint = mkTask {
              name = "actionlint";
              runtimeInputs = [ pkgs.actionlint ];
              text = ''
                cd "${source}"
                if [ "$#" -eq 0 ]; then
                  exec actionlint .github/workflows/*.yml
                fi
                exec actionlint "$@"
              '';
            };

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
                mkdir -p "$caller_dir/dist"
                cp dist/*.whl "$caller_dir/dist/"
              '';
            };

            contract-check = mkTask {
              name = "contract-check";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.actionlint
                pkgs.basedpyright
                pkgs.nodejs_20
                pkgs.shellcheck
              ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD/python:$PWD''${PYTHONPATH:+:$PYTHONPATH}"
                cargo run --locked --package pokecon-contracts --bin generate_contracts -- --check
                cargo test --locked --package pokecon-contracts --test contract_sync
                scripts/generate-api-types.sh --check
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

            compatibility = mkTask {
              name = "compatibility";
              runtimeInputs = rustTaskInputs ++ [ pkgs.git ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD''${PYTHONPATH:+:$PYTHONPATH}"
                cargo build --locked --jobs 1 --package pokecon-worker --bin pokecon-worker --bin pokecon-compatibility
                python -m scripts.compatibility_promote --check
                exec python -m scripts.compatibility_runner \
                  --check \
                  --compatibility-binary "$CARGO_TARGET_DIR/debug/pokecon-compatibility" \
                  --worker "$CARGO_TARGET_DIR/debug/pokecon-worker" \
                  --site-packages "${pythonEnv}/${pkgs.python314.sitePackages}" \
                  "$@"
              '';
            };

            compatibility-roll = mkTask {
              name = "compatibility-roll";
              runtimeInputs = rustTaskInputs ++ [ pkgs.git ];
              text = ''
                ${desktopEnvironment}
                export POKECON_BUILD_UV_PATH="${pkgs.uv}/bin/uv"
                export POKECON_BUILD_UV_VERSION="${pkgs.uv.version}"
                export POKECON_INTERNAL_SCRIPT_SITE_PACKAGES="${pythonEnv}/${pkgs.python314.sitePackages}"
                export PYO3_PYTHON="${pythonEnv}/bin/python"
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD''${PYTHONPATH:+:$PYTHONPATH}"
                export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-$PWD/target/nix-tasks}"
                cargo build --locked --jobs 1 --package pokecon-worker --bin pokecon-worker --bin pokecon-compatibility
                exec python -m scripts.compatibility_roll \
                  --compatibility-binary "$CARGO_TARGET_DIR/debug/pokecon-compatibility" \
                  --worker "$CARGO_TARGET_DIR/debug/pokecon-worker" \
                  --site-packages "${pythonEnv}/${pkgs.python314.sitePackages}" \
                  "$@"
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

            release-check = mkTask {
              name = "release-check";
              runtimeInputs = [ pythonEnv ];
              text = ''
                cd "${source}"
                exec python -m scripts.release_gate "$@"
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
              runtimeInputs = rustTaskInputs ++ [
                pkgs.binutils
                pkgs.cargo-tauri
                pkgs.dpkg
                pkgs.nodejs_20
                pkgs.patchelf
                pkgs.uv
              ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                export POKECON_WEB_VERSION="${workspaceVersion}"
                export SOURCE_DATE_EPOCH=0
                npm --prefix web ci --no-audit --no-fund
                npm --prefix web run build
                release_python="$workdir/release-python"
                release_wheelhouse="$workdir/release-wheelhouse"
                python -m scripts.build_release_runtime \
                  --project "$workdir" \
                  --uv "${portableUvBinary}" \
                  --runtime-output "$release_python" \
                  --wheelhouse-output "$release_wheelhouse" \
                  --patchelf "${pkgs.patchelf}/bin/patchelf" \
                  --strip "${pkgs.binutils}/bin/strip" \
                  --runtime-library-path "${pkgs.portaudio}/lib"
                export PYO3_PYTHON="$release_python/bin/python3.14"
                export POKECON_BUILD_UV_PATH="${portableUvBinary}"
                export POKECON_BUILD_UV_VERSION="${portableUvVersion}"
                unset POKECON_BUILD_PYTHON
                cargo build --locked --release --package pokecon-worker --bin pokecon-worker
                (
                  cd rust/pokecon-app
                  cargo tauri build --ci --no-bundle -- --locked
                )
                python -m scripts.normalize_linux_elf \
                  --application "$CARGO_TARGET_DIR/release/pokecon" \
                  --worker "$CARGO_TARGET_DIR/release/pokecon-worker" \
                  --python-root "$release_python" \
                  --patchelf "${pkgs.patchelf}/bin/patchelf" \
                  --strip "${pkgs.binutils}/bin/strip" \
                  --objdump "${pkgs.binutils}/bin/objdump"
                bundle_root="$workdir/bundle-resources"
                bundle_config="$workdir/tauri.bundle.json"
                python -m scripts.stage_release \
                  --web "$workdir/web/dist" \
                  --worker "$CARGO_TARGET_DIR/release/pokecon-worker" \
                  --uv "${portableUvBinary}" \
                  --wheelhouse "$release_wheelhouse" \
                  --python "$release_python" \
                  --output "$bundle_root" \
                  --config-output "$bundle_config"
                bundle_args=("$@")
                if [ "''${#bundle_args[@]}" -eq 0 ]; then
                  bundle_args=(--bundles deb)
                fi
                (
                  cd rust/pokecon-app
                  cargo tauri bundle --ci --config "$bundle_config" "''${bundle_args[@]}"
                )
                while IFS= read -r -d "" package; do
                  python -m scripts.normalize_debian_package \
                    --dpkg-deb "${pkgs.dpkg}/bin/dpkg-deb" \
                    "$package"
                done < <(find "$CARGO_TARGET_DIR/release/bundle" -type f -name '*.deb' -print0)
                artifact_dir="$caller_dir/dist/tauri"
                mkdir -p "$artifact_dir"
                find "$CARGO_TARGET_DIR/release/bundle" -type f \
                  \( -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' -o -name '*.dmg' -o -name '*.msi' -o -name '*-setup.exe' \) \
                  -exec cp {} "$artifact_dir/" \;
              '';
            };

            package-smoke = mkTask {
              name = "package-smoke";
              runtimeInputs = [
                pythonEnv
                pkgs.binutils
                pkgs.dpkg
                pkgs.patchelf
                pkgs.portaudio
              ];
              text = ''
                export PYTHONPATH="${source}''${PYTHONPATH:+:$PYTHONPATH}"
                exec python -m scripts.package_smoke \
                  --dpkg-deb "${pkgs.dpkg}/bin/dpkg-deb" \
                  --patchelf "${pkgs.patchelf}/bin/patchelf" \
                  --objdump "${pkgs.binutils}/bin/objdump" \
                  --runtime-library-path "${pkgs.portaudio}/lib" \
                  "$@"
              '';
            };

            package-install-smoke = mkTask {
              name = "package-install-smoke";
              runtimeInputs = [
                pkgs.coreutils
                pkgs.docker-client
              ];
              text = ''
                export POKECON_DOCKER="${pkgs.docker-client}/bin/docker"
                exec "${pkgs.bash}/bin/bash" "${source}/scripts/debian_install_smoke.sh" "$@"
              '';
            };

            tauri-check = mkTask {
              name = "tauri-check";
              runtimeInputs = rustTaskInputs ++ [ pkgs.cargo-tauri ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                cd rust/pokecon-app
                exec cargo tauri build --debug --no-bundle --ci -- --locked
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
                exec typos "$@"
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
                  npm ci --no-audit --no-fund
                  npm run lint
                  npm run svelte-check
                  npm test
                  npm run build
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
              runtimeInputs = rustTaskInputs ++ [ pkgs.nodejs_20 ];
              text = ''
                ${desktopEnvironment}
                exec scripts/generate-api-types.sh "$@"
              '';
            };

            check = mkTask {
              name = "check";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.basedpyright
                pkgs.markdownlint-cli
                pkgs.nodejs_20
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
                actionlint .github/workflows/*.yml
                python -m scripts.release_gate
                cargo run --locked --package pokecon-contracts --bin generate_contracts -- --check
                cargo test --locked --package pokecon-contracts --test contract_sync
                scripts/generate-api-types.sh --check
                npm --prefix web ci --no-audit --no-fund
                npm --prefix web run lint
                npm --prefix web run svelte-check
                npm --prefix web test
                npm --prefix web run build
                cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
                cargo test --locked --workspace --all-features
                cargo build --locked --workspace --all-features
                python -m scripts.compatibility_promote --check
                python -m scripts.compatibility_runner \
                  --check \
                  --compatibility-binary "$CARGO_TARGET_DIR/debug/pokecon-compatibility" \
                  --worker "$CARGO_TARGET_DIR/debug/pokecon-worker" \
                  --site-packages "${pythonEnv}/${pkgs.python314.sitePackages}"
                ruff check --config ruff.toml --no-cache python scripts tests
                ruff format --config ruff.toml --no-cache --check python scripts tests
                basedpyright
                python -m pytest -p no:cacheprovider tests -v --tb=short
                shellcheck scripts/*.sh
                markdownlint --config .markdownlint.json ./*.md docs/*.md
                textlint --config .textlintrc.json ./*.md docs/*.md ./*.txt
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
            POKECON_BUILD_PYTHON = "${pythonEnv}/bin/python";
            POKECON_BUILD_UV_PATH = "${pkgs.uv}/bin/uv";
            POKECON_BUILD_UV_VERSION = pkgs.uv.version;
            POKECON_INTERNAL_SCRIPT_SITE_PACKAGES = "${pythonEnv}/${pkgs.python314.sitePackages}";
            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            LIBCLANG_PATH = lib.optionalString pkgs.stdenv.isLinux "${pkgs.llvmPackages.libclang.lib}/lib";
            shellHook = config.pre-commit.shellHook + ''
              echo "PokeCon Nix development shell: Rust $(rustc --version), Python $(python --version)"
            '';
          };
        };
    };
}
