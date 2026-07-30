{
  description = "Poke Controller Modified Extension packages and reproducible task apps";

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
            : "''${POKECON_RUST_REMAP_TARGET:?POKECON_RUST_REMAP_TARGET is required}"
            exec "$rustc" \
              "--remap-path-prefix=$POKECON_RUST_REMAP_SOURCE=/build/pokecon" \
              "--remap-path-prefix=$POKECON_RUST_REMAP_PYTHON=/build/python" \
              "--remap-path-prefix=$POKECON_RUST_REMAP_TARGET=/build/target" \
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
              # Keep legacy JavaScript visible so source_filter can reject it
              # instead of silently omitting it from the Nix source tree.
              || lib.hasSuffix ".js" sourcePath
              || lib.hasSuffix ".jsx" sourcePath
              || lib.hasSuffix ".mjs" sourcePath
              || lib.hasSuffix ".cjs" sourcePath
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
          defaultTaskRuntimeInputs = [
            pkgs.bash
            pkgs.coreutils
          ];
          mkTask =
            {
              excludeShellChecks ? [ ],
              name,
              runtimeInputs ? [ ],
              text,
            }:
            let
              taskRuntimeInputs = defaultTaskRuntimeInputs ++ runtimeInputs;
              taskApplication = pkgs.writeShellApplication {
                inherit
                  excludeShellChecks
                  name
                  ;
                runtimeInputs = taskRuntimeInputs;
                text = ''
                  export PATH="${lib.makeBinPath taskRuntimeInputs}"
                  ${text}
                '';
              };
              taskLauncher = pkgs.writeTextFile {
                name = "${name}-launcher";
                destination = "/bin/${name}";
                executable = true;
                text = ''
                  #!${pkgs.bash}/bin/bash -p
                  blocked_environment=()
                  while IFS= read -r -d "" environment_entry; do
                    environment_name="''${environment_entry%%=*}"
                    case "$environment_name" in
                      BASHOPTS | BASH_COMPAT | BASH_ENV | BASH_FUNC_* | BASH_LOADABLES_PATH | BASH_XTRACEFD | CDPATH | ENV | GLOBIGNORE | POSIXLY_CORRECT | PS4 | SHELLOPTS)
                        blocked_environment+=(-u "$environment_name")
                        ;;
                    esac
                  done < <("${pkgs.coreutils}/bin/env" -0)
                  exec "${pkgs.coreutils}/bin/env" "''${blocked_environment[@]}" \
                    "${pkgs.bash}/bin/bash" -p "${taskApplication}/bin/${name}" "$@"
                '';
              };
            in
            mkApp "${taskLauncher}/bin/${name}";

          formatterRunner = pkgs.writeShellApplication {
            name = "pokecon-formatter-runner";
            runtimeInputs = [
              pkgs.bash
              pkgs.coreutils
            ];
            text = ''
              formatter_root="$PWD"
              umask 022
              formatter_home="$(mktemp -d -p /tmp pokecon-formatter-home.XXXXXXXX)"
              cleanup_formatter_home() {
                formatter_status=$?
                trap - EXIT
                rm -rf -- "$formatter_home"
                exit "$formatter_status"
              }
              trap cleanup_formatter_home EXIT
              mkdir -p \
                "$formatter_home/.cache" \
                "$formatter_home/.config" \
                "$formatter_home/.local/state" \
                "$formatter_home/runtime" \
                "$formatter_home/tmp"
              chmod 0700 "$formatter_home/runtime"
              "${pkgs.coreutils}/bin/env" -i \
                CI=1 \
                HOME="$formatter_home" \
                LANG=C \
                LC_ALL=C \
                PATH="${
                  lib.makeBinPath [
                    pkgs.bash
                    pkgs.coreutils
                  ]
                }" \
                PWD="$formatter_root" \
                TMPDIR="$formatter_home/tmp" \
                TZ=UTC \
                XDG_CACHE_HOME="$formatter_home/.cache" \
                XDG_CONFIG_HOME="$formatter_home/.config" \
                XDG_RUNTIME_DIR="$formatter_home/runtime" \
                XDG_STATE_HOME="$formatter_home/.local/state" \
                "${config.treefmt.build.wrapper}/bin/treefmt" \
                --working-dir "$formatter_root" \
                "$@"
            '';
          };
          safeFormatter = pkgs.writeTextFile {
            name = "pokecon-formatter";
            destination = "/bin/pokecon-format";
            executable = true;
            meta.mainProgram = "pokecon-format";
            text = ''
              #!${pkgs.bash}/bin/bash -p
              blocked_environment=()
              while IFS= read -r -d "" environment_entry; do
                environment_name="''${environment_entry%%=*}"
                case "$environment_name" in
                  BASHOPTS | BASH_COMPAT | BASH_ENV | BASH_FUNC_* | BASH_LOADABLES_PATH | BASH_XTRACEFD | CDPATH | ENV | GLOBIGNORE | POSIXLY_CORRECT | PS4 | SHELLOPTS)
                    blocked_environment+=(-u "$environment_name")
                    ;;
                esac
              done < <("${pkgs.coreutils}/bin/env" -0)
              exec "${pkgs.coreutils}/bin/env" "''${blocked_environment[@]}" \
                "${pkgs.bash}/bin/bash" -p "${formatterRunner}/bin/pokecon-formatter-runner" "$@"
            '';
          };
          hookHardeningSnippet = pkgs.writeText "pokecon-pre-commit-hardening.sh" ''
            # POKECON_HARDENED_PRE_COMMIT_V1
            pokecon_hook_root="$("${pkgs.git}/bin/git" rev-parse --show-toplevel)"
            pokecon_git_environment=()
            while IFS= read -r -d "" pokecon_hook_entry; do
              pokecon_hook_name="''${pokecon_hook_entry%%=*}"
              case "$pokecon_hook_name" in
                GIT_ALTERNATE_OBJECT_DIRECTORIES | GIT_COMMON_DIR | GIT_DIR | GIT_INDEX_FILE | GIT_OBJECT_DIRECTORY | GIT_PREFIX | GIT_WORK_TREE)
                  pokecon_git_environment+=("$pokecon_hook_entry")
                  ;;
                PATH | PWD)
                  ;;
                *)
                  unset "$pokecon_hook_name" 2>/dev/null || true
                  ;;
              esac
            done < <("${pkgs.coreutils}/bin/env" -0)
            umask 022
            export PATH="${
              lib.makeBinPath [
                pkgs.bash
                pkgs.coreutils
                pkgs.git
                pkgs.nix
              ]
            }"
            cd "$pokecon_hook_root"
            pokecon_hook_home="$("${pkgs.coreutils}/bin/mktemp" -d -p /tmp pokecon-pre-commit-home.XXXXXXXX)"
            "${pkgs.coreutils}/bin/mkdir" -p \
              "$pokecon_hook_home/.cache" \
              "$pokecon_hook_home/.config" \
              "$pokecon_hook_home/.local/state" \
              "$pokecon_hook_home/runtime" \
              "$pokecon_hook_home/tmp"
            "${pkgs.coreutils}/bin/chmod" 0700 "$pokecon_hook_home/runtime"
            pokecon_hook_environment=(
              "CI=1"
              "CURL_CA_BUNDLE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              "GIT_CONFIG_NOSYSTEM=1"
              "GIT_SSL_CAINFO=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              "HOME=$pokecon_hook_home"
              "LANG=C"
              "LC_ALL=C"
              "NIX_SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              "PATH=${
                lib.makeBinPath [
                  pkgs.bash
                  pkgs.coreutils
                  pkgs.git
                  pkgs.nix
                ]
              }"
              "PWD=$pokecon_hook_root"
              "PYTHONDONTWRITEBYTECODE=1"
              "PYTHONNOUSERSITE=1"
              "REQUESTS_CA_BUNDLE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              "TMPDIR=$pokecon_hook_home/tmp"
              "TZ=UTC"
              "UV_NO_CONFIG=1"
              "XDG_CACHE_HOME=$pokecon_hook_home/.cache"
              "XDG_CONFIG_HOME=$pokecon_hook_home/.config"
              "XDG_RUNTIME_DIR=$pokecon_hook_home/runtime"
              "XDG_STATE_HOME=$pokecon_hook_home/.local/state"
              "''${pokecon_git_environment[@]}"
            )
            pokecon_cleanup_hook_home() {
              pokecon_hook_status=$?
              trap - EXIT
              "${pkgs.coreutils}/bin/rm" -rf -- "$pokecon_hook_home"
              exit "$pokecon_hook_status"
            }
            trap pokecon_cleanup_hook_home EXIT
          '';

          rustEnvironmentExports = ''
            export POKECON_BUILD_UV_PATH="${pkgs.uv}/bin/uv"
            export POKECON_BUILD_UV_VERSION="${pkgs.uv.version}"
            export POKECON_INTERNAL_SCRIPT_SITE_PACKAGES="${pythonEnv}/${pkgs.python314.sitePackages}"
            export PYO3_PYTHON="${pythonEnv}/bin/python"
            export POKECON_BUILD_PYTHON="${pythonEnv}/bin/python"
            export RUST_SRC_PATH="${rustToolchain}/lib/rustlib/src/rust/library"
          '';

          sanitizeGateEnvironment = ''
            while IFS= read -r -d "" ambient_entry; do
              ambient_name="''${ambient_entry%%=*}"
              case "$ambient_name" in
                PATH | PWD)
                  ;;
                *)
                  unset "$ambient_name"
                  ;;
              esac
            done < <("${pkgs.coreutils}/bin/env" -0)
            unset ambient_entry ambient_name
            export CI=1
            export CURL_CA_BUNDLE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            export GIT_CONFIG_NOSYSTEM=1
            export GIT_SSL_CAINFO="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            export LANG=C
            export LC_ALL=C
            export NIX_SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            export PIP_CONFIG_FILE=/dev/null
            export PYTHONDONTWRITEBYTECODE=1
            export PYTHONNOUSERSITE=1
            export PYTHONUTF8=1
            export REQUESTS_CA_BUNDLE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            export TZ=UTC
            export UV_NO_CONFIG=1
            umask 022
          '';

          gateHomeExports = ''
            mkdir -p \
              "$gate_home/.cache" \
              "$gate_home/.config" \
              "$gate_home/.local/state" \
              "$gate_home/runtime" \
              "$gate_home/tmp"
            chmod 0700 "$gate_home/runtime"
            export HOME="$gate_home"
            export XDG_CACHE_HOME="$gate_home/.cache"
            export XDG_CONFIG_HOME="$gate_home/.config"
            export XDG_RUNTIME_DIR="$gate_home/runtime"
            export XDG_STATE_HOME="$gate_home/.local/state"
            export TMPDIR="$gate_home/tmp"
            export BUN_INSTALL_CACHE_DIR="$XDG_CACHE_HOME/bun"
            export NPM_CONFIG_USERCONFIG=/dev/null
            export UV_CACHE_DIR="$XDG_CACHE_HOME/uv"
          '';

          canonicalizeGateHome = ''
            if [ -L "$gate_home" ] || [ ! -d "$gate_home" ]; then
              echo "gate home must be a real directory: $gate_home" >&2
              exit 2
            fi
            if ! canonical_gate_home="$(readlink -f "$gate_home")"; then
              echo "gate home cannot be resolved: $gate_home" >&2
              exit 2
            fi
            case "$canonical_gate_home" in
              /*) ;;
              *)
                echo "gate home did not resolve to an absolute path: $gate_home" >&2
                exit 2
                ;;
            esac
            gate_home="$canonical_gate_home"
            unset canonical_gate_home
          '';

          setupSourceGateEnvironment = ''
            ${sanitizeGateEnvironment}
            gate_home=
            cleanup_source_gate_home() {
              gate_status=$?
              trap - EXIT
              if [ -n "$gate_home" ]; then
                rm -rf -- "$gate_home"
              fi
              exit "$gate_status"
            }
            trap cleanup_source_gate_home EXIT
            gate_home="$(mktemp -d -t pokecon-source-gate-home.XXXXXXXX)"
            ${canonicalizeGateHome}
            ${gateHomeExports}
          '';

          preparePersistentCargoTargetDirectory = ''
            case "$task_target_name" in
              nix-editor | nix-tasks) ;;
              *)
                echo "unsupported PokeCon Cargo target name: $task_target_name" >&2
                exit 2
                ;;
            esac
            caller_dir="$(readlink -f "$caller_dir")"
            target_root="$caller_dir/target"
            if [ -L "$target_root" ]; then
              echo "refusing a symlinked Cargo target root: $target_root" >&2
              exit 2
            fi
            if [ -e "$target_root" ] && [ ! -d "$target_root" ]; then
              echo "Cargo target root is not a directory: $target_root" >&2
              exit 2
            fi
            if [ ! -e "$target_root" ]; then
              mkdir -- "$target_root"
            fi
            if [ -L "$target_root" ] || [ ! -d "$target_root" ] \
              || [ "$(readlink -f "$target_root")" != "$target_root" ]; then
              echo "Cargo target root escapes the caller worktree: $target_root" >&2
              exit 2
            fi
            cargo_target_dir="$target_root/$task_target_name"
            if [ -L "$cargo_target_dir" ]; then
              echo "refusing a symlinked Cargo task target: $cargo_target_dir" >&2
              exit 2
            fi
            if [ -e "$cargo_target_dir" ] && [ ! -d "$cargo_target_dir" ]; then
              echo "Cargo task target is not a directory: $cargo_target_dir" >&2
              exit 2
            fi
            if [ ! -e "$cargo_target_dir" ]; then
              mkdir -- "$cargo_target_dir"
            fi
            if [ -L "$cargo_target_dir" ] || [ ! -d "$cargo_target_dir" ] \
              || [ "$(readlink -f "$cargo_target_dir")" != "$cargo_target_dir" ]; then
              echo "Cargo task target escapes its worktree target root: $cargo_target_dir" >&2
              exit 2
            fi
            export CARGO_TARGET_DIR="$cargo_target_dir"
            unset cargo_target_dir target_root task_target_name
          '';

          discoverRustWorktree = ''
            ${rustEnvironmentExports}
            if ! caller_dir="$(
              "${pkgs.coreutils}/bin/env" -i \
                PATH="${
                  lib.makeBinPath [
                    pkgs.coreutils
                    pkgs.git
                  ]
                }" \
                "${pkgs.git}/bin/git" -C "$PWD" rev-parse --show-toplevel 2>/dev/null
            )"; then
              echo "Rust task must be run from a PokeCon worktree" >&2
              exit 2
            fi
            if [ -L "$caller_dir/Cargo.toml" ] || [ ! -f "$caller_dir/Cargo.toml" ]; then
              echo "PokeCon worktree root has no regular Cargo.toml: $caller_dir" >&2
              exit 2
            fi
            caller_dir="$(readlink -f "$caller_dir")"
          '';

          setupInteractiveCargoEnvironment = ''
            ${discoverRustWorktree}
            task_target_name=nix-tasks
            ${preparePersistentCargoTargetDirectory}
          '';

          acquireCargoTaskLock = ''
            task_lock="$CARGO_TARGET_DIR/.pokecon-task.lock"
            if [ -L "$task_lock" ]; then
              echo "refusing a symlinked PokeCon task lock: $task_lock" >&2
              exit 2
            fi
            if [ ! -e "$task_lock" ]; then
              (set -o noclobber; umask 022; : > "$task_lock") 2>/dev/null || true
            fi
            if [ -L "$task_lock" ] || [ ! -f "$task_lock" ] \
              || [ "$(readlink -f "$task_lock")" != "$task_lock" ]; then
              echo "PokeCon task lock is not a contained regular file: $task_lock" >&2
              exit 2
            fi
            exec 9<>"$task_lock"
            if [ ! "$task_lock" -ef /dev/fd/9 ]; then
              echo "PokeCon task lock changed while it was opened: $task_lock" >&2
              exit 2
            fi
            echo "waiting for PokeCon task lock: $task_lock" >&2
            "${pkgs.flock}/bin/flock" -x 9
            if [ -L "$task_lock" ] || [ ! -f "$task_lock" ] \
              || [ ! "$task_lock" -ef /dev/fd/9 ]; then
              echo "PokeCon task lock changed while it was held: $task_lock" >&2
              exit 2
            fi
            echo "acquired PokeCon task lock: $task_lock" >&2
          '';

          setupUvLinks = ''
            if [[ $CARGO_TARGET_DIR != /* ]] || [ -L "$CARGO_TARGET_DIR" ] \
              || [ ! -d "$CARGO_TARGET_DIR" ]; then
              echo "CARGO_TARGET_DIR must be a contained real directory: $CARGO_TARGET_DIR" >&2
              exit 2
            fi
            canonical_cargo_target="$(readlink -f "$CARGO_TARGET_DIR")"
            while IFS= read -r -d "" cargo_symlink; do
              case "$cargo_symlink" in
                "$CARGO_TARGET_DIR/debug/uv/uv" | "$CARGO_TARGET_DIR/release/uv/uv")
                  ;;
                *)
                  if ! cargo_symlink_target="$(readlink -f "$cargo_symlink")"; then
                    echo "Cargo target contains a broken symlink: $cargo_symlink" >&2
                    exit 2
                  fi
                  case "$cargo_symlink_target" in
                    "$canonical_cargo_target"/*) ;;
                    *)
                      echo "Cargo target contains an escaping symlink: $cargo_symlink -> $cargo_symlink_target" >&2
                      exit 2
                      ;;
                  esac
                  ;;
              esac
            done < <("${pkgs.findutils}/bin/find" "$CARGO_TARGET_DIR" -type l -print0)
            for cargo_profile in debug release; do
              cargo_profile_dir="$CARGO_TARGET_DIR/$cargo_profile"
              cargo_uv_dir="$cargo_profile_dir/uv"
              for cargo_directory in "$cargo_profile_dir" "$cargo_uv_dir"; do
                if [ -L "$cargo_directory" ]; then
                  echo "refusing a symlinked Cargo runtime directory: $cargo_directory" >&2
                  exit 2
                fi
                if [ -e "$cargo_directory" ] && [ ! -d "$cargo_directory" ]; then
                  echo "Cargo runtime path is not a directory: $cargo_directory" >&2
                  exit 2
                fi
                if [ ! -e "$cargo_directory" ]; then
                  mkdir -- "$cargo_directory"
                fi
                case "$(readlink -f "$cargo_directory")" in
                  "$canonical_cargo_target"/*) ;;
                  *)
                    echo "Cargo runtime directory escapes CARGO_TARGET_DIR: $cargo_directory" >&2
                    exit 2
                    ;;
                esac
              done
              cargo_uv_link="$cargo_uv_dir/uv"
              if [ -e "$cargo_uv_link" ] && [ ! -L "$cargo_uv_link" ]; then
                echo "refusing to replace a non-symlink Cargo uv launcher: $cargo_uv_link" >&2
                exit 2
              fi
              ln -sfn "${pkgs.uv}/bin/uv" "$cargo_uv_link"
              if [ "$(readlink "$cargo_uv_link")" != "${pkgs.uv}/bin/uv" ]; then
                echo "failed to install the fixed Cargo uv launcher: $cargo_uv_link" >&2
                exit 1
              fi
            done
            unset canonical_cargo_target cargo_directory cargo_profile cargo_profile_dir cargo_symlink cargo_symlink_target cargo_uv_dir cargo_uv_link
          '';

          validateMaturinDevelopWheel = ''
            "${pythonEnv}/bin/python" -I -S -c '
            import hashlib
            import sys
            import zipfile
            from pathlib import Path, PurePosixPath

            wheel = sys.argv[1]
            source_package = Path(sys.argv[2])
            with zipfile.ZipFile(wheel) as archive:
                members = [entry.filename for entry in archive.infolist()]
            if len(members) != len(set(members)):
                raise SystemExit(f"Maturin wheel contains duplicate members: {wheel}")
            paths = [PurePosixPath(member) for member in members]
            if any(
                not member
                or member.startswith("/")
                or "\\" in member
                or ".." in path.parts
                for member, path in zip(members, paths, strict=True)
            ):
                raise SystemExit(f"Maturin wheel contains an unsafe member: {wheel}")
            metadata_roots = {
                path.parts[0]
                for path in paths
                if path.parts and path.parts[0].endswith(".dist-info")
            }
            if len(metadata_roots) != 1:
                raise SystemExit(
                    f"Maturin wheel must contain one dist-info root: {sorted(metadata_roots)}"
                )
            metadata_root = next(iter(metadata_roots))
            if not (
                metadata_root.startswith("poke_controller_modified_extension-")
                and metadata_root.endswith(".dist-info")
            ):
                raise SystemExit(f"Maturin wheel has unexpected dist-info: {metadata_root}")
            required_metadata = {
                f"{metadata_root}/METADATA",
                f"{metadata_root}/RECORD",
                f"{metadata_root}/WHEEL",
                f"{metadata_root}/licenses/LICENSE",
            }
            if missing_metadata := sorted(required_metadata - set(members)):
                raise SystemExit(
                    f"Maturin wheel is missing required metadata: {missing_metadata}"
                )
            payload = [
                member
                for member, path in zip(members, paths, strict=True)
                if path.parts and path.parts[0] not in metadata_roots
            ]
            unexpected = sorted(
                member for member in payload if not member.startswith("pokecon/")
            )
            if unexpected:
                raise SystemExit(
                    f"Maturin wheel payload must live under pokecon/: {unexpected}"
                )
            if "pokecon/__init__.py" not in payload:
                raise SystemExit("Maturin wheel is missing pokecon/__init__.py")
            native_members = [
                member
                for member in payload
                if PurePosixPath(member).parent == PurePosixPath("pokecon")
                and PurePosixPath(member).name.startswith("_native.")
                and PurePosixPath(member).suffix in {".so", ".pyd"}
            ]
            if len(native_members) != 1:
                raise SystemExit(
                    f"Maturin wheel must contain one pokecon native module: {native_members}"
                )
            expected_pure = {
                f"pokecon/{path.relative_to(source_package).as_posix()}": path
                for path in source_package.rglob("*")
                if path.is_file() and path.suffix in {".py", ".pyi"}
            }
            wheel_pure = {
                member
                for member in payload
                if PurePosixPath(member).suffix in {".py", ".pyi"}
            }
            if wheel_pure != set(expected_pure):
                raise SystemExit(
                    "Maturin wheel pure-Python inventory differs from caller source: "
                    f"missing={sorted(set(expected_pure) - wheel_pure)}, "
                    f"unexpected={sorted(wheel_pure - set(expected_pure))}"
                )
            with zipfile.ZipFile(wheel) as archive:
                mismatched = sorted(
                    member
                    for member, source in expected_pure.items()
                    if hashlib.sha256(archive.read(member)).digest()
                    != hashlib.sha256(source.read_bytes()).digest()
                )
            if mismatched:
                raise SystemExit(
                    f"Maturin wheel changed caller Python payload bytes: {mismatched}"
                )
            print(
                f"validated canonical Maturin wheel layout: "
                f"{len(payload)} payload members, {native_members[0]}"
            )
            ' "$maturin_wheel" "$maturin_source/python/pokecon"
          '';

          validateMaturinVenvInventory = ''
            maturin_site_roots=(
              "''${venv_python_probe[5]}"
              "''${venv_python_probe[7]}"
            )
            for maturin_site_root in "''${maturin_site_roots[@]}"; do
              if [ -L "$maturin_site_root" ] || [ ! -d "$maturin_site_root" ]; then
                reject_maturin_venv "maturin venv $maturin_inventory_phase site-packages root is redirected or missing: $maturin_site_root"
              fi
              if ! maturin_site_symlink="$(
                "${pkgs.findutils}/bin/find" -P "$maturin_site_root" -type l -print -quit
              )"; then
                reject_maturin_venv "maturin venv $maturin_inventory_phase site-packages symlink scan failed: $maturin_site_root"
              fi
              if [ -n "$maturin_site_symlink" ]; then
                reject_maturin_venv "maturin venv $maturin_inventory_phase site-packages tree contains a symlink: $maturin_site_symlink"
              fi
            done
            unset maturin_site_root maturin_site_roots maturin_site_symlink
            if ! "$maturin_python" -I -S -c '
            import csv
            import importlib.metadata
            import re
            import stat
            import sys
            from pathlib import Path, PurePosixPath, PureWindowsPath

            phase = sys.argv[1]
            roots = sorted({Path(argument).resolve(strict=True) for argument in sys.argv[2:]})

            def contained(path: Path) -> bool:
                return any(path == root or root in path.parents for root in roots)

            forbidden = []
            for root in roots:
                forbidden.extend(root.rglob("*.pth"))
                for customization in ("sitecustomize", "usercustomize"):
                    forbidden.extend(root.glob(f"{customization}.*"))
                    package = root / customization
                    if package.exists():
                        forbidden.append(package)
            if forbidden:
                raise SystemExit(
                    "maturin venv contains executable site customization: "
                    + ", ".join(str(path) for path in sorted(set(forbidden)))
                )
            distribution_entries = []
            for root in roots:
                for distribution in importlib.metadata.distributions(path=[str(root)]):
                    name = distribution.metadata.get("Name")
                    if not name:
                        raise SystemExit(
                            f"maturin venv contains distribution without a name: {distribution}"
                        )
                    canonical_name = re.sub(r"[-_.]+", "-", name).lower()
                    distribution_entries.append(
                        (canonical_name, distribution.version, distribution)
                    )
            expected_name = "poke-controller-modified-extension"
            unexpected = sorted(
                (name, version)
                for name, version, _distribution in distribution_entries
                if name != expected_name
            )
            if unexpected:
                raise SystemExit(
                    f"maturin venv contains unexpected distributions: {unexpected}"
                )
            if len(distribution_entries) > 1 or (
                phase == "after" and len(distribution_entries) != 1
            ):
                raise SystemExit(
                    f"maturin venv {phase} inventory is not project-only: "
                    f"{sorted((name, version) for name, version, _ in distribution_entries)}"
                )

            dist_info_directories = []
            egg_info_entries = []
            uninstall_manifests = []
            for root in roots:
                for candidate in root.iterdir():
                    if candidate.name.endswith(".dist-info"):
                        dist_info_directories.append(candidate)
                egg_info_entries.extend(root.rglob("*.egg-info"))
                egg_info_entries.extend(root.rglob("*.egg-link"))
                uninstall_manifests.extend(root.rglob("RECORD"))
                uninstall_manifests.extend(root.rglob("installed-files.txt"))
            if egg_info_entries:
                raise SystemExit(
                    "maturin venv contains legacy uninstall metadata: "
                    + ", ".join(str(path) for path in sorted(set(egg_info_entries)))
                )

            expected_dist_info = []
            expected_records = []
            for name, version, distribution in distribution_entries:
                distribution_path = getattr(distribution, "_path", None)
                if distribution_path is None:
                    raise SystemExit(
                        f"maturin venv distribution has no concrete metadata path: {(name, version)}"
                    )
                dist_info = Path(distribution_path)
                try:
                    dist_info_status = dist_info.lstat()
                except OSError as error:
                    raise SystemExit(
                        f"maturin venv dist-info cannot be inspected: {dist_info}: {error}"
                    ) from error
                if not stat.S_ISDIR(dist_info_status.st_mode):
                    raise SystemExit(
                        f"maturin venv dist-info is not a real directory: {dist_info}"
                    )
                if not dist_info.name.startswith(
                    "poke_controller_modified_extension-"
                ) or not dist_info.name.endswith(".dist-info"):
                    raise SystemExit(
                        f"maturin venv project has unexpected dist-info path: {dist_info}"
                    )
                if dist_info.parent not in roots or not contained(
                    dist_info.resolve(strict=True)
                ):
                    raise SystemExit(
                        f"maturin venv dist-info escapes site-packages: {dist_info}"
                    )
                record = dist_info / "RECORD"
                try:
                    record_status = record.lstat()
                except OSError as error:
                    raise SystemExit(
                        f"maturin venv project RECORD cannot be inspected: {record}: {error}"
                    ) from error
                if not stat.S_ISREG(record_status.st_mode):
                    raise SystemExit(
                        f"maturin venv project RECORD is not a real regular file: {record}"
                    )

                seen_record_paths = set()
                try:
                    with record.open(encoding="utf-8", newline="") as stream:
                        rows = list(csv.reader(stream, strict=True))
                except (OSError, UnicodeError, csv.Error) as error:
                    raise SystemExit(
                        f"maturin venv project RECORD cannot be parsed: {record}: {error}"
                    ) from error
                for row in rows:
                    if len(row) != 3:
                        raise SystemExit(
                            f"maturin venv project RECORD has a malformed row: {row!r}"
                        )
                    raw_path = row[0]
                    posix_path = PurePosixPath(raw_path)
                    windows_path = PureWindowsPath(raw_path)
                    if (
                        not raw_path
                        or "\0" in raw_path
                        or "\\" in raw_path
                        or posix_path.is_absolute()
                        or windows_path.is_absolute()
                        or windows_path.drive
                        or ".." in posix_path.parts
                        or not posix_path.parts
                        or posix_path.parts[0] not in {"pokecon", dist_info.name}
                    ):
                        raise SystemExit(
                            f"maturin venv project RECORD has an unsafe path: {raw_path!r}"
                        )
                    normalized_path = posix_path.as_posix()
                    if normalized_path in seen_record_paths:
                        raise SystemExit(
                            f"maturin venv project RECORD repeats a path: {normalized_path!r}"
                        )
                    seen_record_paths.add(normalized_path)
                    lexical_target = dist_info.parent.joinpath(*posix_path.parts)
                    if not contained(lexical_target):
                        raise SystemExit(
                            f"maturin venv project RECORD path escapes lexically: {raw_path!r}"
                        )
                    try:
                        target_status = lexical_target.lstat()
                    except OSError as error:
                        raise SystemExit(
                            "maturin venv project RECORD target cannot be inspected: "
                            f"{lexical_target}: {error}"
                        ) from error
                    if not stat.S_ISREG(target_status.st_mode):
                        raise SystemExit(
                            "maturin venv project RECORD target is not a real regular file: "
                            f"{lexical_target}"
                        )
                    real_target = lexical_target.resolve(strict=False)
                    if not contained(real_target):
                        raise SystemExit(
                            f"maturin venv project RECORD path escapes after resolution: {raw_path!r}"
                        )
                expected_dist_info.append(dist_info)
                expected_records.append(record)

            if sorted(dist_info_directories) != sorted(expected_dist_info):
                raise SystemExit(
                    "maturin venv dist-info directory inventory differs from distributions: "
                    f"found={sorted(dist_info_directories)}, "
                    f"expected={sorted(expected_dist_info)}"
                )
            if sorted(set(uninstall_manifests)) != sorted(expected_records) or len(
                uninstall_manifests
            ) != len(expected_records):
                raise SystemExit(
                    "maturin venv uninstall manifest inventory is not canonical: "
                    f"found={sorted(uninstall_manifests)}, "
                    f"expected={sorted(expected_records)}"
                )

            identities = sorted(
                (name, version) for name, version, _distribution in distribution_entries
            )
            print(f"validated maturin venv {phase} inventory: {identities}")
            ' \
              "$maturin_inventory_phase" \
              "''${venv_python_probe[5]}" \
              "''${venv_python_probe[7]}"; then
              reject_maturin_venv "maturin venv $maturin_inventory_phase inventory validation failed"
            fi
          '';

          setupIsolatedCargoHome = ''
            export CARGO_HOME="$gate_home/cargo-home"
            mkdir -p "$CARGO_HOME"
            ln -s "${gateCargoConfig}" "$CARGO_HOME/config.toml"
          '';

          setupPerRunCargoTarget = ''
            if [ -z "''${gate_home:-}" ] || [ -L "$gate_home" ] || [ ! -d "$gate_home" ]; then
              echo "per-run Cargo target requires a real gate home: ''${gate_home:-<unset>}" >&2
              exit 2
            fi
            canonical_gate_home="$(readlink -f "$gate_home")"
            if [ "$canonical_gate_home" != "$gate_home" ]; then
              echo "per-run Cargo gate home is not canonical: $gate_home" >&2
              exit 2
            fi
            cargo_target_dir="$gate_home/cargo-target"
            if [ -e "$cargo_target_dir" ] || [ -L "$cargo_target_dir" ]; then
              echo "per-run Cargo target unexpectedly exists: $cargo_target_dir" >&2
              exit 2
            fi
            mkdir -- "$cargo_target_dir"
            if [ -L "$cargo_target_dir" ] || [ ! -d "$cargo_target_dir" ]; then
              echo "per-run Cargo target is not a real directory: $cargo_target_dir" >&2
              exit 2
            fi
            case "$(readlink -f "$cargo_target_dir")" in
              "$canonical_gate_home"/*) ;;
              *)
                echo "per-run Cargo target escapes its gate home: $cargo_target_dir" >&2
                exit 2
                ;;
            esac
            export CARGO_TARGET_DIR="$cargo_target_dir"
            echo "using isolated per-run Cargo target: $CARGO_TARGET_DIR" >&2
            unset canonical_gate_home cargo_target_dir
          '';

          setupCallerRustTaskEnvironment = ''
            ${discoverRustWorktree}
            gate_home=
            cleanup_caller_rust_task_home() {
              gate_status=$?
              trap - EXIT
              if [ -n "$gate_home" ]; then
                rm -rf -- "$gate_home"
              fi
              exit "$gate_status"
            }
            trap cleanup_caller_rust_task_home EXIT
            gate_home="$(mktemp -d -t pokecon-caller-rust-home.XXXXXXXX)"
            ${canonicalizeGateHome}
            ${gateHomeExports}
            ${setupIsolatedCargoHome}
            ${setupPerRunCargoTarget}
            ${setupUvLinks}
          '';

          setupWorkdir = ''
            ${sanitizeGateEnvironment}
            ${discoverRustWorktree}
            workdir=
            gate_home=
            cleanup_gate_directories() {
              gate_status=$?
              trap - EXIT
              if [ -n "$workdir" ]; then
                rm -rf -- "$workdir"
              fi
              if [ -n "$gate_home" ]; then
                rm -rf -- "$gate_home"
              fi
              exit "$gate_status"
            }
            trap cleanup_gate_directories EXIT
            gate_home="$(mktemp -d -t pokecon-rust-gate-home.XXXXXXXX)"
            ${canonicalizeGateHome}
            ${gateHomeExports}
            ${setupIsolatedCargoHome}
            ${setupPerRunCargoTarget}
            ${setupUvLinks}
            workdir="$(mktemp -d)"
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
            pkgs.flock
            pkgs.findutils
            pkgs.git
            pkgs.gnumake
            pkgs.gnugrep
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
          apiBunDependencies = pkgs.stdenvNoCC.mkDerivation {
            pname = "pokecon-api-bun-dependencies";
            version = workspaceVersion;
            src = source;
            sourceRoot = "pokecon-source/api";
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
            outputHash = "sha256-EBHI+6ewrngG4N4bx0HL/34Bb4bH4C3o99MJvs/KMsk=";
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
              "pokecon"
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
              "pokecon"
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
          gateCargoConfig = pkgs.writeText "pokecon-gate-cargo-config.toml" ''
            [source.crates-io]
            replace-with = "vendored-sources"

            [source.vendored-sources]
            directory = "${pokeconPackage.cargoDeps}"

            [net]
            offline = true
          '';
          cliHelpCheck = mkTask {
            name = "cli-help-check";
            runtimeInputs = [ pkgs.diffutils ];
            text = ''
              if [ "$#" -ne 0 ]; then
                echo "usage: nix run .#cli-help-check" >&2
                exit 2
              fi
              ${setupSourceGateEnvironment}
              mkdir -p "$gate_home/.local/share"
              export XDG_DATA_HOME="$gate_home/.local/share"
              cli_help_output="$gate_home/cli-help"
              mkdir -p "$cli_help_output"

              check_cli_help() {
                local binary fixture label stderr_file stdout_file status
                binary=$1
                fixture=$2
                label=$3
                stdout_file="$cli_help_output/$label.stdout"
                stderr_file="$cli_help_output/$label.stderr"

                if "$binary" --help >"$stdout_file" 2>"$stderr_file"; then
                  status=0
                else
                  status=$?
                fi
                if [ "$status" -ne 0 ]; then
                  echo "$label --help exited with status $status" >&2
                  if [ -s "$stderr_file" ]; then
                    echo "$label --help stderr:" >&2
                    cat "$stderr_file" >&2
                  fi
                  return 1
                fi
                if [ -s "$stderr_file" ]; then
                  echo "$label --help wrote unexpected stderr:" >&2
                  cat "$stderr_file" >&2
                  return 1
                fi
                if ! diff -u \
                  --label "$label.expected" \
                  --label "$label.actual" \
                  "$fixture" \
                  "$stdout_file" >&2; then
                  echo "$label --help stdout differs from its tracked fixture" >&2
                  return 1
                fi
              }

              check_cli_help \
                "${self'.packages.pokecon}/bin/pokecon" \
                "${source}/tests/fixtures/cli-help/pokecon.txt" \
                pokecon
              check_cli_help \
                "${self'.packages.pokecon}/bin/pokecon-worker" \
                "${source}/tests/fixtures/cli-help/pokecon-worker.txt" \
                pokecon-worker
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

          formatter = safeFormatter;

          checks.pokecon = pokeconPackage;
          checks.web = webPackage;

          apps = {
            default = mkApp "${self'.packages.pokecon}/bin/pokecon";
            fmt = mkApp "${safeFormatter}/bin/pokecon-format";
            cli-help-check = cliHelpCheck;

            cargo = mkTask {
              name = "cargo";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.cargo-tauri
                pkgs.git
                pkgs.uv
              ];
              text = ''
                if ! repo_root="$(
                  "${pkgs.coreutils}/bin/env" -i \
                    PATH="${
                      lib.makeBinPath [
                        pkgs.coreutils
                        pkgs.git
                      ]
                    }" \
                    "${pkgs.git}/bin/git" -C "$PWD" rev-parse --show-toplevel
                )"; then
                  echo "cargo must be run from a PokeCon worktree" >&2
                  exit 2
                fi
                if [ ! -f "$repo_root/Cargo.toml" ]; then
                  echo "Cargo.toml is missing from worktree root: $repo_root" >&2
                  exit 2
                fi
                cd "$repo_root"
                ${setupInteractiveCargoEnvironment}
                ${acquireCargoTaskLock}
                ${setupUvLinks}
                ${desktopEnvironment}
                "${rustToolchain}/bin/cargo" "$@"
              '';
            };

            web-dev = mkTask {
              name = "web-dev";
              runtimeInputs = [
                bun
                pkgs.findutils
                pkgs.git
              ];
              text = ''
                if ! repo_root="$(
                  "${pkgs.coreutils}/bin/env" -i \
                    PATH="${
                      lib.makeBinPath [
                        pkgs.coreutils
                        pkgs.git
                      ]
                    }" \
                    "${pkgs.git}/bin/git" -C "$PWD" rev-parse --show-toplevel
                )"; then
                  echo "web-dev must be run from a PokeCon worktree" >&2
                  exit 2
                fi
                repo_root="$(readlink -f "$repo_root")"
                web_dir="$repo_root/web"
                if [ -L "$web_dir" ] || [ ! -d "$web_dir" ] \
                  || [ "$(readlink -f "$web_dir")" != "$web_dir" ]; then
                  echo "web package directory must stay inside the worktree: $web_dir" >&2
                  exit 2
                fi
                if [ ! -f "$web_dir/package.json" ] || [ ! -f "$web_dir/bun.lock" ]; then
                  echo "web package is missing from the PokeCon worktree: $web_dir" >&2
                  exit 2
                fi
                validate_web_directory() {
                  web_path=$1
                  web_label=$2
                  if [ -L "$web_path" ]; then
                    echo "web-dev refuses a symlinked $web_label: $web_path" >&2
                    exit 2
                  fi
                  if [ -e "$web_path" ] && [ ! -d "$web_path" ]; then
                    echo "web $web_label is not a directory: $web_path" >&2
                    exit 2
                  fi
                  if [ -d "$web_path" ] \
                    && [ "$(readlink -f "$web_path")" != "$web_path" ]; then
                    echo "web $web_label escapes its fixed worktree path: $web_path" >&2
                    exit 2
                  fi
                }
                validate_web_tree_symlinks() {
                  web_tree=$1
                  web_label=$2
                  if [ ! -d "$web_tree" ]; then
                    return
                  fi
                  canonical_web_tree="$(readlink -f "$web_tree")"
                  while IFS= read -r -d "" web_symlink; do
                    if ! web_symlink_target="$(readlink -f "$web_symlink")"; then
                      echo "web $web_label contains a broken symlink: $web_symlink" >&2
                      exit 2
                    fi
                    case "$web_symlink_target" in
                      "$canonical_web_tree" | "$canonical_web_tree"/*) ;;
                      *)
                        echo "web $web_label contains an escaping symlink: $web_symlink -> $web_symlink_target" >&2
                        exit 2
                        ;;
                    esac
                  done < <("${pkgs.findutils}/bin/find" "$web_tree" -type l -print0)
                }
                validate_web_boundaries() {
                  validate_web_directory "$web_dir/node_modules" node_modules
                  validate_web_tree_symlinks "$web_dir/node_modules" node_modules
                  validate_web_directory "$web_dir/.svelte-kit" .svelte-kit
                  validate_web_tree_symlinks "$web_dir/.svelte-kit" .svelte-kit
                  validate_web_directory "$web_dir/dist" dist
                  validate_web_tree_symlinks "$web_dir/dist" dist
                }
                validate_web_boundaries
                if [ "''${1:-}" = "--help" ]; then
                  if [ "$#" -ne 1 ]; then
                    echo "usage: nix run .#web-dev [-- vite-options]" >&2
                    exit 2
                  fi
                  echo "usage: nix run .#web-dev [-- vite-options]"
                  echo "web root: $web_dir"
                  exit 0
                fi
                web_node_modules="$web_dir/node_modules"
                if [ ! -e "$web_node_modules" ]; then
                  mkdir -- "$web_node_modules"
                fi
                validate_web_directory "$web_node_modules" node_modules
                for web_cache_directory in \
                  "$web_node_modules/.cache" \
                  "$web_node_modules/.cache/bun"; do
                  validate_web_directory "$web_cache_directory" "Bun cache directory"
                  if [ ! -e "$web_cache_directory" ]; then
                    mkdir -- "$web_cache_directory"
                  fi
                  validate_web_directory "$web_cache_directory" "Bun cache directory"
                done
                export BUN_INSTALL_CACHE_DIR="$web_node_modules/.cache/bun"
                cd "$web_dir"
                "${bun}/bin/bun" install --frozen-lockfile --ignore-scripts --no-progress
                validate_web_boundaries
                validate_web_directory "$BUN_INSTALL_CACHE_DIR" "Bun cache directory"
                exec "${bun}/bin/bun" --bun ./node_modules/vite/bin/vite.js "$@"
              '';
            };

            hooks-install = mkTask {
              name = "hooks-install";
              # Keep upstream installation semantics, then harden only the
              # generated hook shape that is validated below.
              excludeShellChecks = [
                "SC2006"
                "SC2043"
                "SC2086"
                "SC2157"
                "SC2221"
                "SC2222"
                "SC2295"
              ];
              runtimeInputs = [
                pkgs.git
                pkgs.gnugrep
                pkgs.gnused
                pkgs.nix
              ];
              text = ''
                if [ "$#" -ne 0 ]; then
                  echo "usage: nix run .#hooks-install" >&2
                  exit 2
                fi
                ${setupSourceGateEnvironment}
                if ! repo_root="$("${pkgs.git}/bin/git" rev-parse --show-toplevel)"; then
                  echo "hooks-install must be run from a Git worktree" >&2
                  exit 2
                fi
                repo_root="$(readlink -f "$repo_root")"
                git_common_dir="$("${pkgs.git}/bin/git" rev-parse --git-common-dir)"
                case "$git_common_dir" in
                  /*) ;;
                  *) git_common_dir="$repo_root/$git_common_dir" ;;
                esac
                if [ -L "$git_common_dir" ] || [ ! -d "$git_common_dir" ]; then
                  echo "Git common directory must be a real directory: $git_common_dir" >&2
                  exit 2
                fi
                git_common_dir="$(readlink -f "$git_common_dir")"
                hook_path="$("${pkgs.git}/bin/git" rev-parse --git-path hooks/pre-commit)"
                case "$hook_path" in
                  /*) ;;
                  *) hook_path="$repo_root/$hook_path" ;;
                esac
                if [ -L "$hook_path" ]; then
                  echo "refusing to replace unexpected hook symlink: $hook_path -> $(readlink "$hook_path")" >&2
                  exit 2
                fi
                canonical_hook_path="$(readlink -m "$hook_path")"
                case "$canonical_hook_path" in
                  "$repo_root"/* | "$git_common_dir"/*) ;;
                  *)
                    echo "refusing a hook path outside the worktree and Git common directory: $hook_path" >&2
                    exit 2
                    ;;
                esac
                hook_parent="$(dirname "$hook_path")"
                if [ -L "$hook_parent" ] || [ ! -d "$hook_parent" ] \
                  || [ "$(readlink -f "$hook_parent")/pre-commit" != "$canonical_hook_path" ]; then
                  echo "refusing a redirected or missing hook directory: $hook_parent" >&2
                  exit 2
                fi
                hook_path="$canonical_hook_path"
                validate_recognized_hook() {
                  candidate_hook=$1
                  if [ ! -f "$candidate_hook" ]; then
                    return 1
                  fi
                  grep -Eq '^#!/nix/store/[a-z0-9]+-bash-[^/]*/bin/bash( -p)?$' "$candidate_hook" || return 1
                  grep -Fqx '# File generated by pre-commit: https://pre-commit.com' "$candidate_hook" || return 1
                  grep -Eq '^# ID: [0-9a-f]+$' "$candidate_hook" || return 1
                  if grep -Eq '^exec /nix/store/[a-z0-9]+-pre-commit-[^/]*/bin/pre-commit "\$\{ARGS\[@\]\}"$' "$candidate_hook"; then
                    return 0
                  fi
                  grep -Eq '^"/nix/store/[a-z0-9]+-coreutils-[^/]*/bin/env" -i "\$\{pokecon_hook_environment\[@\]\}" /nix/store/[a-z0-9]+-pre-commit-[^/]*/bin/pre-commit "\$\{ARGS\[@\]\}"$' "$candidate_hook"
                }
                validate_current_hook() {
                  candidate_hook=$1
                  validate_recognized_hook "$candidate_hook" || return 1
                  IFS= read -r candidate_shebang < "$candidate_hook"
                  case "$candidate_shebang" in
                    "#!${pkgs.bashNonInteractive}/bin/bash" | "#!${pkgs.bashNonInteractive}/bin/bash -p") ;;
                    *) return 1 ;;
                  esac
                }
                validate_hardened_hook() {
                  candidate_hook=$1
                  [ -x "$candidate_hook" ] \
                    && validate_current_hook "$candidate_hook" \
                    && grep -Fxq '#!${pkgs.bashNonInteractive}/bin/bash -p' "$candidate_hook" \
                    && grep -Fqx '# POKECON_HARDENED_PRE_COMMIT_V1' "$candidate_hook" \
                    && grep -Eq '^"${pkgs.coreutils}/bin/env" -i "\$\{pokecon_hook_environment\[@\]\}" /nix/store/[a-z0-9]+-pre-commit-[^/]*/bin/pre-commit "\$\{ARGS\[@\]\}"$' "$candidate_hook"
                }
                if [ -L "$hook_path" ]; then
                  echo "refusing to replace unexpected hook symlink: $hook_path -> $(readlink "$hook_path")" >&2
                  exit 2
                fi
                if [ -e "$hook_path" ] && ! validate_recognized_hook "$hook_path"; then
                  echo "refusing to replace unexpected pre-commit hook: $hook_path" >&2
                  exit 2
                fi
                config_path="$repo_root/${config.pre-commit.settings.configPath}"
                if [ -L "$config_path" ]; then
                  generated_config="$(readlink "$config_path")"
                  case "$generated_config" in
                    /nix/store/*-pre-commit-config.json)
                      unlink -- "$config_path"
                      ;;
                    *)
                      echo "refusing to replace unexpected config symlink: $config_path -> $generated_config" >&2
                      exit 2
                      ;;
                  esac
                elif [ -e "$config_path" ]; then
                  echo "refusing to replace non-symlink config: $config_path" >&2
                  exit 2
                fi
                cd "$repo_root"
                ${config.pre-commit.installationScript}
                if ! validate_current_hook "$hook_path"; then
                  echo "upstream installer produced an unexpected pre-commit hook: $hook_path" >&2
                  exit 1
                fi
                sed -i '1c #!${pkgs.bashNonInteractive}/bin/bash -p' "$hook_path"
                if ! grep -Fqx '# POKECON_HARDENED_PRE_COMMIT_V1' "$hook_path"; then
                  sed -i '/^# end templated$/r ${hookHardeningSnippet}' "$hook_path"
                fi
                sed -i '$s|^exec |"${pkgs.coreutils}/bin/env" -i "''${pokecon_hook_environment[@]}" |' "$hook_path"
                if ! validate_hardened_hook "$hook_path"; then
                  echo "failed to harden generated pre-commit hook: $hook_path" >&2
                  exit 1
                fi
              '';
            };

            editor = mkTask {
              name = "editor";
              runtimeInputs = rustTaskInputs ++ [
                bun
                pkgs.basedpyright
                pkgs.git
                pkgs.rust-analyzer
                pkgs.svelte-language-server
                pkgs.typescript-language-server
                pkgs.uv
              ];
              text = ''
                if ! editor_root="$(
                  "${pkgs.coreutils}/bin/env" -i \
                    PATH="${
                      lib.makeBinPath [
                        pkgs.coreutils
                        pkgs.git
                      ]
                    }" \
                    "${pkgs.git}/bin/git" -C "$PWD" rev-parse --show-toplevel
                )"; then
                  echo "editor must be run from a PokeCon worktree" >&2
                  exit 2
                fi
                caller_dir="$editor_root"
                task_target_name=nix-editor
                ${preparePersistentCargoTargetDirectory}
                ${rustEnvironmentExports}
                ${setupUvLinks}
                ${desktopEnvironment}
                export RUST_ANALYZER_PATH="${pkgs.rust-analyzer}/bin/rust-analyzer"
                export BASEDPYRIGHT_LANGSERVER_PATH="${pkgs.basedpyright}/bin/basedpyright-langserver"
                export TYPESCRIPT_LANGUAGE_SERVER_PATH="${pkgs.typescript-language-server}/bin/typescript-language-server"
                export SVELTE_LANGUAGE_SERVER_PATH="${pkgs.svelte-language-server}/bin/svelteserver"

                print_paths() {
                  printf '%s\n' \
                    "rust-analyzer=$RUST_ANALYZER_PATH" \
                    "python=${pythonEnv}/bin/python" \
                    "basedpyright-langserver=$BASEDPYRIGHT_LANGSERVER_PATH" \
                    "typescript-language-server=$TYPESCRIPT_LANGUAGE_SERVER_PATH" \
                    "svelteserver=$SVELTE_LANGUAGE_SERVER_PATH"
                }

                case "''${1:-}" in
                  "" | --print)
                    print_paths
                    exit 0
                    ;;
                  --help)
                    echo "usage: nix run .#editor [-- --print | -- /absolute/path/to/editor [arguments...]]"
                    exit 0
                    ;;
                  --)
                    shift
                    ;;
                esac

                if [ "$#" -eq 0 ]; then
                  echo "editor command is required" >&2
                  exit 2
                fi
                if [[ $1 != /* ]]; then
                  echo "editor command must be an absolute executable path: $1" >&2
                  exit 2
                fi
                if [ ! -x "$1" ]; then
                  echo "editor command is not executable: $1" >&2
                  exit 2
                fi
                exec "$@"
              '';
            };

            editor-smoke = mkTask {
              name = "editor-smoke";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.basedpyright
                pkgs.rust-analyzer
                pkgs.svelte-language-server
                pkgs.typescript-language-server
              ];
              text = ''
                if [ "$#" -ne 0 ]; then
                  echo "usage: nix run .#editor-smoke" >&2
                  exit 2
                fi
                ${setupSourceGateEnvironment}
                export CARGO_TARGET_DIR="$gate_home/editor-cargo-target"
                mkdir -p "$CARGO_TARGET_DIR"
                ${rustEnvironmentExports}
                ${setupIsolatedCargoHome}
                ${setupUvLinks}
                ${desktopEnvironment}
                fixture_root="$gate_home/editor-smoke"
                mkdir -p "$fixture_root"
                cp -R "${webBunDependencies}/node_modules" "$fixture_root/"
                chmod -R u+w "$fixture_root/node_modules"
                "${pythonEnv}/bin/python" "${source}/scripts/integration/editor_lsp_smoke.py" \
                  --fixture-root "$fixture_root" \
                  --rust-analyzer "${pkgs.rust-analyzer}/bin/rust-analyzer" \
                  --basedpyright "${pkgs.basedpyright}/bin/basedpyright-langserver" \
                  --typescript-language-server "${pkgs.typescript-language-server}/bin/typescript-language-server" \
                  --svelte-language-server "${pkgs.svelte-language-server}/bin/svelteserver" \
                  --python "${pythonEnv}/bin/python" \
                  --timeout-seconds 180
              '';
            };

            ci-watch = mkTask {
              name = "ci-watch";
              runtimeInputs = [
                pkgs.gh
                pkgs.git
                pkgs.jq
              ];
              text = ''
                if ! repo_root="$(
                  "${pkgs.coreutils}/bin/env" -i \
                    PATH="${
                      lib.makeBinPath [
                        pkgs.coreutils
                        pkgs.git
                      ]
                    }" \
                    "${pkgs.git}/bin/git" -C "$PWD" rev-parse --show-toplevel
                )"; then
                  echo "ci-watch must be run from a PokeCon worktree" >&2
                  exit 2
                fi
                while IFS= read -r -d "" ci_watch_entry; do
                  ci_watch_name="''${ci_watch_entry%%=*}"
                  case "$ci_watch_name" in
                    GIT_*) unset "$ci_watch_name" ;;
                  esac
                done < <("${pkgs.coreutils}/bin/env" -0)
                unset ci_watch_entry ci_watch_name
                cd "$repo_root"
                exec "${pkgs.bash}/bin/bash" "${source}/scripts/ci-watch.sh" "$@"
              '';
            };

            workspace-lock-check = mkTask {
              name = "workspace-lock-check";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.git
                pkgs.uv
              ];
              text = ''
                ${sanitizeGateEnvironment}
                if ! repo_root="$("${pkgs.git}/bin/git" rev-parse --show-toplevel)"; then
                  echo "workspace-lock-check must be run from a PokeCon worktree" >&2
                  exit 2
                fi
                cd "$repo_root"
                ${setupCallerRustTaskEnvironment}
                "${pkgs.bash}/bin/bash" "${source}/scripts/quality/check-workspace-lock.sh" "$@"
              '';
            };

            actionlint = mkTask {
              name = "actionlint";
              runtimeInputs = [ pkgs.actionlint ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                if [ "$#" -eq 0 ]; then
                  actionlint .github/workflows/*.yml
                else
                  actionlint "$@"
                fi
              '';
            };

            acceptance-record-check = mkTask {
              name = "acceptance-record-check";
              runtimeInputs = [
                pythonEnv
                pkgs.check-jsonschema
              ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                export PYTHONDONTWRITEBYTECODE=1
                python -m scripts.acceptance.records "$@"
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
                  pkgs.gnused
                  pkgs.kmod
                  pkgs.v4l-utils
                ];
              text = ''
                virtual_io_sudo_is_set=false
                virtual_io_sudo=
                if [[ -v POKECON_SUDO ]]; then
                  virtual_io_sudo_is_set=true
                  virtual_io_sudo=$POKECON_SUDO
                fi
                ${setupWorkdir}
                if [[ $virtual_io_sudo_is_set == true ]]; then
                  export POKECON_SUDO="$virtual_io_sudo"
                fi
                unset virtual_io_sudo virtual_io_sudo_is_set
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
                pkgs.diffutils
                pkgs.shellcheck
              ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD/python:$PWD"
                cargo run --locked --package pokecon --bin generate_contracts --features contract-generator -- --check
                cargo test --locked --package pokecon --test contract_sync
                check-jsonschema --check-metaschema generated/settings.schema.json
                python -m scripts.acceptance.records
                export POKECON_API_NODE_MODULES="${apiBunDependencies}/node_modules"
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
                ${sanitizeGateEnvironment}
                if ! repo_root="$("${pkgs.git}/bin/git" rev-parse --show-toplevel)"; then
                  echo "generate-contracts must be run from a PokeCon worktree" >&2
                  exit 2
                fi
                cd "$repo_root"
                ${setupCallerRustTaskEnvironment}
                ${desktopEnvironment}
                export PYO3_PYTHON="${pythonEnv}/bin/python"
                cargo run --locked --package pokecon --bin generate_contracts --features contract-generator -- "$@"
              '';
            };

            compatibility-inventory = mkTask {
              name = "compatibility-inventory";
              runtimeInputs = [
                pythonEnv
                pkgs.git
              ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                python -m scripts.compatibility.inventory --check "$@"
              '';
            };

            compatibility = mkTask {
              name = "compatibility";
              runtimeInputs = rustTaskInputs ++ [ pkgs.git ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD"
                cargo build --locked --jobs 1 --package pokecon-worker --bin pokecon-worker --bin pokecon-compatibility
                python -m scripts.compatibility.promote --check
                python -m scripts.compatibility.runner \
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
                ${sanitizeGateEnvironment}
                if ! repo_root="$("${pkgs.git}/bin/git" rev-parse --show-toplevel)"; then
                  echo "compatibility-roll must be run from a PokeCon worktree" >&2
                  exit 2
                fi
                cd "$repo_root"
                ${setupCallerRustTaskEnvironment}
                ${desktopEnvironment}
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD"
                cargo build --locked --jobs 1 --package pokecon-worker --bin pokecon-worker --bin pokecon-compatibility
                python -m scripts.compatibility.roll \
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
                ${setupSourceGateEnvironment}
                cd "${source}"
                python -m scripts.quality.source_guard "$@"
              '';
            };

            source-filter-check = mkTask {
              name = "source-filter-check";
              runtimeInputs = [
                pythonEnv
                pkgs.git
              ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                python -m scripts.quality.source_filter "$@"
              '';
            };

            release-check = mkTask {
              name = "release-check";
              runtimeInputs = [ pythonEnv ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                python -m scripts.release.gate "$@"
              '';
            };

            ruff-check = mkTask {
              name = "ruff-check";
              runtimeInputs = [
                pythonEnv
                pkgs.ripgrep
              ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                python_files=("$@")
                if [ "''${#python_files[@]}" -eq 0 ]; then
                  mapfile -t python_files < <(rg --files python scripts tests -g '*.py' -g '*.pyi')
                fi
                ruff check --config ruff.toml --no-cache "''${python_files[@]}"
              '';
            };

            ruff-format = mkTask {
              name = "ruff-format";
              runtimeInputs = [
                pythonEnv
                pkgs.git
              ];
              text = ''
                ${setupSourceGateEnvironment}
                if ! repo_root="$("${pkgs.git}/bin/git" rev-parse --show-toplevel)"; then
                  echo "ruff-format must be run from a PokeCon worktree" >&2
                  exit 2
                fi
                cd "$repo_root"
                python_files=("$@")
                if [ "''${#python_files[@]}" -eq 0 ]; then
                  tracked_files=()
                  mapfile -d "" -t tracked_files < <("${pkgs.git}/bin/git" ls-files -z -- python scripts tests)
                  for tracked_file in "''${tracked_files[@]}"; do
                    case "$tracked_file" in
                      *.py | *.pyi) python_files+=("$tracked_file") ;;
                    esac
                  done
                fi
                for python_file in "''${python_files[@]}"; do
                  case "$python_file" in
                    /* | .. | ../* | */../* | -*)
                      echo "ruff-format accepts only relative tracked Python paths: $python_file" >&2
                      exit 2
                      ;;
                    *.py | *.pyi) ;;
                    *)
                      echo "ruff-format accepts only .py and .pyi files: $python_file" >&2
                      exit 2
                      ;;
                  esac
                  if [ -L "$python_file" ] || [ ! -f "$python_file" ] \
                    || ! "${pkgs.git}/bin/git" ls-files --error-unmatch -- "$python_file" >/dev/null; then
                    echo "ruff-format path must be a tracked regular file: $python_file" >&2
                    exit 2
                  fi
                done
                ruff format --config ruff.toml --no-cache -- "''${python_files[@]}"
              '';
            };

            ruff-format-check = mkTask {
              name = "ruff-format-check";
              runtimeInputs = [
                pythonEnv
                pkgs.ripgrep
              ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                python_files=("$@")
                if [ "''${#python_files[@]}" -eq 0 ]; then
                  mapfile -t python_files < <(rg --files python scripts tests -g '*.py' -g '*.pyi')
                fi
                ruff format --config ruff.toml --no-cache --check "''${python_files[@]}"
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
                ${setupSourceGateEnvironment}
                cd "${source}"
                export PYTHONDONTWRITEBYTECODE=1
                bun --bun "${basedpyrightCli}"
              '';
            };

            test = mkTask {
              name = "test";
              runtimeInputs = [
                pythonEnv
                pkgs.check-jsonschema
                pkgs.git
              ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="${source}/python:${source}"
                python -m pytest -p no:cacheprovider tests -v --tb=short
              '';
            };

            maturin-develop = mkTask {
              name = "maturin-develop";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.maturin
                pkgs.uv
              ];
              text = ''
                                ${sanitizeGateEnvironment}
                                if ! repo_root="$("${pkgs.git}/bin/git" rev-parse --show-toplevel)"; then
                                  echo "maturin-develop must be run from a PokeCon worktree" >&2
                                  exit 2
                                fi
                                repo_root="$(readlink -f "$repo_root")"
                                cd "$repo_root"
                                maturin_target_root="$repo_root/target"
                                maturin_venv="$maturin_target_root/maturin-venv"
                                maturin_python="$maturin_venv/bin/python"
                                reject_maturin_venv() {
                                  echo "$1" >&2
                                  echo "move $maturin_venv aside, then rerun nix run .#maturin-develop" >&2
                                  exit 2
                                }
                                if [ "''${1:-}" = "--help" ]; then
                                  if [ "$#" -ne 1 ]; then
                                    echo "--help does not accept additional arguments" >&2
                                    exit 2
                                  fi
                                  echo "usage: nix run .#maturin-develop [-- safe-build-options]"
                                  echo "safe options: --release, --strip, --features, --all-features, --no-default-features, --jobs, --profile, --timings, --future-incompat-report, --ignore-rust-version, --quiet, --verbose, --color, --compression-method, --compression-level"
                                  exit 0
                                fi
                                maturin_arguments=()
                                while [ "$#" -gt 0 ]; do
                                  case "$1" in
                                    -r | --release | --strip | --all-features | --no-default-features | --timings | --future-incompat-report | --ignore-rust-version | -q | --quiet | -v | -vv* | --verbose)
                                      maturin_arguments+=("$1")
                                      shift
                                      ;;
                                    -F | --features | -j | --jobs | --profile | --color | --compression-method | --compression-level)
                                      if [ "$#" -lt 2 ] || [[ $2 == -* ]]; then
                                        echo "maturin-develop option requires a value: $1" >&2
                                        exit 2
                                      fi
                                      maturin_arguments+=("$1" "$2")
                                      shift 2
                                      ;;
                                    -F?* | -j?* | --features=* | --jobs=* | --profile=* | --color=* | --compression-method=* | --compression-level=*)
                                      maturin_arguments+=("$1")
                                      shift
                                      ;;
                                    *)
                                      echo "maturin-develop rejects an unsupported or contract-overriding option: $1" >&2
                                      echo "run nix run .#maturin-develop -- --help for the safe option list" >&2
                                      exit 2
                                      ;;
                                  esac
                                done
                                if [ -L "$maturin_target_root" ]; then
                                  echo "maturin-develop refuses a symlinked target directory: $maturin_target_root" >&2
                                  exit 2
                                fi
                                if [ -e "$maturin_target_root" ] && [ ! -d "$maturin_target_root" ]; then
                                  echo "maturin-develop target path is not a directory: $maturin_target_root" >&2
                                  exit 2
                                fi
                                if [ ! -e "$maturin_target_root" ]; then
                                  (umask 022; mkdir -- "$maturin_target_root") 2>/dev/null || true
                                fi
                                if [ -L "$maturin_target_root" ] \
                                  || [ ! -d "$maturin_target_root" ] \
                                  || [ "$(readlink -f "$maturin_target_root")" != "$maturin_target_root" ]; then
                                  echo "maturin-develop target directory escapes its worktree: $maturin_target_root" >&2
                                  exit 2
                                fi
                                maturin_lock="$maturin_target_root/.maturin-develop.lock"
                                if [ -L "$maturin_lock" ]; then
                                  echo "maturin-develop lock path must not be a symlink: $maturin_lock" >&2
                                  exit 2
                                fi
                                if [ ! -e "$maturin_lock" ]; then
                                  (set -o noclobber; umask 022; : > "$maturin_lock") 2>/dev/null || true
                                fi
                                if [ -L "$maturin_lock" ] \
                                  || [ ! -f "$maturin_lock" ] \
                                  || [ "$(readlink -f "$maturin_lock")" != "$maturin_lock" ]; then
                                  echo "maturin-develop lock is not a contained regular file: $maturin_lock" >&2
                                  exit 2
                                fi
                                exec 8<>"$maturin_lock"
                                if [ ! "$maturin_lock" -ef /dev/fd/8 ]; then
                                  echo "maturin-develop lock changed while it was opened: $maturin_lock" >&2
                                  exit 2
                                fi
                                echo "waiting for maturin-develop lock: $maturin_lock" >&2
                                "${pkgs.flock}/bin/flock" -x 8
                                if [ -L "$maturin_lock" ] \
                                  || [ ! -f "$maturin_lock" ] \
                                  || [ ! "$maturin_lock" -ef /dev/fd/8 ]; then
                                  echo "maturin-develop lock changed while it was held: $maturin_lock" >&2
                                  exit 2
                                fi
                                echo "acquired maturin-develop lock: $maturin_lock" >&2
                                if [ -L "$maturin_venv" ]; then
                                  reject_maturin_venv "maturin venv path must not be a symlink: $maturin_venv"
                                fi
                                ${setupCallerRustTaskEnvironment}
                                ${desktopEnvironment}
                                if [ -e "$maturin_venv" ] && [ ! -d "$maturin_venv" ]; then
                                  reject_maturin_venv "maturin venv path is not a directory: $maturin_venv"
                                fi
                                canonical_maturin_venv="$repo_root/target/maturin-venv"
                                if [ "$(readlink -m "$maturin_venv")" != "$canonical_maturin_venv" ]; then
                                  reject_maturin_venv "maturin venv resolves outside the worktree target directory: $maturin_venv"
                                fi
                                if [ ! -e "$maturin_venv" ]; then
                                  "${pythonEnv}/bin/python" -I -S -m venv \
                                    --without-pip \
                                    "$maturin_venv"
                                fi
                                if [ "$(readlink -f "$maturin_venv")" != "$canonical_maturin_venv" ]; then
                                  reject_maturin_venv "maturin venv does not resolve to its fixed worktree path: $maturin_venv"
                                fi
                                for maturin_directory in \
                                  "$maturin_venv/bin" \
                                  "$maturin_venv/lib"; do
                                  if [ -L "$maturin_directory" ] || [ ! -d "$maturin_directory" ]; then
                                    reject_maturin_venv "maturin venv contains a redirected or missing directory: $maturin_directory"
                                  fi
                                done
                                if [ -L "$maturin_venv/pyvenv.cfg" ] || [ ! -f "$maturin_venv/pyvenv.cfg" ]; then
                                  reject_maturin_venv "maturin venv has a redirected or missing pyvenv.cfg: $maturin_venv/pyvenv.cfg"
                                fi
                                if [ ! -x "$maturin_python" ]; then
                                  reject_maturin_venv "maturin venv has no executable Python: $maturin_python"
                                fi
                                expected_python="$(readlink -f "${pythonEnv}/bin/python")"
                                actual_python="$(readlink -f "$maturin_python")"
                                if [ "$actual_python" != "$expected_python" ]; then
                                  echo "maturin venv uses an unexpected Python: $actual_python" >&2
                                  echo "expected the flake Python: $expected_python" >&2
                                  reject_maturin_venv "maturin venv interpreter does not match the flake Python"
                                fi
                                flake_python_output="$("${pythonEnv}/bin/python" -I -S -c '
                import os
                import sys

                print(os.path.realpath(sys.base_prefix))
                print(os.path.realpath(sys._base_executable))
                ')"
                                mapfile -t flake_python_probe <<< "$flake_python_output"
                                venv_python_output="$("$maturin_python" -I -S -c '
                import os
                import sys
                import sysconfig

                print(f"{sys.version_info.major}.{sys.version_info.minor}")
                print(os.path.realpath(sys.prefix))
                print(os.path.realpath(sys.base_prefix))
                print(os.path.realpath(sys._base_executable))
                print(os.path.realpath(sys.executable))
                for name in ("purelib", "platlib", "scripts"):
                    path = sysconfig.get_path(name)
                    if path is None:
                        raise RuntimeError(f"missing sysconfig path: {name}")
                    print(path)
                    print(os.path.realpath(path))
                ')"
                                mapfile -t venv_python_probe <<< "$venv_python_output"
                                if [ "''${#flake_python_probe[@]}" -ne 2 ] || [ "''${#venv_python_probe[@]}" -ne 11 ]; then
                                  reject_maturin_venv "maturin venv Python returned an incomplete isolation probe"
                                fi
                                if [ "''${venv_python_probe[0]}" != "3.14" ]; then
                                  reject_maturin_venv "maturin venv must use Python 3.14: $maturin_python"
                                fi
                                if [ "''${venv_python_probe[1]}" != "$canonical_maturin_venv" ]; then
                                  reject_maturin_venv "maturin venv reports an unexpected sys.prefix: ''${venv_python_probe[1]}"
                                fi
                                if [ "''${venv_python_probe[2]}" != "''${flake_python_probe[0]}" ] \
                                  || [ "''${venv_python_probe[3]}" != "''${flake_python_probe[1]}" ] \
                                  || [ "''${venv_python_probe[4]}" != "$expected_python" ]; then
                                  reject_maturin_venv "maturin venv Python base does not match the flake Python"
                                fi
                                for maturin_directory in \
                                  "''${venv_python_probe[5]}" \
                                  "''${venv_python_probe[7]}" \
                                  "''${venv_python_probe[9]}" \
                                  "''${venv_python_probe[5]%/site-packages}" \
                                  "''${venv_python_probe[7]%/site-packages}"; do
                                  if [ -L "$maturin_directory" ] || [ ! -d "$maturin_directory" ]; then
                                    reject_maturin_venv "maturin venv sysconfig contains a redirected or missing directory: $maturin_directory"
                                  fi
                                done
                                for maturin_directory in \
                                  "''${venv_python_probe[6]}" \
                                  "''${venv_python_probe[8]}" \
                                  "''${venv_python_probe[10]}"; do
                                  case "$maturin_directory" in
                                    "$canonical_maturin_venv"/*) ;;
                                    *) reject_maturin_venv "maturin venv sysconfig path escapes the fixed venv: $maturin_directory" ;;
                                  esac
                                done
                                maturin_inventory_phase=before
                                ${validateMaturinVenvInventory}
                                maturin_source="$gate_home/maturin-source"
                                mkdir -p "$maturin_source/python"
                                for maturin_source_file in \
                                  Cargo.toml \
                                  Cargo.lock \
                                  pyproject.toml \
                                  README.md \
                                  LICENSE; do
                                  if [ -L "$repo_root/$maturin_source_file" ] \
                                    || [ ! -f "$repo_root/$maturin_source_file" ]; then
                                    echo "maturin-develop source input must be a regular file: $repo_root/$maturin_source_file" >&2
                                    exit 2
                                  fi
                                  cp -p -- "$repo_root/$maturin_source_file" "$maturin_source/$maturin_source_file"
                                done
                                for maturin_source_directory in \
                                  "$repo_root/rust" \
                                  "$repo_root/python/pokecon"; do
                                  if [ -L "$maturin_source_directory" ] \
                                    || [ ! -d "$maturin_source_directory" ]; then
                                    echo "maturin-develop source input must be a real directory: $maturin_source_directory" >&2
                                    exit 2
                                  fi
                                  if [ -n "$("${pkgs.findutils}/bin/find" "$maturin_source_directory" -type l -print -quit)" ]; then
                                    echo "maturin-develop source input must not contain symlinks: $maturin_source_directory" >&2
                                    exit 2
                                  fi
                                done
                                cp -a -- "$repo_root/rust" "$maturin_source/rust"
                                cp -a -- "$repo_root/python/pokecon" "$maturin_source/python/pokecon"
                                "${pythonEnv}/bin/python" -I -S -c '
                import sys
                from pathlib import Path

                pyproject = Path(sys.argv[1])
                content = pyproject.read_text(encoding="utf-8")
                old = "python-packages = [\"python/pokecon\"]"
                replacement = "python-source = \"python\"\npython-packages = [\"pokecon\"]"
                if content.count(old) != 1 or "python-source" in content:
                    raise SystemExit(
                        "maturin-develop expected exactly one legacy python-packages setting"
                    )
                pyproject.write_text(content.replace(old, replacement), encoding="utf-8")
                ' "$maturin_source/pyproject.toml"
                                export VIRTUAL_ENV="$maturin_venv"
                                export PYO3_PYTHON="$maturin_python"
                                export UV_OFFLINE=1
                                wheel_dir="$gate_home/maturin-wheel"
                                mkdir -p "$wheel_dir"
                                cd "$maturin_source"
                                maturin build \
                                  --locked \
                                  --offline \
                                  --manifest-path rust/pokecon-pybindings/Cargo.toml \
                                  --out "$wheel_dir" \
                                  "''${maturin_arguments[@]}"
                                shopt -s nullglob
                                maturin_wheels=("$wheel_dir"/*.whl)
                                shopt -u nullglob
                                if [ "''${#maturin_wheels[@]}" -ne 1 ]; then
                                  echo "maturin-develop expected exactly one wheel, found ''${#maturin_wheels[@]}" >&2
                                  exit 1
                                fi
                                maturin_wheel="''${maturin_wheels[0]}"
                                ${validateMaturinDevelopWheel}
                                maturin_inventory_phase=before
                                ${validateMaturinVenvInventory}
                                "${pkgs.uv}/bin/uv" --no-config pip install \
                                  --offline \
                                  --no-deps \
                                  --reinstall \
                                  --python "$maturin_python" \
                                  "$maturin_wheel"
                                maturin_inventory_phase=after
                                ${validateMaturinVenvInventory}
                                native_module="$("$maturin_python" -I -c '
                import os
                import pokecon._native as module

                if module.__file__ is None:
                    raise RuntimeError("pokecon._native has no module file")
                print(os.path.realpath(module.__file__))
                ')"
                                case "$native_module" in
                                  "''${venv_python_probe[6]}"/* | "''${venv_python_probe[8]}"/*) ;;
                                  *)
                                    reject_maturin_venv "installed pokecon._native escapes the fixed venv: $native_module"
                                    ;;
                                esac
                                echo "installed and imported pokecon._native from $native_module"
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
                cp -R "${webBunDependencies}/node_modules" web/
                chmod -R u+w web/node_modules
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
                export CFLAGS="-ffile-prefix-map=$workdir=/build/pokecon -ffile-prefix-map=$release_python=/build/python -ffile-prefix-map=$CARGO_TARGET_DIR=/build/target''${CFLAGS:+ $CFLAGS}"
                export CXXFLAGS="-ffile-prefix-map=$workdir=/build/pokecon -ffile-prefix-map=$release_python=/build/python -ffile-prefix-map=$CARGO_TARGET_DIR=/build/target''${CXXFLAGS:+ $CXXFLAGS}"
                export POKECON_RUST_REMAP_SOURCE="$workdir"
                export POKECON_RUST_REMAP_PYTHON="$release_python"
                export POKECON_RUST_REMAP_TARGET="$CARGO_TARGET_DIR"
                export RUSTC_WRAPPER="${reproducibleRustcWrapper}"
                export POKECON_BUILD_UV_PATH="${portableUvBinary}"
                export POKECON_BUILD_UV_VERSION="${portableUvVersion}"
                unset POKECON_BUILD_PYTHON
                cargo build --locked --release --package pokecon-worker --bin pokecon-worker
                cargo build \
                  --locked \
                  --release \
                  --package pokecon \
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
                  --objdump "${pkgs.binutils}/bin/objdump" \
                  --ephemeral-build-root "$gate_home"
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
                cleanup_tauri_build() {
                  gate_status=$?
                  trap - EXIT
                  restore_release_application || true
                  rm -rf -- "$workdir" "$gate_home"
                  exit "$gate_status"
                }
                trap cleanup_tauri_build EXIT
                cp -p "$normalized_application" "$application"
                (
                  cd rust/pokecon
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
                ${setupSourceGateEnvironment}
                export PYTHONPATH="${source}"
                unset LD_LIBRARY_PATH
                python -m scripts.release.package_smoke \
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
                pkgs.docker-client
              ];
              text = ''
                docker_environment=()
                for docker_name in \
                  DOCKER_CERT_PATH \
                  DOCKER_CONFIG \
                  DOCKER_CONTEXT \
                  DOCKER_HOST \
                  DOCKER_TLS_VERIFY; do
                  if [[ -v $docker_name ]]; then
                    docker_environment+=("$docker_name=''${!docker_name}")
                  fi
                done
                ${setupSourceGateEnvironment}
                for docker_assignment in "''${docker_environment[@]}"; do
                  export "''${docker_assignment?}"
                done
                unset docker_assignment docker_environment docker_name
                if [[ -v DOCKER_CONFIG ]]; then
                  if [[ $DOCKER_CONFIG != /* ]]; then
                    echo "DOCKER_CONFIG must be an absolute directory path: $DOCKER_CONFIG" >&2
                    exit 2
                  fi
                  if [[ ! -d $DOCKER_CONFIG ]] || [[ ! -r $DOCKER_CONFIG ]] || [[ ! -x $DOCKER_CONFIG ]]; then
                    echo "DOCKER_CONFIG must be an accessible directory: $DOCKER_CONFIG" >&2
                    exit 2
                  fi
                fi
                if [[ -v DOCKER_CERT_PATH ]]; then
                  if [[ $DOCKER_CERT_PATH != /* ]] || [[ ! -d $DOCKER_CERT_PATH ]] || [[ ! -r $DOCKER_CERT_PATH ]] || [[ ! -x $DOCKER_CERT_PATH ]]; then
                    echo "DOCKER_CERT_PATH must be an accessible absolute directory: $DOCKER_CERT_PATH" >&2
                    exit 2
                  fi
                fi
                export POKECON_DOCKER="${pkgs.docker-client}/bin/docker"
                "${pkgs.bash}/bin/bash" "${source}/scripts/release/debian_install_smoke.sh" "$@"
              '';
            };

            tauri-check = mkTask {
              name = "tauri-check";
              runtimeInputs = rustTaskInputs ++ [ pkgs.cargo-tauri ];
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                cd rust/pokecon
                cargo tauri build --debug --no-bundle --ci -- --locked
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
                ${setupSourceGateEnvironment}
                cd "${source}"
                typos "$@"
              '';
            };

            typos-check = mkTask {
              name = "typos-check";
              runtimeInputs = [ pkgs.typos ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                typos "$@"
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
                ${setupSourceGateEnvironment}
                cd "${source}"
                markdown_files=("$@")
                if [ "''${#markdown_files[@]}" -eq 0 ]; then
                  mapfile -t markdown_files < <(rg --files -g '*.md')
                fi
                bun --bun "${markdownlintCli}" --config .markdownlint.json "''${markdown_files[@]}"
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
                ${setupSourceGateEnvironment}
                cd "${source}"
                markdown_files=("$@")
                if [ "''${#markdown_files[@]}" -eq 0 ]; then
                  mapfile -t markdown_files < <(rg --files -g '*.md')
                fi
                bun --bun "${markdownlintCli}" --config .markdownlint.json "''${markdown_files[@]}"
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
                ${setupSourceGateEnvironment}
                cd "${source}"
                export NODE_PATH="${pkgs.textlint-rule-no-start-duplicated-conjunction}/lib/node_modules"
                text_files=("$@")
                if [ "''${#text_files[@]}" -eq 0 ]; then
                  mapfile -t text_files < <(rg --files -g '*.md' -g '*.txt')
                fi
                bun --bun "${textlintCli}" --config .textlintrc.json "''${text_files[@]}"
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
                ${setupSourceGateEnvironment}
                cd "${source}"
                export NODE_PATH="${pkgs.textlint-rule-no-start-duplicated-conjunction}/lib/node_modules"
                text_files=("$@")
                if [ "''${#text_files[@]}" -eq 0 ]; then
                  mapfile -t text_files < <(rg --files -g '*.md' -g '*.txt')
                fi
                bun --bun "${textlintCli}" --config .textlintrc.json "''${text_files[@]}"
              '';
            };

            web-check = mkTask {
              name = "web-check";
              runtimeInputs = [
                bun
                pythonEnv
              ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                if python -m scripts.quality.source_guard web --require-applicable; then
                  workdir="$gate_home/web"
                  mkdir -p "$workdir"
                  cp -R web/. "$workdir/"
                  chmod -R u+w "$workdir"
                  cp -R "${webBunDependencies}/node_modules" "$workdir/"
                  chmod -R u+w "$workdir/node_modules"
                  cd "$workdir"
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
              runtimeInputs = rustTaskInputs ++ [
                bun
                pkgs.diffutils
              ];
              text = ''
                ${sanitizeGateEnvironment}
                if ! repo_root="$("${pkgs.git}/bin/git" rev-parse --show-toplevel)"; then
                  echo "generate-api-types must be run from a PokeCon worktree" >&2
                  exit 2
                fi
                cd "$repo_root"
                ${setupCallerRustTaskEnvironment}
                ${desktopEnvironment}
                export POKECON_API_NODE_MODULES="${apiBunDependencies}/node_modules"
                scripts/quality/generate-api-types.sh "$@"
              '';
            };

            check = mkTask {
              name = "check";
              runtimeInputs = rustTaskInputs ++ [
                pkgs.basedpyright
                pkgs.actionlint
                bun
                pkgs.check-jsonschema
                pkgs.diffutils
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
                "${cliHelpCheck.program}"
                ${setupWorkdir}
                ${desktopEnvironment}
                export NODE_PATH="${pkgs.textlint-rule-no-start-duplicated-conjunction}/lib/node_modules"
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD/python:$PWD"
                python -m scripts.quality.source_filter
                actionlint .github/workflows/*.yml
                python -m scripts.release.gate
                cargo run --locked --package pokecon --bin generate_contracts --features contract-generator -- --check
                cargo test --locked --package pokecon --test contract_sync
                check-jsonschema --check-metaschema generated/settings.schema.json
                python -m scripts.acceptance.records
                export POKECON_API_NODE_MODULES="${apiBunDependencies}/node_modules"
                scripts/quality/generate-api-types.sh --check
                cp -R "${webBunDependencies}/node_modules" web/
                chmod -R u+w web/node_modules
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
                  entry = "nix run .#workspace-lock-check";
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
                  entry = "${safeFormatter}/bin/pokecon-format --ci";
                  pass_filenames = false;
                };
                typos = {
                  enable = true;
                  entry = "nix run .#typos --";
                  pass_filenames = false;
                };
              };
            };
          };

        };
    };
}
