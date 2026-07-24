{
  description = "Poke Controller Modified Extension development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Ubuntu 24.04 is the Linux package baseline, so native release artifacts
    # must be built and exercised against its glibc 2.39 ABI ceiling.
    linux-release-nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
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
          linuxReleaseMaximumGlibc = "2.39";
          linuxReleasePkgs =
            if !pkgs.stdenv.isLinux then
              pkgs
            else
              let
                releasePkgs = import inputs.linux-release-nixpkgs { inherit system; };
                releaseGlibcVersion = lib.versions.majorMinor releasePkgs.glibc.version;
              in
              assert lib.assertMsg (lib.versionAtLeast linuxReleaseMaximumGlibc releaseGlibcVersion)
                "Linux release dependencies use glibc ${releaseGlibcVersion}; expected at most ${linuxReleaseMaximumGlibc}";
              releasePkgs;
          linuxReleaseCc = linuxReleasePkgs.stdenv.cc;
          linuxReleasePortaudio = linuxReleasePkgs.portaudio;
          linuxReleaseBuildPath = lib.makeBinPath [
            linuxReleaseCc
            linuxReleasePkgs.bash
            linuxReleasePkgs.coreutils
            linuxReleasePkgs.gnumake
            linuxReleasePkgs.pkg-config
          ];
          linuxReleaseEvdevConfig =
            if pkgs.stdenv.isLinux then
              pkgs.writeText "pokecon-linux-release-distutils.cfg" ''
                [build_ecodes]
                evdev_headers = ${linuxReleasePkgs.linuxHeaders}/include/linux/input.h:${linuxReleasePkgs.linuxHeaders}/include/linux/input-event-codes.h:${linuxReleasePkgs.linuxHeaders}/include/linux/uinput.h
                reproducible = 1
              ''
            else
              pkgs.writeText "pokecon-non-linux-release-distutils.cfg" "";
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
          webManifest = builtins.fromJSON (builtins.readFile ./web/package.json);
          bunPackageManager = webManifest.packageManager;
          bunVersion = lib.removePrefix "bun@" bunPackageManager;
          bun =
            assert lib.assertMsg (lib.hasPrefix "bun@" bunPackageManager)
              "web/package.json packageManager must select Bun";
            assert lib.assertMsg (
              pkgs.bun.version == bunVersion
            ) "Nix provides Bun ${pkgs.bun.version}; web/package.json requires ${bunVersion}";
            pkgs.bun;
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
          reproducibleRustcWrapper = pkgs.writeShellScript "pokecon-reproducible-rustc-wrapper" ''
            set -o errexit -o nounset -o pipefail
            rustc="$1"
            shift
            : "''${POKECON_RUST_REMAP_SOURCE:?POKECON_RUST_REMAP_SOURCE is required}"
            : "''${POKECON_RUST_REMAP_PYTHON:?POKECON_RUST_REMAP_PYTHON is required}"
            exec "$rustc" \
              "--remap-path-prefix=$POKECON_RUST_REMAP_SOURCE=/build/pokecon" \
              "--remap-path-prefix=$POKECON_RUST_REMAP_PYTHON=/build/python" \
              "-Lnative=$POKECON_RUST_REMAP_PYTHON/lib" \
              "$@"
          '';

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
          basedpyrightCli = "${pkgs.basedpyright}/lib/node_modules/pyright-root/index.js";
          markdownlintCli = "${pkgs.markdownlint-cli}/lib/node_modules/markdownlint-cli/markdownlint.js";
          textlintCli = "${pkgs.textlint}/lib/node_modules/textlint/bin/textlint.js";
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
            cp -a "${source}/." "$workdir/"
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

          webBunDependencies = pkgs.stdenvNoCC.mkDerivation {
            pname = "pokecon-web-bun-dependencies";
            version = workspaceVersion;
            src = source;
            sourceRoot = "pokecon-source/web";
            nativeBuildInputs = [
              bun
              pkgs.writableTmpDirAsHomeHook
            ];
            dontConfigure = true;
            buildPhase = ''
              runHook preBuild
              export BUN_INSTALL_CACHE_DIR="$TMPDIR/bun-cache"
              bun install \
                --cpu="*" \
                --frozen-lockfile \
                --ignore-scripts \
                --no-progress \
                --os="*"
              runHook postBuild
            '';
            installPhase = ''
              runHook preInstall
              mkdir -p "$out"
              cp -R node_modules "$out/"
              runHook postInstall
            '';
            dontFixup = true;
            outputHash = "sha256-N8dxmWteovkSMk9wA6K8js8kD9/xB8Ra8QntcnbEC9k=";
            outputHashMode = "recursive";
          };
          webPackage = pkgs.stdenvNoCC.mkDerivation {
            pname = "pokecon-web";
            version = workspaceVersion;
            src = source;
            sourceRoot = "pokecon-source/web";
            nativeBuildInputs = [ bun ];
            configurePhase = ''
              runHook preConfigure
              cp -R "${webBunDependencies}/node_modules" .
              chmod -R u+w node_modules
              runHook postConfigure
            '';
            buildPhase = ''
              runHook preBuild
              bun run --bun build
              runHook postBuild
            '';
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

            acceptance-record-check = mkTask {
              name = "acceptance-record-check";
              runtimeInputs = [
                pythonEnv
                pkgs.check-jsonschema
              ];
              text = ''
                cd "${source}"
                export PYTHONDONTWRITEBYTECODE=1
                exec python -m scripts.acceptance.records "$@"
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

            virtual-io-check = mkTask {
              name = "virtual-io-check";
              runtimeInputs =
                rustTaskInputs
                ++ lib.optionals pkgs.stdenv.isLinux [
                  pkgs.ffmpeg
                  pkgs.gnugrep
                  pkgs.kmod
                  pkgs.v4l-utils
                ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                "${pkgs.bash}/bin/bash" scripts/integration/virtual-io-smoke.sh "$@"
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
                bun
                pkgs.check-jsonschema
                pkgs.shellcheck
              ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD/python:$PWD''${PYTHONPATH:+:$PYTHONPATH}"
                cargo run --locked --package pokecon-contracts --bin generate_contracts -- --check
                cargo test --locked --package pokecon-contracts --test contract_sync
                check-jsonschema --check-metaschema generated/settings.schema.json
                python -m scripts.acceptance.records
                scripts/quality/generate-api-types.sh --check
                bun --bun "${basedpyrightCli}"
                shellcheck scripts/*.sh scripts/*/*.sh
                python -m scripts.quality.source_filter
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
                exec python -m scripts.compatibility.inventory --check "$@"
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
                python -m scripts.compatibility.promote --check
                exec python -m scripts.compatibility.runner \
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
                exec python -m scripts.compatibility.roll \
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
                exec python -m scripts.quality.source_guard "$@"
              '';
            };

            source-filter-check = mkTask {
              name = "source-filter-check";
              runtimeInputs = [ pythonEnv ];
              text = ''
                cd "${source}"
                exec python -m scripts.quality.source_filter "$@"
              '';
            };

            release-check = mkTask {
              name = "release-check";
              runtimeInputs = [ pythonEnv ];
              text = ''
                cd "${source}"
                exec python -m scripts.release.gate "$@"
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
                bun
                pythonEnv
                pkgs.basedpyright
              ];
              text = ''
                cd "${source}"
                export PYTHONDONTWRITEBYTECODE=1
                exec bun --bun "${basedpyrightCli}"
              '';
            };

            test = mkTask {
              name = "test";
              runtimeInputs = [
                pythonEnv
                pkgs.check-jsonschema
              ];
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
                bun
                pkgs.cargo-tauri
                pkgs.dpkg
                pkgs.patchelf
                pkgs.uv
              ];
              text = ''
                ${setupWorkdir}
                release_workdir="$CARGO_TARGET_DIR/pokecon-release-workdir"
                rm -rf -- "$release_workdir"
                mv "$workdir" "$release_workdir"
                workdir="$release_workdir"
                cd "$workdir"
                ${desktopEnvironment}
                export POKECON_WEB_VERSION="${workspaceVersion}"
                export SOURCE_DATE_EPOCH=0
                bun install --cwd web --frozen-lockfile --ignore-scripts --no-progress
                bun run --cwd web --bun build
                release_python="$workdir/release-python"
                release_wheelhouse="$workdir/release-wheelhouse"
                release_build_home="$workdir/release-build-home"
                release_build_tmp="$workdir/release-build-tmp"
                release_uv_cache="$workdir/release-uv-cache"
                mkdir -p "$release_build_home" "$release_build_tmp" "$release_uv_cache"
                cp "${linuxReleaseEvdevConfig}" "$release_build_home/.pydistutils.cfg"
                "${pkgs.coreutils}/bin/env" -i \
                  AR="${linuxReleaseCc}/bin/ar" \
                  AS="${linuxReleaseCc}/bin/as" \
                  CC="${linuxReleaseCc}/bin/gcc" \
                  CFLAGS="-I${linuxReleasePortaudio}/include" \
                  CPP="${linuxReleaseCc}/bin/cpp" \
                  CXX="${linuxReleaseCc}/bin/g++" \
                  HOME="$release_build_home" \
                  LANG=C.UTF-8 \
                  LC_ALL=C.UTF-8 \
                  LD="${linuxReleaseCc}/bin/ld" \
                  LDFLAGS="-L${linuxReleasePortaudio}/lib" \
                  NIX_SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" \
                  PATH="${linuxReleaseBuildPath}" \
                  PKG_CONFIG_PATH="${linuxReleasePortaudio}/lib/pkgconfig" \
                  PYTHONPATH="$workdir" \
                  RANLIB="${linuxReleaseCc}/bin/ranlib" \
                  SOURCE_DATE_EPOCH=0 \
                  SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" \
                  STRIP="${linuxReleaseCc}/bin/strip" \
                  TMPDIR="$release_build_tmp" \
                  TZ=UTC \
                  UV_CACHE_DIR="$release_uv_cache" \
                  XDG_CACHE_HOME="$release_build_home/.cache" \
                  XDG_CONFIG_HOME="$release_build_home/.config" \
                  "${pythonEnv}/bin/python" -m scripts.release.build_runtime \
                  --project "$workdir" \
                  --uv "${portableUvBinary}" \
                  --runtime-output "$release_python" \
                  --wheelhouse-output "$release_wheelhouse" \
                  --patchelf "${pkgs.patchelf}/bin/patchelf" \
                  --strip "${pkgs.binutils}/bin/strip" \
                  --runtime-library-path "${linuxReleasePortaudio}/lib"
                export PYO3_PYTHON="$release_python/bin/python3.14"
                export CFLAGS="-ffile-prefix-map=$workdir=/build/pokecon -ffile-prefix-map=$release_python=/build/python''${CFLAGS:+ $CFLAGS}"
                export CXXFLAGS="-ffile-prefix-map=$workdir=/build/pokecon -ffile-prefix-map=$release_python=/build/python''${CXXFLAGS:+ $CXXFLAGS}"
                export POKECON_RUST_REMAP_SOURCE="$workdir"
                export POKECON_RUST_REMAP_PYTHON="$release_python"
                export RUSTC_WRAPPER="${reproducibleRustcWrapper}"
                export POKECON_BUILD_UV_PATH="${portableUvBinary}"
                export POKECON_BUILD_UV_VERSION="${portableUvVersion}"
                unset POKECON_BUILD_PYTHON
                cargo build --locked --release --package pokecon-worker --bin pokecon-worker
                cargo build \
                  --locked \
                  --release \
                  --package pokecon-app \
                  --bin pokecon \
                  --features tauri-shell
                normalized_bin="$workdir/normalized-bin"
                application="$CARGO_TARGET_DIR/release/pokecon"
                worker="$CARGO_TARGET_DIR/release/pokecon-worker"
                normalized_application="$normalized_bin/pokecon"
                normalized_worker="$normalized_bin/pokecon-worker"
                application_backup="$normalized_bin/pokecon.raw"
                mkdir -p "$normalized_bin"
                cp -p "$application" "$application_backup"
                cp -p "$application" "$normalized_application"
                cp -p "$worker" "$normalized_worker"
                python -m scripts.release.normalize_linux_elf \
                  --application "$normalized_application" \
                  --worker "$normalized_worker" \
                  --python-root "$release_python" \
                  --patchelf "${pkgs.patchelf}/bin/patchelf" \
                  --strip "${pkgs.binutils}/bin/strip" \
                  --objdump "${pkgs.binutils}/bin/objdump"
                bundle_root="$workdir/bundle-resources"
                bundle_config="$workdir/tauri.bundle.json"
                python -m scripts.release.stage \
                  --web "$workdir/web/dist" \
                  --worker "$normalized_worker" \
                  --uv "${portableUvBinary}" \
                  --wheelhouse "$release_wheelhouse" \
                  --python "$release_python" \
                  --output "$bundle_root" \
                  --config-output "$bundle_config"
                bundle_args=("$@")
                if [ "''${#bundle_args[@]}" -eq 0 ]; then
                  bundle_args=(--bundles deb)
                fi
                restore_release_application() {
                  cp -p "$application_backup" "$application"
                }
                trap 'restore_release_application; rm -rf "$workdir"' EXIT
                cp -p "$normalized_application" "$application"
                (
                  cd rust/pokecon-app
                  cargo tauri bundle --ci --config "$bundle_config" "''${bundle_args[@]}"
                )
                restore_release_application
                while IFS= read -r -d "" package; do
                  python -m scripts.release.normalize_debian_package \
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
                linuxReleasePortaudio
              ];
              text = ''
                export PYTHONPATH="${source}''${PYTHONPATH:+:$PYTHONPATH}"
                unset LD_LIBRARY_PATH
                exec python -m scripts.release.package_smoke \
                  --dpkg-deb "${pkgs.dpkg}/bin/dpkg-deb" \
                  --patchelf "${pkgs.patchelf}/bin/patchelf" \
                  --objdump "${pkgs.binutils}/bin/objdump" \
                  --runtime-library-path "${linuxReleasePortaudio}/lib" \
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
                exec "${pkgs.bash}/bin/bash" "${source}/scripts/release/debian_install_smoke.sh" "$@"
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
                bun
                pkgs.markdownlint-cli
                pkgs.ripgrep
              ];
              text = ''
                cd "${source}"
                markdown_files=("$@")
                if [ "''${#markdown_files[@]}" -eq 0 ]; then
                  mapfile -t markdown_files < <(rg --files -g '*.md')
                fi
                exec bun --bun "${markdownlintCli}" --config .markdownlint.json "''${markdown_files[@]}"
              '';
            };

            markdownlint-check = mkTask {
              name = "markdownlint-check";
              runtimeInputs = [
                bun
                pkgs.markdownlint-cli
                pkgs.ripgrep
              ];
              text = ''
                cd "${source}"
                markdown_files=("$@")
                if [ "''${#markdown_files[@]}" -eq 0 ]; then
                  mapfile -t markdown_files < <(rg --files -g '*.md')
                fi
                exec bun --bun "${markdownlintCli}" --config .markdownlint.json "''${markdown_files[@]}"
              '';
            };

            textlint = mkTask {
              name = "textlint";
              runtimeInputs = [
                bun
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
                exec bun --bun "${textlintCli}" --config .textlintrc.json "''${text_files[@]}"
              '';
            };

            textlint-check = mkTask {
              name = "textlint-check";
              runtimeInputs = [
                bun
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
                exec bun --bun "${textlintCli}" --config .textlintrc.json "''${text_files[@]}"
              '';
            };

            web-check = mkTask {
              name = "web-check";
              runtimeInputs = [
                bun
                pythonEnv
              ];
              text = ''
                cd "${source}"
                if python -m scripts.quality.source_guard web --require-applicable; then
                  workdir="$(mktemp -d)"
                  trap 'rm -rf "$workdir"' EXIT
                  cp -R web/. "$workdir/"
                  chmod -R u+w "$workdir"
                  cd "$workdir"
                  bun install --frozen-lockfile --ignore-scripts --no-progress
                  bun run --bun lint
                  bun run --bun svelte-check
                  bun run --bun test
                  bun run --bun build
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
              runtimeInputs = rustTaskInputs ++ [ bun ];
              text = ''
                ${desktopEnvironment}
                exec scripts/quality/generate-api-types.sh "$@"
              '';
            };

            check = mkTask {
              name = "check";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.basedpyright
                bun
                pkgs.check-jsonschema
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
                python -m scripts.quality.source_filter
                actionlint .github/workflows/*.yml
                python -m scripts.release.gate
                cargo run --locked --package pokecon-contracts --bin generate_contracts -- --check
                cargo test --locked --package pokecon-contracts --test contract_sync
                check-jsonschema --check-metaschema generated/settings.schema.json
                python -m scripts.acceptance.records
                scripts/quality/generate-api-types.sh --check
                bun install --cwd web --frozen-lockfile --ignore-scripts --no-progress
                bun run --cwd web --bun lint
                bun run --cwd web --bun svelte-check
                bun run --cwd web --bun test
                bun run --cwd web --bun build
                cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
                cargo test --locked --workspace --all-features
                cargo build --locked --workspace --all-features
                python -m scripts.compatibility.promote --check
                python -m scripts.compatibility.runner \
                  --check \
                  --compatibility-binary "$CARGO_TARGET_DIR/debug/pokecon-compatibility" \
                  --worker "$CARGO_TARGET_DIR/debug/pokecon-worker" \
                  --site-packages "${pythonEnv}/${pkgs.python314.sitePackages}"
                ruff check --config ruff.toml --no-cache python scripts tests
                ruff format --config ruff.toml --no-cache --check python scripts tests
                bun --bun "${basedpyrightCli}"
                python -m pytest -p no:cacheprovider tests -v --tb=short
                shellcheck scripts/*.sh scripts/*/*.sh
                bun --bun "${markdownlintCli}" --config .markdownlint.json ./*.md docs/*.md
                bun --bun "${textlintCli}" --config .textlintrc.json ./*.md docs/*.md docs/legacy/*.txt ./*.txt
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
                  entry = "bash ${source}/scripts/quality/check-workspace-lock.sh";
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
              bun
              pkgs.cargo-tauri
              pkgs.curl
              pkgs.gh
              pkgs.git
              pkgs.jq
              pkgs.markdownlint-cli
              pkgs.maturin
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
              echo "PokeCon Nix development shell: Rust $(rustc --version), Python $(python --version), Bun $(bun --version)"
            '';
          };
        };
    };
}
