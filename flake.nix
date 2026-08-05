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
    let
      canonicalFlakeHash = "4e0bd8b4041c7305fab51c464e3793498f28d3337f36a430ad0f49e8ea74fd63";
      canonicalFlakePath = ./flake.nix;
      canonicalFlakeText = builtins.readFile canonicalFlakePath;
      normalizedCanonicalFlakeText =
        builtins.replaceStrings [ canonicalFlakeHash ] [ "<canonical-flake-sha256>" ]
          canonicalFlakeText;
      expectedResolvedInputs = [
        {
          name = "flake-parts";
          value = flake-parts;
          rev = "17c9d6cdfc60c64f4ee8d306f9bc0b4ccb51481e";
          narHash = "sha256-vp6Y/Grm98ESt6ceOkWiHWyZRDV3J1RID4w+6NWK9yA=";
        }
        {
          name = "git-hooks";
          value = git-hooks;
          rev = "43b3c1ab9d40fb1dbb008f451988a91e375825e9";
          narHash = "sha256-ReRHaLgr/uVqdD8afFSn+myXIfpHeOhP0yYe0TJqAA8=";
        }
        {
          name = "linux-release-nixpkgs";
          value = inputs.linux-release-nixpkgs;
          rev = "b134951a4c9f3c995fd7be05f3243f8ecd65d798";
          narHash = "sha256-OnSAY7XDSx7CtDoqNh8jwVwh4xNL/2HaJxGjryLWzX8=";
        }
        {
          name = "nixpkgs";
          value = inputs.nixpkgs;
          rev = "e2587caef70cea85dd97d7daab492899902dbf5d";
          narHash = "sha256-wWFrV5/Qbm+lyt5x20E/bSbfJiGKMo4RCxZV8cl/WZI=";
        }
        {
          name = "rust-overlay";
          value = rust-overlay;
          rev = "471286a5fadc690e2408ad854eb32325f5e74da7";
          narHash = "sha256-ZaS89rj5u6pygXKDRfCzPMGm7isJxLsSeIAR5EaSyu8=";
        }
        {
          name = "systems";
          value = systems;
          rev = "da67096a3b9bf56a91d16901293e51ba5b49a27e";
          narHash = "sha256-Vy1rq5AaRuLzOxct8nz4T6wlgyUR7zLU309k9mBC768=";
        }
        {
          name = "treefmt-nix";
          value = treefmt-nix;
          rev = "df3c0640565d04a0261253cdd89fce78ec50168a";
          narHash = "sha256-47cxbcZODibHv3rELFQ9vZly0vUNkND/atn/U7HLeb0=";
        }
      ];
      resolvedInputsAreCanonical = builtins.all (
        expected:
        expected.value ? rev
        && expected.value.rev == expected.rev
        && expected.value ? narHash
        && expected.value.narHash == expected.narHash
      ) expectedResolvedInputs;
    in
    # Repository follows edges are fixed by the normalized flake hash and the
    # audited flake.lock. Nix only exposes source identity for direct inputs;
    # nested CLI --override-input authority remains outside this app boundary.
    assert
      (builtins.readDir ./.)."flake.nix" == "regular" || builtins.throw "flake.nix is not a regular file";
    assert
      builtins.hashString "sha256" normalizedCanonicalFlakeText == canonicalFlakeHash
      || builtins.throw "flake.nix differs from its normalized canonical hash";
    assert
      resolvedInputsAreCanonical || builtins.throw "resolved direct Nix inputs differ from flake.lock";
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
          linuxReleaseRuntimeLibraries = linuxReleasePkgs.symlinkJoin {
            name = "pokecon-linux-release-runtime-libraries";
            paths = [
              linuxReleasePortaudio
              linuxReleaseCc.cc.lib
              linuxReleasePkgs.zlib
              linuxReleasePkgs.xorg.libxcb
              linuxReleasePkgs.libglvnd
              linuxReleasePkgs.glib.out
              linuxReleasePkgs.xorg.libSM
              linuxReleasePkgs.xorg.libXext
              linuxReleasePkgs.xorg.libXrender
            ];
          };
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
          rustToolchain =
            assert lib.assertMsg (
              (builtins.readDir inputs.self.outPath)."rust-toolchain.toml" == "regular"
              &&
                builtins.hashFile "sha256" (inputs.self.outPath + "/rust-toolchain.toml")
                == "d3ceb1cb2217972a209e49ca1ac585998f21a2dabf96e6ceda03e9442e34ee29"
            ) "Rust toolchain file changed or is not a regular file";
            pkgsWithOverlays.rust-bin.fromRustupToolchainFile (inputs.self.outPath + "/rust-toolchain.toml");
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
          portableUvVersionOutput = "uv 0.11.8 (x86_64-unknown-linux-gnu)";
          pythonPackageBuildUvVersion = "0.11.28";
          pythonPackageBuildUv =
            assert lib.assertMsg (pkgs.uv.version == pythonPackageBuildUvVersion)
              "Nix provides uv ${pkgs.uv.version}; the Python package build requires ${pythonPackageBuildUvVersion}";
            pkgs.uv;
          portableUv =
            if system == "x86_64-linux" then
              pkgs.fetchzip {
                url = "https://github.com/astral-sh/uv/releases/download/${portableUvVersion}/uv-x86_64-unknown-linux-gnu.tar.gz";
                hash = "sha256-LWnCnwmLdeJIV4ytqFqWwwFPlTs+FlODuPJSTgTFngY=";
              }
            else
              pkgs.uv;
          portableUvBinary = if system == "x86_64-linux" then "${portableUv}/uv" else "${pkgs.uv}/bin/uv";
          portableUvFileSha256 = "646adf5cf12ba17d1a41fa77c8dd6496f73651dcfeeed6b5f4ec019b36bc7153";
          portableUvSystemInterpreter = "/lib64/ld-linux-x86-64.so.2";
          portableUvNeededLibraries = [
            "libc.so.6"
            "libdl.so.2"
            "libgcc_s.so.1"
            "libm.so.6"
            "libpthread.so.0"
            "librt.so.1"
          ];
          portableUvNeededInventory = linuxReleasePkgs.writeText "pokecon-portable-uv-needed.txt" ''
            ${lib.concatStringsSep "\n" portableUvNeededLibraries}
          '';
          portableUvExecutionLoader = linuxReleasePkgs.stdenv.cc.bintools.dynamicLinker;
          portableUvExecutionLibraryPath = linuxReleasePkgs.lib.makeLibraryPath [
            linuxReleasePkgs.glibc
            linuxReleaseCc.cc.lib
          ];
          portableUvExecution =
            if system == "x86_64-linux" then
              linuxReleasePkgs.runCommand "pokecon-portable-uv-execution-${portableUvVersion}" { } ''
                set -o errexit -o nounset -o pipefail
                raw_uv="${portableUvBinary}"
                if [ -L "$raw_uv" ] || [ ! -f "$raw_uv" ] || [ ! -x "$raw_uv" ]; then
                  echo "portable uv is not a real executable file: $raw_uv" >&2
                  exit 2
                fi
                raw_uv_sha256="$("${linuxReleasePkgs.coreutils}/bin/sha256sum" -- "$raw_uv")"
                raw_uv_sha256="''${raw_uv_sha256%% *}"
                if [ "$raw_uv_sha256" != "${portableUvFileSha256}" ]; then
                  echo "portable uv executable digest changed: $raw_uv_sha256" >&2
                  exit 2
                fi
                raw_uv_interpreter="$("${linuxReleasePkgs.patchelf}/bin/patchelf" --print-interpreter "$raw_uv")"
                if [ "$raw_uv_interpreter" != "${portableUvSystemInterpreter}" ]; then
                  echo "portable uv interpreter changed: $raw_uv_interpreter" >&2
                  exit 2
                fi
                raw_uv_rpath="$("${linuxReleasePkgs.patchelf}/bin/patchelf" --print-rpath "$raw_uv")"
                if [ -n "$raw_uv_rpath" ]; then
                  echo "portable uv unexpectedly carries an RPATH: $raw_uv_rpath" >&2
                  exit 2
                fi
                actual_needed="$TMPDIR/portable-uv-needed.txt"
                "${linuxReleasePkgs.patchelf}/bin/patchelf" --print-needed "$raw_uv" \
                  | "${linuxReleasePkgs.coreutils}/bin/sort" > "$actual_needed"
                if ! "${linuxReleasePkgs.diffutils}/bin/cmp" -s -- \
                  "${portableUvNeededInventory}" "$actual_needed"; then
                  echo "portable uv DT_NEEDED inventory changed" >&2
                  "${linuxReleasePkgs.diffutils}/bin/diff" -u \
                    "${portableUvNeededInventory}" "$actual_needed" >&2 || true
                  exit 2
                fi
                if [ ! -f "${portableUvExecutionLoader}" ] \
                  || [ ! -x "${portableUvExecutionLoader}" ]; then
                  echo "portable uv execution loader is unavailable" >&2
                  exit 2
                fi
                "${linuxReleasePkgs.coreutils}/bin/mkdir" -p "$out/bin"
                "${linuxReleasePkgs.coreutils}/bin/install" -m 0755 \
                  "$raw_uv" "$out/bin/uv"
                "${linuxReleasePkgs.patchelf}/bin/patchelf" \
                  --set-interpreter "${portableUvExecutionLoader}" \
                  --set-rpath "${portableUvExecutionLibraryPath}" \
                  "$out/bin/uv"
                if [ "$("${linuxReleasePkgs.patchelf}/bin/patchelf" --print-interpreter "$out/bin/uv")" \
                  != "${portableUvExecutionLoader}" ]; then
                  echo "portable uv execution copy has an unexpected interpreter" >&2
                  exit 2
                fi
                if [ "$("${linuxReleasePkgs.patchelf}/bin/patchelf" --print-rpath "$out/bin/uv")" \
                  != "${portableUvExecutionLibraryPath}" ]; then
                  echo "portable uv execution copy has an unexpected RPATH" >&2
                  exit 2
                fi
                patched_needed="$TMPDIR/patched-portable-uv-needed.txt"
                "${linuxReleasePkgs.patchelf}/bin/patchelf" --print-needed "$out/bin/uv" \
                  | "${linuxReleasePkgs.coreutils}/bin/sort" > "$patched_needed"
                if ! "${linuxReleasePkgs.diffutils}/bin/cmp" -s -- \
                  "${portableUvNeededInventory}" "$patched_needed"; then
                  echo "patching the portable uv execution copy changed DT_NEEDED" >&2
                  exit 2
                fi
                resolution="$TMPDIR/portable-uv-resolution.txt"
                "${portableUvExecutionLoader}" \
                  --inhibit-cache \
                  --library-path "${portableUvExecutionLibraryPath}" \
                  --list "$out/bin/uv" > "$resolution"
                while IFS= read -r needed_library; do
                  if [ "$needed_library" = "libgcc_s.so.1" ]; then
                    expected_library="${linuxReleaseCc.cc.lib}/lib/$needed_library"
                  else
                    expected_library="${linuxReleasePkgs.glibc}/lib/$needed_library"
                  fi
                  if [ "$("${linuxReleasePkgs.gnugrep}/bin/grep" -F -c -- "$needed_library => " "$resolution")" -ne 1 ]; then
                    echo "portable uv loader did not resolve exactly one $needed_library" >&2
                    exit 2
                  fi
                  resolved_library="$(
                    "${linuxReleasePkgs.gnugrep}/bin/grep" -F -- "$needed_library => " "$resolution" \
                      | "${linuxReleasePkgs.gawk}/bin/awk" '{ print $3 }'
                  )"
                  if [ "$("${linuxReleasePkgs.coreutils}/bin/readlink" -f -- "$resolved_library")" \
                    != "$("${linuxReleasePkgs.coreutils}/bin/readlink" -f -- "$expected_library")" ]; then
                    echo "portable uv resolved $needed_library outside the pinned release closure: $resolved_library" >&2
                    exit 2
                  fi
                done < "${portableUvNeededInventory}"
                actual_portable_uv_version_output=
                if ! actual_portable_uv_version_output="$("$out/bin/uv" --version)"; then
                  echo "portable uv execution copy version probe failed; actual output: $actual_portable_uv_version_output" >&2
                  exit 2
                fi
                if [ "$actual_portable_uv_version_output" != "${portableUvVersionOutput}" ]; then
                  echo "portable uv execution copy reports an unexpected version; expected: ${portableUvVersionOutput}; actual: $actual_portable_uv_version_output" >&2
                  exit 2
                fi
                if [ "$("${linuxReleasePkgs.coreutils}/bin/sha256sum" -- "$raw_uv")" \
                  != "${portableUvFileSha256}  $raw_uv" ]; then
                  echo "portable uv raw artifact changed while preparing its execution copy" >&2
                  exit 2
                fi
                "${linuxReleasePkgs.coreutils}/bin/chmod" 0555 "$out/bin/uv"
              ''
            else
              portableUv;
          portableUvExecutionBinary =
            if system == "x86_64-linux" then "${portableUvExecution}/bin/uv" else portableUvBinary;
          reproducibleRustcWrapper = pkgs.writeShellScript "pokecon-reproducible-rustc-wrapper" ''
            set -o errexit -o nounset -o pipefail
            if [ "$#" -lt 1 ]; then
              echo "reproducible rustc wrapper received no compiler" >&2
              exit 2
            fi
            rustc="$1"
            shift
            if [ "$rustc" != "${rustToolchain}/bin/rustc" ]; then
              echo "reproducible rustc wrapper rejected a nested or redirected compiler: $rustc" >&2
              exit 2
            fi
            : "''${POKECON_RUST_REMAP_SOURCE:?POKECON_RUST_REMAP_SOURCE is required}"
            : "''${POKECON_RUST_REMAP_PYTHON:?POKECON_RUST_REMAP_PYTHON is required}"
            : "''${POKECON_RUST_REMAP_TARGET:?POKECON_RUST_REMAP_TARGET is required}"
            exec "$rustc" \
              "--remap-path-prefix=${source}=/build/pokecon" \
              "--remap-path-prefix=${controlledCargoSource}=/build/pokecon" \
              "--remap-path-prefix=$POKECON_RUST_REMAP_SOURCE=/build/pokecon" \
              "--remap-path-prefix=$POKECON_RUST_REMAP_PYTHON=/build/python" \
              "--remap-path-prefix=$POKECON_RUST_REMAP_TARGET=/build/target" \
              "-Lnative=$POKECON_RUST_REMAP_PYTHON/lib" \
              "$@"
          '';
          pinnedRustcWrapper = pkgs.writeShellScript "pokecon-pinned-rustc-wrapper" ''
            set -o errexit -o nounset -o pipefail
            if [ "$#" -lt 1 ]; then
              echo "pinned rustc wrapper received no compiler" >&2
              exit 2
            fi
            compiler="$1"
            shift
            case "$compiler" in
              "${rustToolchain}/bin/rustc")
                exec "$compiler" "$@"
                ;;
              "${pkgs.cargo-auditable}/bin/cargo-auditable")
                if [ "$#" -lt 1 ] || [ "$1" != "${rustToolchain}/bin/rustc" ]; then
                  echo "pinned rustc wrapper rejected cargo-auditable with a redirected compiler: ''${1:-<unset>}" >&2
                  exit 2
                fi
                exec "$compiler" "$@"
                ;;
              *)
                echo "pinned rustc wrapper rejected a nested or redirected compiler: $compiler" >&2
                exit 2
                ;;
            esac
          '';
          cargoInvocationRoot = pkgs.runCommand "pokecon-cargo-invocation-root" { } ''
            mkdir -p "$out"
          '';

          source =
            assert workspaceCargoInputsAreCanonical;
            builtins.path {
              path = inputs.self.outPath;
              name = "pokecon-source";
              filter =
                path: type:
                let
                  sourcePath = toString path;
                in
                type == "directory"
                || (
                  type == "regular"
                  && (
                    lib.hasSuffix "/.gitignore" sourcePath
                    || sourcePath == "${inputs.self.outPath}/LICENSE"
                    || lib.hasSuffix ".rs" sourcePath
                    || lib.hasSuffix ".toml" sourcePath
                    || lib.hasSuffix ".lock" sourcePath
                    || lib.hasSuffix ".json" sourcePath
                    || lib.hasSuffix ".json5" sourcePath
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
                    || lib.hasSuffix ".eot" sourcePath
                  )
                );
            };
          productionRoutingAuditTest =
            let
              relativeAuditTest = "/tests/quality/test_ui_package_check.py";
              expectedAuditTestHash = "12b400accc76e65f8c3fdc042f8522c044c7ab9aaa2fdc193fb45d8a9803f4e5";
              inputAuditTest = inputs.self.outPath + relativeAuditTest;
              filteredAuditTest = source + relativeAuditTest;
            in
            assert
              (builtins.readDir (inputs.self.outPath + "/tests/quality"))."test_ui_package_check.py" == "regular"
              || builtins.throw "production routing audit test is not a regular input file";
            assert
              builtins.hashFile "sha256" inputAuditTest == expectedAuditTestHash
              || builtins.throw "production routing audit test input changed";
            assert
              builtins.hashFile "sha256" filteredAuditTest == expectedAuditTestHash
              || builtins.throw "filtered production routing audit test changed";
            filteredAuditTest;

          workspaceMemberPaths = [
            "rust/pokecon"
            "rust/pokecon-contracts"
            "rust/pokecon-core"
            "rust/pokecon-camera"
            "rust/pokecon-device"
            "rust/pokecon-dynamic"
            "rust/pokecon-desktop"
            "rust/pokecon-server"
            "rust/pokecon-settings"
            "rust/pokecon-worker"
          ];
          workspaceDefaultMemberPaths = [
            "rust/pokecon"
            "rust/pokecon-contracts"
            "rust/pokecon-core"
            "rust/pokecon-camera"
            "rust/pokecon-device"
            "rust/pokecon-desktop"
            "rust/pokecon-server"
            "rust/pokecon-settings"
            "rust/pokecon-worker"
          ];
          workspaceMemberManifests = builtins.listToAttrs (
            map (memberPath: {
              name = memberPath;
              value = builtins.fromTOML (builtins.readFile (inputs.self.outPath + "/${memberPath}/Cargo.toml"));
            }) workspaceMemberPaths
          );
          expectedWorkspacePackageNames = {
            "rust/pokecon" = "pokecon";
            "rust/pokecon-camera" = "pokecon-camera";
            "rust/pokecon-contracts" = "pokecon-contracts";
            "rust/pokecon-core" = "pokecon-core";
            "rust/pokecon-desktop" = "pokecon-desktop";
            "rust/pokecon-device" = "pokecon-device";
            "rust/pokecon-dynamic" = "pokecon-dynamic";
            "rust/pokecon-server" = "pokecon-server";
            "rust/pokecon-settings" = "pokecon-settings";
            "rust/pokecon-worker" = "pokecon-worker";
          };
          expectedWorkspacePackageBuild = {
            "rust/pokecon" = "build.rs";
            "rust/pokecon-camera" = false;
            "rust/pokecon-contracts" = false;
            "rust/pokecon-core" = false;
            "rust/pokecon-desktop" = false;
            "rust/pokecon-device" = false;
            "rust/pokecon-dynamic" = false;
            "rust/pokecon-server" = false;
            "rust/pokecon-settings" = "build.rs";
            "rust/pokecon-worker" = false;
          };
          expectedWorkspaceManifestHashes = {
            "rust/pokecon" = "9bcb9da3d19ecc6ecd99b04a2d0a698071dbcdf6d3f3eec606a527700dcc5084";
            "rust/pokecon-camera" = "e14e0b007e994bdb7f74df1aaf6765ce57c9269fc78612c290e3e35fbb5037fd";
            "rust/pokecon-contracts" = "7e1dac2f761acaf07f144ae0a59d464f725a71c262367efd20903920d6a9db98";
            "rust/pokecon-core" = "7102df1a8877cc2ed5c2033e1cb256067181af13867bf2c20d78f841bb894cd9";
            "rust/pokecon-desktop" = "36b5e58e6b7a6ad12993c7287021a5fb6e688ca2fedefd225e1707281e5238aa";
            "rust/pokecon-device" = "9bf1252084706e64f67cf1471fbd8f659e2a71b8773e5c28354c18cb6adf887b";
            "rust/pokecon-dynamic" = "04d64485584691365a1de0bc1165734fe8426458511a80e2cc13df2079816834";
            "rust/pokecon-server" = "8e8ffb87eac705f21b84254b44fd08a86191691e3e9aec080ae86b8a54e763b5";
            "rust/pokecon-settings" = "31a6cedff2ac68e2c712e2cb624bc29ad0950904413ae26107890d1c4955a972";
            "rust/pokecon-worker" = "268c724c9070a28684ca669e889662644ff5044e37bf2f6876afd3b346b8fbb2";
          };
          expectedWorkspaceBuildDependencies = {
            "rust/pokecon" = {
              dunce = "1.0.5";
              tauri-build = {
                features = [ ];
                version = "2.5.4";
              };
            };
            "rust/pokecon-camera" = { };
            "rust/pokecon-contracts" = { };
            "rust/pokecon-core" = { };
            "rust/pokecon-desktop" = { };
            "rust/pokecon-device" = { };
            "rust/pokecon-dynamic" = { };
            "rust/pokecon-server" = { };
            "rust/pokecon-settings" = {
              hex.workspace = true;
              serde.workspace = true;
              serde_json.workspace = true;
              sha2.workspace = true;
              toml.workspace = true;
            };
            "rust/pokecon-worker" = { };
          };
          actualWorkspacePackageNames = lib.mapAttrs (
            _memberPath: manifest: manifest.package.name or null
          ) workspaceMemberManifests;
          actualWorkspacePackageBuild = lib.mapAttrs (
            _memberPath: manifest: manifest.package.build or null
          ) workspaceMemberManifests;
          actualWorkspaceBuildDependencies = lib.mapAttrs (
            _memberPath: manifest: manifest."build-dependencies" or { }
          ) workspaceMemberManifests;
          workspaceTargetBuildDependenciesAreEmpty = lib.all (
            manifest:
            lib.all (targetManifest: (targetManifest."build-dependencies" or { }) == { }) (
              builtins.attrValues (manifest.target or { })
            )
          ) (builtins.attrValues workspaceMemberManifests);
          dependencyTableNames = [
            "dependencies"
            "dev-dependencies"
            "build-dependencies"
          ];
          dependencyTablesForManifest =
            manifest:
            map (dependencyTableName: manifest.${dependencyTableName} or { }) dependencyTableNames
            ++ lib.concatMap (
              targetManifest:
              map (dependencyTableName: targetManifest.${dependencyTableName} or { }) dependencyTableNames
            ) (builtins.attrValues (manifest.target or { }));
          pathDependenciesForOwner =
            ownerPath: manifest:
            lib.concatMap (
              dependencyTable:
              lib.filter (dependency: dependency != null) (
                lib.mapAttrsToList (
                  dependencyName: dependencySpec:
                  if builtins.isAttrs dependencySpec && dependencySpec ? path then
                    {
                      inherit dependencyName ownerPath;
                      packageName = dependencySpec.package or dependencyName;
                      path = dependencySpec.path;
                    }
                  else
                    null
                ) dependencyTable
              )
            ) (dependencyTablesForManifest manifest);
          workspacePathDependencies =
            pathDependenciesForOwner "" {
              dependencies = workspaceManifest.workspace.dependencies or { };
            }
            ++ lib.concatMap (
              memberPath: pathDependenciesForOwner memberPath workspaceMemberManifests.${memberPath}
            ) workspaceMemberPaths;
          normalizeWorkspacePath =
            ownerPath: dependencyPath:
            let
              initialParts = lib.filter (part: part != "") (lib.splitString "/" ownerPath);
              normalized =
                lib.foldl'
                  (
                    state: part:
                    if part == "" || part == "." then
                      state
                    else if part == ".." then
                      if state.parts == [ ] then
                        state // { escaped = true; }
                      else
                        state // { parts = lib.init state.parts; }
                    else
                      state // { parts = state.parts ++ [ part ]; }
                  )
                  {
                    escaped = false;
                    parts = initialParts;
                  }
                  (lib.splitString "/" dependencyPath);
            in
            if normalized.escaped then null else lib.concatStringsSep "/" normalized.parts;
          workspacePathDependenciesAreClosed = lib.all (
            dependency:
            builtins.isString dependency.path
            && !(lib.hasPrefix "/" dependency.path)
            && !(lib.hasInfix "\\" dependency.path)
            && (
              let
                resolvedPath = normalizeWorkspacePath dependency.ownerPath dependency.path;
              in
              resolvedPath != null
              && lib.elem resolvedPath workspaceMemberPaths
              && dependency.dependencyName == expectedWorkspacePackageNames.${resolvedPath}
              && dependency.packageName == expectedWorkspacePackageNames.${resolvedPath}
            )
          ) workspacePathDependencies;
          expectedWorkspaceBuildScripts = {
            "rust/pokecon/build.rs" = "4be14ee06480c69114877e23c07e18ff7ead774f68e4ded3e7b62fe549b86d9a";
            "rust/pokecon-settings/build.rs" =
              "961b332422780c5fa343893522af831ed23828e707e5934f7fdf91caac0138b0";
          };
          actualWorkspaceBuildScriptPaths = lib.sort builtins.lessThan (
            lib.concatMap (
              memberPath:
              let
                memberEntries = builtins.readDir (inputs.self.outPath + "/${memberPath}");
              in
              lib.optional (memberEntries ? "build.rs") "${memberPath}/build.rs"
            ) workspaceMemberPaths
          );
          workspaceBuildScriptsAreCanonical = lib.all (
            buildScriptPath:
            let
              memberPath = lib.removeSuffix "/build.rs" buildScriptPath;
              memberEntries = builtins.readDir (inputs.self.outPath + "/${memberPath}");
            in
            memberEntries."build.rs" == "regular"
            &&
              builtins.hashFile "sha256" (inputs.self.outPath + "/${buildScriptPath}")
              == expectedWorkspaceBuildScripts.${buildScriptPath}
          ) (builtins.attrNames expectedWorkspaceBuildScripts);
          workspaceMemberManifestsAreCanonical = lib.all (
            memberPath:
            let
              memberEntries = builtins.readDir (inputs.self.outPath + "/${memberPath}");
            in
            memberEntries."Cargo.toml" == "regular"
            &&
              builtins.hashFile "sha256" (inputs.self.outPath + "/${memberPath}/Cargo.toml")
              == expectedWorkspaceManifestHashes.${memberPath}
          ) workspaceMemberPaths;
          repositoryCargoConfigInventory =
            let
              scanDirectory =
                relativeDirectory:
                let
                  absoluteDirectory =
                    inputs.self.outPath + lib.optionalString (relativeDirectory != "") "/${relativeDirectory}";
                  entries = builtins.readDir absoluteDirectory;
                in
                lib.concatMap (
                  entryName:
                  let
                    entryType = entries.${entryName};
                    relativeEntry = if relativeDirectory == "" then entryName else "${relativeDirectory}/${entryName}";
                  in
                  if entryName == ".cargo" then
                    if entryType != "directory" then
                      [ "${relativeEntry}/<redirected>" ]
                    else
                      let
                        cargoEntries = builtins.readDir (inputs.self.outPath + "/${relativeEntry}");
                      in
                      lib.concatMap
                        (
                          configName: lib.optional (builtins.hasAttr configName cargoEntries) "${relativeEntry}/${configName}"
                        )
                        [
                          "config"
                          "config.toml"
                        ]
                  else if entryType == "directory" then
                    scanDirectory relativeEntry
                  else
                    [ ]
                ) (builtins.attrNames entries);
            in
            lib.sort builtins.lessThan (scanDirectory "");
          workspaceCargoInputsAreCanonical =
            assert lib.assertMsg (
              workspaceManifest.workspace.resolver == "2"
            ) "Cargo workspace resolver changed";
            assert lib.assertMsg (
              !(workspaceManifest ? patch) && !(workspaceManifest ? replace)
            ) "Cargo workspace patch/replace tables are forbidden";
            assert lib.assertMsg (
              workspaceManifest.workspace.members == workspaceMemberPaths
            ) "Cargo workspace member inventory changed";
            assert lib.assertMsg (
              workspaceManifest.workspace.default-members == workspaceDefaultMemberPaths
            ) "Cargo workspace default-member inventory changed";
            assert lib.assertMsg (
              actualWorkspacePackageNames == expectedWorkspacePackageNames
            ) "Cargo workspace package names changed";
            assert lib.assertMsg (
              actualWorkspacePackageBuild == expectedWorkspacePackageBuild
            ) "Cargo workspace package.build map changed";
            assert lib.assertMsg (
              actualWorkspaceBuildDependencies == expectedWorkspaceBuildDependencies
            ) "Cargo workspace build-dependencies changed";
            assert lib.assertMsg workspaceTargetBuildDependenciesAreEmpty
              "Cargo target-specific build-dependencies are forbidden";
            assert lib.assertMsg workspacePathDependenciesAreClosed
              "Cargo path dependency escaped or renamed a canonical workspace member";
            assert lib.assertMsg (
              (builtins.readDir inputs.self.outPath)."Cargo.toml" == "regular"
              &&
                builtins.hashFile "sha256" (inputs.self.outPath + "/Cargo.toml")
                == "1bd093a87503d36a0cedab4bf94c057f04c25cb7ad7719dab005855857e19ca2"
            ) "Cargo workspace manifest content changed";
            assert lib.assertMsg workspaceMemberManifestsAreCanonical
              "Cargo workspace member manifest content changed";
            assert lib.assertMsg (
              (builtins.readDir inputs.self.outPath)."Cargo.lock" == "regular"
              &&
                builtins.hashFile "sha256" (inputs.self.outPath + "/Cargo.lock")
                == "ee283fec3ee44f21e28165eaab164ca77b70826e28ef080fca8cd15359d38ead"
            ) "Cargo lockfile content changed";
            assert lib.assertMsg (
              actualWorkspaceBuildScriptPaths == builtins.attrNames expectedWorkspaceBuildScripts
            ) "Cargo workspace build.rs inventory changed";
            assert lib.assertMsg workspaceBuildScriptsAreCanonical "Cargo workspace build.rs content changed";
            assert lib.assertMsg (
              repositoryCargoConfigInventory == [ ]
            ) "repository Cargo config inventory is forbidden";
            true;
          countStringOccurrences = needle: haystack: builtins.length (lib.splitString needle haystack) - 1;
          replaceManifestString =
            label: needle: replacement: manifestText:
            assert lib.assertMsg (
              countStringOccurrences needle manifestText == 1
            ) "${label} manifest anchor is not unique";
            lib.replaceStrings [ needle ] [ replacement ] manifestText;
          canonicalPokeconManifestText = builtins.readFile (inputs.self.outPath + "/rust/pokecon/Cargo.toml");
          canonicalWorkspaceManifestText = builtins.readFile (inputs.self.outPath + "/Cargo.toml");
          canonicalCargoLockText = builtins.readFile (inputs.self.outPath + "/Cargo.lock");
          canonicalServerManifestText = builtins.readFile (
            inputs.self.outPath + "/rust/pokecon-server/Cargo.toml"
          );
          canonicalSettingsManifestText = builtins.readFile (
            inputs.self.outPath + "/rust/pokecon-settings/Cargo.toml"
          );
          canonicalPokeconManifest = workspaceMemberManifests."rust/pokecon";
          canonicalServerManifest = workspaceMemberManifests."rust/pokecon-server";
          canonicalSettingsManifest = workspaceMemberManifests."rust/pokecon-settings";
          controlledPokeconManifestText =
            replaceManifestString "pokecon implicit library target" "\n[features]\n"
              ''

                [lib]
                path = "${source}/rust/pokecon/src/lib.rs"

                [features]
              ''
              (
                replaceManifestString "pokecon primary binary target"
                  "[[bin]]\nname = \"pokecon\"\npath = \"src/main.rs\"\n"
                  "[[bin]]\nname = \"pokecon\"\npath = \"${source}/rust/pokecon/src/main.rs\"\n"
                  (
                    replaceManifestString "pokecon build script" ''build = "build.rs"''
                      ''build = "${source}/rust/pokecon/build.rs"''
                      canonicalPokeconManifestText
                  )
              );
          expectedControlledPokeconManifest = canonicalPokeconManifest // {
            package = canonicalPokeconManifest.package // {
              build = "${source}/rust/pokecon/build.rs";
            };
            lib = {
              path = "${source}/rust/pokecon/src/lib.rs";
            };
            bin = [
              (
                (builtins.head canonicalPokeconManifest.bin)
                // {
                  path = "${source}/rust/pokecon/src/main.rs";
                }
              )
            ]
            ++ builtins.tail canonicalPokeconManifest.bin;
          };
          controlledPokeconManifest =
            assert lib.assertMsg (
              builtins.fromTOML (builtins.unsafeDiscardStringContext controlledPokeconManifestText)
              == expectedControlledPokeconManifest
            ) "controlled pokecon Cargo manifest changed outside its audited target paths";
            pkgs.writeText "pokecon-controlled-Cargo.toml" controlledPokeconManifestText;
          controlledServerManifestText =
            replaceManifestString "pokecon-server library target" "\n[dependencies]\n"
              ''

                [lib]
                path = "${source}/rust/pokecon-server/src/lib.rs"

                [dependencies]
              ''
              canonicalServerManifestText;
          expectedControlledServerManifest = canonicalServerManifest // {
            lib = {
              path = "${source}/rust/pokecon-server/src/lib.rs";
            };
          };
          controlledServerManifest =
            assert lib.assertMsg (
              builtins.fromTOML (builtins.unsafeDiscardStringContext controlledServerManifestText)
              == expectedControlledServerManifest
            ) "controlled pokecon-server Cargo manifest changed outside its audited library path";
            pkgs.writeText "pokecon-server-controlled-Cargo.toml" controlledServerManifestText;
          controlledSettingsManifestText =
            replaceManifestString "pokecon-settings build script" ''build = "build.rs"''
              ''build = "${source}/rust/pokecon-settings/build.rs"''
              canonicalSettingsManifestText;
          expectedControlledSettingsManifest = canonicalSettingsManifest // {
            package = canonicalSettingsManifest.package // {
              build = "${source}/rust/pokecon-settings/build.rs";
            };
          };
          controlledSettingsManifest =
            assert lib.assertMsg (
              builtins.fromTOML (builtins.unsafeDiscardStringContext controlledSettingsManifestText)
              == expectedControlledSettingsManifest
            ) "controlled pokecon-settings Cargo manifest changed outside its audited build path";
            pkgs.writeText "pokecon-settings-controlled-Cargo.toml" controlledSettingsManifestText;
          controlledWorkspaceManifest =
            assert lib.assertMsg (
              builtins.fromTOML canonicalWorkspaceManifestText == workspaceManifest
            ) "controlled workspace Cargo manifest differs from the audited workspace manifest";
            pkgs.writeText "pokecon-workspace-controlled-Cargo.toml" canonicalWorkspaceManifestText;
          controlledCargoLock = pkgs.writeText "pokecon-controlled-Cargo.lock" canonicalCargoLockText;
          controlledWorkspaceMemberManifests = lib.mapAttrs (
            memberPath: canonicalManifest:
            if memberPath == "rust/pokecon" then
              controlledPokeconManifest
            else if memberPath == "rust/pokecon-server" then
              controlledServerManifest
            else if memberPath == "rust/pokecon-settings" then
              controlledSettingsManifest
            else
              let
                canonicalManifestText = builtins.readFile (inputs.self.outPath + "/${memberPath}/Cargo.toml");
              in
              assert lib.assertMsg (
                builtins.fromTOML canonicalManifestText == canonicalManifest
              ) "controlled ${memberPath} Cargo manifest differs from its audited manifest";
              pkgs.writeText "${canonicalManifest.package.name}-controlled-Cargo.toml" canonicalManifestText
          ) workspaceMemberManifests;
          validateControlledCargoWorkspace = ''
            controlled_workspace_root="$(pwd -P)"
            if [ -L "$PWD" ] || [ "$PWD" != "$controlled_workspace_root" ] \
              || [ ! -d "$controlled_workspace_root" ]; then
              echo "controlled Cargo workspace root is redirected or missing: $PWD" >&2
              exit 2
            fi
            ${lib.concatMapStringsSep "\n" (workspaceDirectory: ''
              controlled_workspace_directory="$controlled_workspace_root/${workspaceDirectory}"
              if [ -L "$controlled_workspace_directory" ] \
                || [ ! -d "$controlled_workspace_directory" ] \
                || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$controlled_workspace_directory")" != "$controlled_workspace_directory" ]; then
                echo "controlled Cargo workspace directory is redirected or missing: $controlled_workspace_directory" >&2
                exit 2
              fi
            '') ([ "rust" ] ++ workspaceMemberPaths)}
          '';
          verifyControlledCargoManifests = ''
            ${validateControlledCargoWorkspace}
            verify_controlled_cargo_manifest() {
              controlled_manifest_source="$1"
              controlled_manifest_destination="$controlled_workspace_root/$2"
              if [ -L "$controlled_manifest_destination" ] \
                || [ ! -f "$controlled_manifest_destination" ] \
                || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$controlled_manifest_destination")" != "$controlled_manifest_destination" ] \
                || ! "${pkgs.diffutils}/bin/cmp" -s -- "$controlled_manifest_source" "$controlled_manifest_destination"; then
                echo "controlled Cargo manifest differs from its immutable source: $controlled_manifest_destination" >&2
                exit 2
              fi
            }
            verify_controlled_cargo_manifest "${controlledWorkspaceManifest}" Cargo.toml
            verify_controlled_cargo_manifest "${controlledCargoLock}" Cargo.lock
            ${lib.concatMapStringsSep "\n" (
              memberPath:
              ''verify_controlled_cargo_manifest "${
                controlledWorkspaceMemberManifests.${memberPath}
              }" "${memberPath}/Cargo.toml"''
            ) workspaceMemberPaths}
            unset -f verify_controlled_cargo_manifest
            unset \
              controlled_manifest_source \
              controlled_manifest_destination \
              controlled_workspace_directory \
              controlled_workspace_root
          '';
          installControlledCargoManifests = ''
            ${validateControlledCargoWorkspace}
            install_controlled_cargo_manifest() {
              controlled_manifest_source="$1"
              controlled_manifest_destination="$controlled_workspace_root/$2"
              "${pkgs.coreutils}/bin/rm" -rf -- "$controlled_manifest_destination"
              "${pkgs.coreutils}/bin/cp" -- "$controlled_manifest_source" "$controlled_manifest_destination"
            }
            install_controlled_cargo_manifest "${controlledWorkspaceManifest}" Cargo.toml
            install_controlled_cargo_manifest "${controlledCargoLock}" Cargo.lock
            ${lib.concatMapStringsSep "\n" (
              memberPath:
              ''install_controlled_cargo_manifest "${
                controlledWorkspaceMemberManifests.${memberPath}
              }" "${memberPath}/Cargo.toml"''
            ) workspaceMemberPaths}
            unset -f install_controlled_cargo_manifest
            unset \
              controlled_manifest_source \
              controlled_manifest_destination \
              controlled_workspace_directory \
              controlled_workspace_root
            ${verifyControlledCargoManifests}
          '';
          controlledCargoSource = pkgs.runCommand "pokecon-controlled-cargo-source" { } ''
            "${pkgs.coreutils}/bin/mkdir" -p "$out"
            "${pkgs.coreutils}/bin/cp" -a -- "${source}/." "$out/"
            if ! "${pkgs.diffutils}/bin/diff" \
              --brief \
              --recursive \
              --no-dereference \
              -- "${source}" "$out"; then
              echo "controlled Cargo source copy differs before manifest overlay" >&2
              exit 2
            fi
            "${pkgs.coreutils}/bin/chmod" -R u+w -- "$out"
            cd "$out"
            ${installControlledCargoManifests}
            "${pkgs.coreutils}/bin/chmod" -R a-w -- "$out"
            if ! controlled_source_violation="$(
              "${pkgs.findutils}/bin/find" -P "$out" \
                \( -type l -o -perm /0222 \) -print -quit
            )"; then
              echo "controlled Cargo source inventory failed" >&2
              exit 2
            fi
            if [ -n "$controlled_source_violation" ]; then
              echo "controlled Cargo source contains a symlink or writable entry: $controlled_source_violation" >&2
              exit 2
            fi
            ${verifyControlledCargoManifests}
            unset controlled_source_violation
          '';
          auditPytestConfig = pkgs.writeText "pokecon-audit-pytest.ini" ''
            [pytest]
            addopts =
            markers =
                production_routing_mutation: exhaustive fail-closed mutation audit run by its dedicated Nix gate
          '';
          productionRoutingAudit =
            pkgs.runCommand "pokecon-production-routing-audit"
              {
                nativeBuildInputs = [
                  pythonEnv
                  pkgs.util-linux
                ];
              }
              ''
                export CI=1
                export LANG=C
                export LC_ALL=C
                export PYTHONDONTWRITEBYTECODE=1
                export PYTEST_DISABLE_PLUGIN_AUTOLOAD=1
                unset PYTEST_ADDOPTS PYTEST_PLUGINS PYTHONPATH
                artifact_directory_lock_probe="$TMPDIR/pokecon-artifact-directory-lock-probe"
                mkdir -- "$artifact_directory_lock_probe"
                exec {artifact_directory_lock_probe_fd}< "$artifact_directory_lock_probe"
                if [ "$(stat -Lc '%d:%i' -- "$artifact_directory_lock_probe")" \
                  != "$(stat -Lc '%d:%i' -- "/proc/self/fd/$artifact_directory_lock_probe_fd")" ]; then
                  echo "directory FD lock probe path and descriptor differ" >&2
                  exit 2
                fi
                flock -x "$artifact_directory_lock_probe_fd"
                exec {artifact_directory_lock_contender_fd}< "$artifact_directory_lock_probe"
                if flock -n -x "$artifact_directory_lock_contender_fd"; then
                  echo "directory FD lock probe admitted a concurrent contender" >&2
                  exit 2
                fi
                exec {artifact_directory_lock_contender_fd}<&-
                exec {artifact_directory_lock_probe_fd}<&-
                rmdir -- "$artifact_directory_lock_probe"
                unset \
                  artifact_directory_lock_contender_fd \
                  artifact_directory_lock_probe \
                  artifact_directory_lock_probe_fd
                cd "${source}"
                "${pythonEnv}/bin/python" -I -m pytest \
                  -c "${auditPytestConfig}" \
                  --noconftest \
                  --import-mode=importlib \
                  -p no:cacheprovider \
                  "${productionRoutingAuditTest}::test_rust_routes_override_implicit_head_and_websocket_any"
                mkdir -p "$out"
                touch "$out/passed"
              '';
          productionRoutingMutationAuditRunner = pkgs.writeShellApplication {
            name = "pokecon-production-routing-mutation-audit";
            excludeShellChecks = [ "SC2329" ];
            runtimeInputs = [
              pkgs.coreutils
              pythonEnv
            ];
            text = ''
              mutation_worker_limit=8
              mutation_worker_default_limit=4
              if [ "$#" -eq 0 ]; then
                mutation_worker_count="$(nproc)"
                if [ "$mutation_worker_count" -gt "$mutation_worker_default_limit" ]; then
                  mutation_worker_count="$mutation_worker_default_limit"
                fi
              elif [ "$#" -eq 1 ] && [ "$1" = "--help" ]; then
                echo "usage: nix run .#test-production-routing-mutations [-- --workers COUNT]"
                echo "Run the exhaustive production-routing mutation audit in parallel."
                exit 0
              elif [ "$#" -eq 2 ] && [ "$1" = "--workers" ]; then
                mutation_worker_count="$2"
              else
                echo "usage: nix run .#test-production-routing-mutations [-- --workers COUNT]" >&2
                exit 2
              fi
              if [[ ! $mutation_worker_count =~ ^[1-9][0-9]*$ ]] \
                || [ "$mutation_worker_count" -gt "$mutation_worker_limit" ]; then
                echo "mutation worker count must be an integer from 1 through $mutation_worker_limit" >&2
                exit 2
              fi

              export CI=1
              export LANG=C
              export LC_ALL=C
              export PYTHONDONTWRITEBYTECODE=1
              export PYTEST_DISABLE_PLUGIN_AUTOLOAD=1
              unset \
                POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_COUNT \
                POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_INDEX \
                PYTEST_ADDOPTS \
                PYTEST_PLUGINS \
                PYTHONPATH
              cd "${source}"

              mutation_log_directory=
              mutation_worker_pids=()
              cleanup_mutation_audit() {
                mutation_cleanup_status=$?
                trap - EXIT
                if [ -n "$mutation_log_directory" ]; then
                  rm -rf -- "$mutation_log_directory"
                fi
                exit "$mutation_cleanup_status"
              }
              terminate_mutation_workers() {
                trap - INT TERM
                mutation_active_worker_pids=()
                mapfile -t mutation_active_worker_pids < <(jobs -p)
                for mutation_worker_pid in "''${mutation_active_worker_pids[@]}"; do
                  kill "$mutation_worker_pid" 2>/dev/null || true
                done
                for mutation_worker_pid in "''${mutation_active_worker_pids[@]}"; do
                  wait "$mutation_worker_pid" 2>/dev/null || true
                done
                exit 130
              }
              trap cleanup_mutation_audit EXIT
              trap terminate_mutation_workers INT TERM
              mutation_log_directory="$(mktemp -d -t pokecon-routing-mutations.XXXXXXXX)"
              mutation_test="${productionRoutingAuditTest}::test_production_routing_audit_fails_closed_under_registration_mutations"

              echo "Running 395 production-routing mutations across $mutation_worker_count process shards"
              for ((mutation_shard_index = 0; mutation_shard_index < mutation_worker_count; mutation_shard_index++)); do
                POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_INDEX="$mutation_shard_index" \
                  POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_COUNT="$mutation_worker_count" \
                  "${pythonEnv}/bin/python" -I -m pytest \
                  -c "${auditPytestConfig}" \
                  --noconftest \
                  --import-mode=importlib \
                  -p no:cacheprovider \
                  "$mutation_test" \
                  >"$mutation_log_directory/$mutation_shard_index.log" 2>&1 &
                mutation_worker_pids+=("$!")
              done

              mutation_audit_status=0
              mutation_worker_statuses=()
              for mutation_worker_pid in "''${mutation_worker_pids[@]}"; do
                if wait "$mutation_worker_pid"; then
                  mutation_worker_status=0
                else
                  mutation_worker_status=$?
                  if [ "$mutation_audit_status" -eq 0 ]; then
                    mutation_audit_status="$mutation_worker_status"
                  fi
                fi
                mutation_worker_statuses+=("$mutation_worker_status")
              done
              trap - INT TERM

              for ((mutation_shard_index = 0; mutation_shard_index < mutation_worker_count; mutation_shard_index++)); do
                printf '\n=== production-routing mutation shard %s/%s ===\n' \
                  "$mutation_shard_index" "$mutation_worker_count"
                cat -- "$mutation_log_directory/$mutation_shard_index.log"
                mutation_worker_status="''${mutation_worker_statuses[$mutation_shard_index]}"
                if [ "$mutation_worker_status" -ne 0 ]; then
                  echo "production-routing mutation shard $mutation_shard_index failed with status $mutation_worker_status" >&2
                fi
              done
              exit "$mutation_audit_status"
            '';
          };
          productionRoutingMutationAudit =
            pkgs.runCommand "pokecon-production-routing-mutation-audit"
              {
                nativeBuildInputs = [ productionRoutingMutationAuditRunner ];
              }
              ''
                "${productionRoutingMutationAuditRunner}/bin/pokecon-production-routing-mutation-audit"
                mkdir -p "$out"
                touch "$out/passed"
              '';

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

          sanitizeCargoCompilerEnvironment = ''
            cargo_compiler_environment=()
            while IFS= read -r -d "" cargo_environment_entry; do
              cargo_environment_name="''${cargo_environment_entry%%=*}"
              case "$cargo_environment_name" in
                CARGO_BUILD_RUSTC | CARGO_BUILD_RUSTC_WRAPPER | CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER \
                  | CARGO_BUILD_RUSTFLAGS | CARGO_BUILD_RUSTDOC | CARGO_BUILD_RUSTDOCFLAGS \
                  | CARGO_BUILD_TARGET | CARGO_ENCODED_RUSTFLAGS | CARGO_ENCODED_RUSTDOCFLAGS \
                  | CARGO_TARGET_*_RUSTC | CARGO_TARGET_*_RUSTFLAGS | CARGO_TARGET_*_RUSTDOC \
                  | CARGO_TARGET_*_RUSTDOCFLAGS | CARGO_TARGET_*_RUNNER | CARGO_TARGET_*_LINKER)
                  cargo_compiler_environment+=("$cargo_environment_name")
                  ;;
              esac
            done < <("${pkgs.coreutils}/bin/env" -0)
            for cargo_environment_name in "''${cargo_compiler_environment[@]}"; do
              unset "$cargo_environment_name"
              if [[ -v $cargo_environment_name ]]; then
                echo "failed to clear Cargo compiler environment alias: $cargo_environment_name" >&2
                exit 2
              fi
            done
            unset cargo_compiler_environment cargo_environment_entry cargo_environment_name
          '';

          assertNoRepositoryCargoConfigs = ''
            repository_cargo_configs=()
            repository_cargo_config_inventory=
            if [ -z "''${TMPDIR:-}" ] || [ -L "$TMPDIR" ] || [ ! -d "$TMPDIR" ]; then
              echo "repository Cargo configuration scan requires a real TMPDIR" >&2
              exit 2
            fi
            if ! repository_cargo_config_tmpdir="$(
              "${pkgs.coreutils}/bin/readlink" -f -- "$TMPDIR"
            )"; then
              echo "repository Cargo configuration scan cannot resolve TMPDIR: $TMPDIR" >&2
              exit 2
            fi
            if [ "$repository_cargo_config_tmpdir" != "$TMPDIR" ]; then
              echo "repository Cargo configuration scan TMPDIR is redirected: $TMPDIR" >&2
              exit 2
            fi
            if ! repository_cargo_config_inventory="$(
              "${pkgs.coreutils}/bin/mktemp" \
                --tmpdir="$repository_cargo_config_tmpdir" \
                pokecon-repository-cargo-configs.XXXXXXXX
            )"; then
              echo "failed to create repository Cargo configuration inventory" >&2
              exit 2
            fi
            case "$repository_cargo_config_inventory" in
              "$repository_cargo_config_tmpdir"/*) ;;
              *)
                "${pkgs.coreutils}/bin/rm" -f -- "$repository_cargo_config_inventory"
                echo "repository Cargo configuration inventory escaped TMPDIR" >&2
                exit 2
                ;;
            esac
            if ! repository_cargo_config_inventory_canonical="$(
              "${pkgs.coreutils}/bin/readlink" -f -- "$repository_cargo_config_inventory"
            )"; then
              "${pkgs.coreutils}/bin/rm" -f -- "$repository_cargo_config_inventory"
              echo "repository Cargo configuration inventory cannot be resolved" >&2
              exit 2
            fi
            if [ -L "$repository_cargo_config_inventory" ] \
              || [ ! -f "$repository_cargo_config_inventory" ] \
              || [ "$repository_cargo_config_inventory_canonical" != "$repository_cargo_config_inventory" ] \
              || [ "$("${pkgs.coreutils}/bin/stat" -c %a -- "$repository_cargo_config_inventory")" != 600 ]; then
              "${pkgs.coreutils}/bin/rm" -f -- "$repository_cargo_config_inventory"
              echo "repository Cargo configuration inventory is not a private regular file" >&2
              exit 2
            fi
            if ! "${pkgs.findutils}/bin/find" -P "$PWD" \
              \( \
                -path "$PWD/web/node_modules" \
                -o -path "$PWD/release-python" \
                -o -path "$PWD/release-wheelhouse" \
                -o -path "$PWD/bundle-resources" \
                -o -path "$PWD/normalized-bin" \
              \) -prune \
              -o \( \
                \( -type l -name .cargo \) \
                -o -path '*/.cargo/config' \
                -o -path '*/.cargo/config.toml' \
              \) -print0 > "$repository_cargo_config_inventory"; then
              "${pkgs.coreutils}/bin/rm" -f -- "$repository_cargo_config_inventory"
              echo "repository Cargo configuration scan failed" >&2
              exit 2
            fi
            while IFS= read -r -d "" repository_cargo_config; do
              repository_cargo_configs+=("$repository_cargo_config")
            done < "$repository_cargo_config_inventory"
            "${pkgs.coreutils}/bin/rm" -f -- "$repository_cargo_config_inventory"
            if [ -e "$repository_cargo_config_inventory" ] \
              || [ -L "$repository_cargo_config_inventory" ]; then
              echo "failed to remove repository Cargo configuration inventory" >&2
              exit 2
            fi
            if [ "''${#repository_cargo_configs[@]}" -ne 0 ]; then
              printf 'repository Cargo configuration is forbidden: %s\n' \
                "''${repository_cargo_configs[@]}" >&2
              exit 2
            fi
            unset \
              repository_cargo_config \
              repository_cargo_config_inventory \
              repository_cargo_config_inventory_canonical \
              repository_cargo_config_tmpdir \
              repository_cargo_configs
          '';

          assertNoCargoConfigAncestors = ''
            cargo_config_ancestor="$(pwd -P)"
            while true; do
              cargo_config_directory="''${cargo_config_ancestor%/}/.cargo"
              if [ -L "$cargo_config_directory" ]; then
                echo "ancestor Cargo config directory must not be a symlink: $cargo_config_directory" >&2
                exit 2
              fi
              for cargo_config_name in config config.toml; do
                cargo_config_candidate="$cargo_config_directory/$cargo_config_name"
                if [ -e "$cargo_config_candidate" ] || [ -L "$cargo_config_candidate" ]; then
                  echo "ancestor Cargo configuration is forbidden: $cargo_config_candidate" >&2
                  exit 2
                fi
              done
              if [ "$cargo_config_ancestor" = / ]; then
                break
              fi
              cargo_config_parent="$(dirname -- "$cargo_config_ancestor")"
              if [ "$cargo_config_parent" = "$cargo_config_ancestor" ]; then
                echo "Cargo config ancestor walk did not reach the filesystem root" >&2
                exit 2
              fi
              cargo_config_ancestor="$cargo_config_parent"
            done
            unset \
              cargo_config_ancestor \
              cargo_config_candidate \
              cargo_config_directory \
              cargo_config_name \
              cargo_config_parent
          '';

          resetTauriCargoTarget = ''
            expected_cargo_target_dir="$gate_home/cargo-target"
            if [ -L "$gate_home" ] \
              || [ ! -d "$gate_home" ] \
              || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$gate_home")" != "$gate_home" ]; then
              echo "tauri-build cannot reset Cargo target below a redirected gate home" >&2
              exit 2
            fi
            "${pkgs.coreutils}/bin/rm" -rf -- "$expected_cargo_target_dir"
            "${pkgs.coreutils}/bin/mkdir" -- "$expected_cargo_target_dir"
            if [ -L "$expected_cargo_target_dir" ] \
              || [ ! -d "$expected_cargo_target_dir" ] \
              || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$expected_cargo_target_dir")" != "$expected_cargo_target_dir" ]; then
              echo "tauri-build failed to recreate a canonical Cargo target" >&2
              exit 2
            fi
            if ! cargo_target_residue="$(
              "${pkgs.findutils}/bin/find" -P "$expected_cargo_target_dir" \
                -mindepth 1 -print -quit
            )"; then
              echo "tauri-build Cargo target inventory failed" >&2
              exit 2
            fi
            if [ -n "$cargo_target_residue" ]; then
              echo "tauri-build recreated a non-empty Cargo target: $cargo_target_residue" >&2
              exit 2
            fi
            export CARGO_INCREMENTAL=0
            export CARGO_TARGET_DIR="$expected_cargo_target_dir"
            unset cargo_target_residue expected_cargo_target_dir
          '';

          prepareTauriCargoInvocation = ''
            cargo_source_root="${controlledCargoSource}"
            if [ -L "$cargo_source_root" ] \
              || [ ! -d "$cargo_source_root" ] \
              || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$cargo_source_root")" != "$cargo_source_root" ]; then
              echo "tauri-build controlled Cargo source is redirected or missing" >&2
              exit 2
            fi
            if ! controlled_source_boundary_violation="$(
              "${pkgs.findutils}/bin/find" -P "$cargo_source_root" \
                \( -type l -o -perm /0222 \) -print -quit
            )"; then
              echo "tauri-build controlled Cargo source inventory failed" >&2
              exit 2
            fi
            if [ -n "$controlled_source_boundary_violation" ]; then
              echo "tauri-build controlled Cargo source is redirected or writable: $controlled_source_boundary_violation" >&2
              exit 2
            fi
            unset controlled_source_boundary_violation
            (
              cd "$cargo_source_root"
              ${verifyControlledCargoManifests}
              ${assertNoRepositoryCargoConfigs}
              ${assertNoCargoConfigAncestors}
            )
            if ! "${pkgs.diffutils}/bin/cmp" -s -- \
              "${source}/pyproject.toml" \
              "$cargo_source_root/pyproject.toml"; then
              echo "tauri-build controlled pyproject.toml differs from the immutable source" >&2
              exit 2
            fi
            export CARGO_HOME="${gateCargoHome}"
            if [ -L "$CARGO_HOME" ] \
              || [ ! -d "$CARGO_HOME" ] \
              || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$CARGO_HOME")" != "${gateCargoHome}" ] \
              || [ ! -L "$CARGO_HOME/config.toml" ] \
              || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$CARGO_HOME/config.toml")" != "${gateCargoConfig}" ]; then
              echo "tauri-build immutable Cargo home is not canonical" >&2
              exit 2
            fi
            ${sanitizeCargoCompilerEnvironment}
            expected_cargo_target_dir="$gate_home/cargo-target"
            if [ "''${CARGO_TARGET_DIR:-}" != "$expected_cargo_target_dir" ] \
              || [ "''${CARGO_INCREMENTAL:-}" != 0 ] \
              || [ -L "$CARGO_TARGET_DIR" ] \
              || [ ! -d "$CARGO_TARGET_DIR" ] \
              || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$CARGO_TARGET_DIR")" != "$expected_cargo_target_dir" ]; then
              echo "tauri-build Cargo target boundary is not canonical" >&2
              exit 2
            fi
            unset expected_cargo_target_dir
            if [[ -v CARGO ]] && [ "$CARGO" != "${rustToolchain}/bin/cargo" ]; then
              echo "tauri-build retained an unexpected CARGO: $CARGO" >&2
              exit 2
            fi
            if [[ -v RUSTC ]] && [ "$RUSTC" != "${rustToolchain}/bin/rustc" ]; then
              echo "tauri-build retained an unexpected RUSTC: $RUSTC" >&2
              exit 2
            fi
            if [[ -v RUSTC_WRAPPER ]] && [ "$RUSTC_WRAPPER" != "${reproducibleRustcWrapper}" ]; then
              echo "tauri-build retained an unexpected RUSTC_WRAPPER: $RUSTC_WRAPPER" >&2
              exit 2
            fi
            unset CARGO RUSTC RUSTC_WRAPPER
            for forbidden_rust_environment in \
              RUSTC_WORKSPACE_WRAPPER RUSTFLAGS CARGO_ENCODED_RUSTFLAGS; do
              if [[ -v $forbidden_rust_environment ]]; then
                echo "tauri-build retained forbidden ambient Rust configuration: $forbidden_rust_environment" >&2
                exit 2
              fi
            done
            unset forbidden_rust_environment
            export CARGO="${rustToolchain}/bin/cargo"
            export RUSTC="${rustToolchain}/bin/rustc"
            export RUSTC_WRAPPER="${reproducibleRustcWrapper}"
            if [ "$CARGO" != "${rustToolchain}/bin/cargo" ] \
              || [ "$RUSTC" != "${rustToolchain}/bin/rustc" ] \
              || [ "$RUSTC_WRAPPER" != "${reproducibleRustcWrapper}" ] \
              || [ "$CARGO_HOME" != "${gateCargoHome}" ]; then
              echo "tauri-build Cargo invocation boundary is not canonical" >&2
              exit 2
            fi
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

          restoreGateCargoConfig = ''
            expected_cargo_home="$gate_home/cargo-home"
            export CARGO_HOME="$expected_cargo_home"
            if [ -L "$CARGO_HOME" ] || [ ! -d "$CARGO_HOME" ] \
              || [ "$(readlink -f "$CARGO_HOME")" != "$expected_cargo_home" ]; then
              echo "isolated Cargo home is redirected or missing: $CARGO_HOME" >&2
              exit 2
            fi
            rm -rf -- "$CARGO_HOME/config" "$CARGO_HOME/config.toml"
            ln -s -- "${gateCargoConfig}" "$CARGO_HOME/config.toml"
            if [ -e "$CARGO_HOME/config" ] || [ -L "$CARGO_HOME/config" ] \
              || [ ! -L "$CARGO_HOME/config.toml" ] \
              || [ "$(readlink -- "$CARGO_HOME/config.toml")" != "${gateCargoConfig}" ] \
              || [ "$(readlink -f "$CARGO_HOME/config.toml")" != "${gateCargoConfig}" ]; then
              echo "failed to restore the immutable Cargo gate config" >&2
              exit 2
            fi
            unset expected_cargo_home
          '';

          setupIsolatedCargoHome = ''
            export CARGO_HOME="$gate_home/cargo-home"
            mkdir -p "$CARGO_HOME"
            ${restoreGateCargoConfig}
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
            cargo_target_dir="$gate_home/target"
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
            cargo_cache_tag="$cargo_target_dir/CACHEDIR.TAG"
            if ! (set -o noclobber; umask 022; "${pkgs.coreutils}/bin/cat" "${cargoCacheDirectoryTag}" >"$cargo_cache_tag"); then
              echo "failed to create the canonical Cargo cache directory tag: $cargo_cache_tag" >&2
              exit 2
            fi
            if [ -L "$cargo_cache_tag" ] || [ ! -f "$cargo_cache_tag" ] \
              || [ "$(readlink -f "$cargo_cache_tag")" != "$cargo_cache_tag" ]; then
              echo "per-run Cargo cache directory tag is redirected or not a regular file: $cargo_cache_tag" >&2
              exit 2
            fi
            if ! "${pkgs.diffutils}/bin/cmp" -s -- "${cargoCacheDirectoryTag}" "$cargo_cache_tag"; then
              echo "per-run Cargo cache directory tag differs from Cargo's canonical tag: $cargo_cache_tag" >&2
              exit 2
            fi
            export CARGO_INCREMENTAL=0
            export CARGO_TARGET_DIR="$cargo_target_dir"
            echo "using isolated per-run Cargo target: $CARGO_TARGET_DIR" >&2
            unset canonical_gate_home cargo_cache_tag cargo_target_dir
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

          cargoCacheDirectoryTag = pkgs.writeText "pokecon-cargo-cache-directory-tag" ''
            Signature: 8a477f597d28d172789f06886806bc55
            # This file is a cache directory tag created by cargo.
            # For information about cache directory tags see https://bford.info/cachedir/
          '';

          setupWorkdir = ''
            ${sanitizeGateEnvironment}
            ${discoverRustWorktree}
            workdir=
            gate_home=
            cleanup_gate_directories() {
              gate_status=$?
              trap - EXIT
              gate_cleanup_status=0
              if declare -F pokecon_cleanup_task_artifacts >/dev/null; then
                pokecon_cleanup_task_artifacts || gate_cleanup_status=$?
              fi
              if [ -n "$workdir" ]; then
                if ! "${pkgs.coreutils}/bin/rm" -rf -- "$workdir"; then
                  echo "failed to remove the gate worktree: $workdir" >&2
                  gate_cleanup_status=1
                fi
              fi
              if [ -n "$gate_home" ]; then
                if ! "${pkgs.coreutils}/bin/rm" -rf -- "$gate_home"; then
                  echo "failed to remove the gate home: $gate_home" >&2
                  gate_cleanup_status=1
                fi
              fi
              if [ "$gate_status" -eq 0 ] && [ "$gate_cleanup_status" -ne 0 ]; then
                gate_status=$gate_cleanup_status
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
            ${assertNoCargoConfigAncestors}
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
          linuxApplicationRuntimePackages = lib.optionals pkgs.stdenv.isLinux [
            pkgs.libayatana-appindicator
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
            POKECON_WEB_VERSION = workspaceVersion;
            SOURCE_DATE_EPOCH = "0";
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

          linuxReleaseRuntime =
            if system == "x86_64-linux" then
              pkgs.stdenvNoCC.mkDerivation {
                pname = "pokecon-linux-release-runtime";
                version = workspaceVersion;
                nativeBuildInputs = [
                  pkgs.cacert
                  pkgs.patchelf
                  pythonEnv
                ];
                phases = [ "buildPhase" ];
                buildPhase = ''
                  runHook preBuild
                  mkdir -p "$TMPDIR/release-home" "$TMPDIR/uv-cache"
                  cp "${linuxReleaseEvdevConfig}" "$TMPDIR/release-home/.pydistutils.cfg"
                  "${pkgs.coreutils}/bin/env" -i \
                    AR="${linuxReleaseCc}/bin/ar" \
                    AS="${linuxReleaseCc}/bin/as" \
                    CC="${linuxReleaseCc}/bin/gcc" \
                    CFLAGS="-I${linuxReleasePortaudio}/include" \
                    CPP="${linuxReleaseCc}/bin/cpp" \
                    CXX="${linuxReleaseCc}/bin/g++" \
                    HOME="$TMPDIR/release-home" \
                    LANG=C.UTF-8 \
                    LC_ALL=C.UTF-8 \
                    LD="${linuxReleaseCc}/bin/ld" \
                    LDFLAGS="-L${linuxReleasePortaudio}/lib" \
                    NIX_SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" \
                    PATH="${linuxReleaseBuildPath}" \
                    PIP_CONFIG_FILE=/dev/null \
                    PKG_CONFIG_LIBDIR="${linuxReleasePortaudio}/lib/pkgconfig" \
                    PKG_CONFIG_PATH="${linuxReleasePortaudio}/lib/pkgconfig" \
                    PYTHONHASHSEED=0 \
                    PYTHONDONTWRITEBYTECODE=1 \
                    PYTHONNOUSERSITE=1 \
                    PYTHONSAFEPATH=1 \
                    RANLIB="${linuxReleaseCc}/bin/ranlib" \
                    SOURCE_DATE_EPOCH=0 \
                    SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" \
                    STRIP="${linuxReleaseCc}/bin/strip" \
                    TMPDIR="$TMPDIR" \
                    TZ=UTC \
                    UV_CACHE_DIR="$TMPDIR/uv-cache" \
                    UV_LIBC=gnu \
                    UV_NO_CONFIG=1 \
                    XDG_CACHE_HOME="$TMPDIR/release-home/.cache" \
                    XDG_CONFIG_HOME="$TMPDIR/release-home/.config" \
                    "${pythonEnv}/bin/python" -I "${source}/scripts/release/build_runtime.py" \
                    --project "${controlledCargoSource}" \
                    --uv "${portableUvExecutionBinary}" \
                    --runtime-output "$out/python" \
                    --wheelhouse-output "$out/wheelhouse" \
                    --patchelf "${pkgs.patchelf}/bin/patchelf" \
                    --strip "${pkgs.binutils}/bin/strip" \
                    --execution-loader "${portableUvExecutionLoader}" \
                    --execution-library-path "${portableUvExecutionLibraryPath}" \
                    --runtime-library-path "${linuxReleaseRuntimeLibraries}/lib"
                  if [ -n "$(
                    "${pkgs.findutils}/bin/find" "$out" \
                      \( -type d -name __pycache__ -o -type f -name '*.pyc' \) \
                      -print -quit
                  )" ]; then
                    echo "fixed release runtime contains Python bytecode cache artifacts" >&2
                    exit 2
                  fi
                  runHook postBuild
                '';
                dontFixup = true;
                outputHash = "sha256-/oX5m7mZkIwXl383NXN4qc2qGnfsJSlPMgVKMrurSmY=";
                outputHashMode = "recursive";
              }
            else
              null;

          pokeconPackage = rustPlatform.buildRustPackage {
            pname = "pokecon";
            version = workspaceVersion;
            src = source;
            nativeBuildInputs = [
              pkgs.nasm
              pkgs.pkg-config
            ]
            ++ lib.optionals pkgs.stdenv.isLinux [
              pkgs.llvmPackages.libclang
              pkgs.patchelf
            ];
            buildInputs = [ pythonEnv ] ++ linuxDesktopPackages ++ linuxApplicationRuntimePackages;
            cargoLock = {
              lockFile = ./Cargo.lock;
              allowBuiltinFetchGit = true;
            };
            cargoBuildFlags = [
              "--locked"
              "--package"
              "pokecon"
              "--package"
              "pokecon-worker"
              "--bin"
              "pokecon"
              "--bin"
              "pokecon-worker"
            ];
            # Source correctness is owned once by rust-ci-core. The package
            # gates below execute the exact store binaries instead of rebuilding
            # the same tests inside this product derivation.
            doCheck = false;
            POKECON_RESOURCE_PROVENANCE = "nix-exact";
            postPatch = ''
              test -f "${productionRoutingAudit}/passed"
              ${installControlledCargoManifests}
            '';
            preBuild = ''
              ${installControlledCargoManifests}
              ${sanitizeCargoCompilerEnvironment}
              if [ "''${RUSTC:-}" != "${rustToolchain}/bin/rustc" ]; then
                echo "pokecon package build has an unpinned RUSTC: ''${RUSTC:-<unset>}" >&2
                exit 2
              fi
              if [ "''${RUSTC_WRAPPER:-}" != "${pinnedRustcWrapper}" ]; then
                echo "pokecon package build has an unpinned RUSTC_WRAPPER: ''${RUSTC_WRAPPER:-<unset>}" >&2
                exit 2
              fi
              for forbidden_package_rust_environment in RUSTC_WORKSPACE_WRAPPER RUSTFLAGS; do
                if [[ -v $forbidden_package_rust_environment ]]; then
                  echo "pokecon package build forbids $forbidden_package_rust_environment" >&2
                  exit 2
                fi
              done
              unset forbidden_package_rust_environment
            '';
            installPhase = ''
              runHook preInstall
              : "''${cargoBuildType:?cargoBuildType is required}"
              package_target="target/${pkgs.stdenv.targetPlatform.rust.cargoShortTarget}/$cargoBuildType"
              for packaged_binary in pokecon pokecon-worker; do
                packaged_source="$package_target/$packaged_binary"
                if [ -L "$packaged_source" ] || [ ! -f "$packaged_source" ] || [ ! -x "$packaged_source" ]; then
                  echo "packaged executable is missing, redirected, or not executable: $packaged_source" >&2
                  exit 2
                fi
                "${pkgs.coreutils}/bin/install" -Dm755 -- \
                  "$packaged_source" "$out/bin/$packaged_binary"
              done
              unset packaged_binary packaged_source package_target
              runHook postInstall
            '';
            POKECON_BUILD_UV_PATH = "${pkgs.uv}/bin/uv";
            POKECON_BUILD_UV_VERSION = pkgs.uv.version;
            POKECON_INTERNAL_SCRIPT_SITE_PACKAGES = "${pythonEnv}/${pkgs.python314.sitePackages}";
            PYO3_PYTHON = "${pythonEnv}/bin/python";
            POKECON_BUILD_PYTHON = "${pythonEnv}/bin/python";
            RUSTC = "${rustToolchain}/bin/rustc";
            RUSTC_WRAPPER = "${pinnedRustcWrapper}";
            BINDGEN_EXTRA_CLANG_ARGS = linuxBindgenArgs;
            LIBCLANG_PATH = lib.optionalString pkgs.stdenv.isLinux "${pkgs.llvmPackages.libclang.lib}/lib";
            postInstall = ''
              mkdir -p "$out/web/dist"
              cp -R "${webPackage}/." "$out/web/dist/"
              ln -s ../web "$out/bin/web"
              mkdir -p "$out/bin/uv"
              cp "${pkgs.uv}/bin/uv" "$out/bin/uv/uv"

              retired_native_extension="$(${pkgs.findutils}/bin/find "$out" \( -type f -o -type l \) \( \
                -path '*/pokecon/_native*.so' -o \
                -path '*/pokecon/_native*.pyd' -o \
                -path '*/pokecon/_native*.dylib' -o \
                -path '*/pokecon/_native*.dll' \
              \) -print -quit)"
              if [ -n "$retired_native_extension" ]; then
                echo "retired first-party Python native extension leaked into package: $retired_native_extension" >&2
                exit 1
              fi
              retired_project_wheel="$(${pkgs.findutils}/bin/find "$out" \( -type f -o -type l \) \
                -name 'poke_controller_modified_extension-*.whl' -print -quit)"
              if [ -n "$retired_project_wheel" ]; then
                echo "retired first-party Python wheel leaked into package: $retired_project_wheel" >&2
                exit 1
              fi
            '';
            postFixup = lib.optionalString pkgs.stdenv.isLinux ''
              application_runtime_path=${lib.escapeShellArg (lib.makeLibraryPath linuxApplicationRuntimePackages)}
              patchelf --add-rpath "$application_runtime_path" "$out/bin/pokecon"
              patched_rpath="$(patchelf --print-rpath "$out/bin/pokecon")"
              case ":$patched_rpath:" in
                *":$application_runtime_path:"*) ;;
                *)
                  echo "packaged application RPATH does not contain its AppIndicator closure" >&2
                  exit 1
                  ;;
              esac
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
          gateCargoHome = pkgs.runCommand "pokecon-gate-cargo-home" { } ''
            "${pkgs.coreutils}/bin/mkdir" -p "$out"
            "${pkgs.coreutils}/bin/ln" -s -- "${gateCargoConfig}" "$out/config.toml"
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
          workerPackageCheck = mkTask {
            name = "worker-package-check";
            runtimeInputs = rustTaskInputs ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.strace ];
            text = ''
              if [ "$#" -ne 0 ]; then
                echo "usage: nix run .#worker-package-check" >&2
                exit 2
              fi
            ''
            + (
              if pkgs.stdenv.isLinux then
                ''
                  ${setupWorkdir}
                  ${desktopEnvironment}

                  package_output="${self'.packages.pokecon}"
                  application="$package_output/bin/pokecon"
                  worker_binary="$(dirname -- "$application")/pokecon-worker"
                  for packaged_binary in "$application" "$worker_binary"; do
                    if [ ! -f "$packaged_binary" ] || [ ! -x "$packaged_binary" ]; then
                      echo "packaged executable is missing or not executable: $packaged_binary" >&2
                      exit 1
                    fi
                  done
                  store_directory=${lib.escapeShellArg builtins.storeDir}
                  canonical_package="$(readlink -f -- "$package_output")"
                  canonical_application="$(readlink -f -- "$application")"
                  canonical_worker="$(readlink -f -- "$worker_binary")"
                  if [ "$(dirname -- "$canonical_package")" != "$store_directory" ]; then
                    echo "package is not a direct output under evaluator store $store_directory: $canonical_package" >&2
                    exit 1
                  fi
                  case "$canonical_application" in
                    "$canonical_package"/*) ;;
                    *)
                      echo "application escapes the selected package output: $canonical_application" >&2
                      exit 1
                      ;;
                  esac
                  case "$canonical_worker" in
                    "$canonical_package"/*) ;;
                    *)
                      echo "worker escapes the selected package output: $canonical_worker" >&2
                      exit 1
                      ;;
                  esac
                  if [ "$(dirname -- "$canonical_application")" != "$(dirname -- "$canonical_worker")" ]; then
                    echo "worker is not the packaged application's sibling: $canonical_worker" >&2
                    exit 1
                  fi
                  application="$canonical_application"
                  worker_binary="$canonical_worker"

                  product_root="$gate_home/packaged-product"
                  product_home="$product_root/home"
                  product_config="$product_root/config"
                  product_data="$product_root/data"
                  product_cache="$product_root/cache"
                  product_state="$product_root/state"
                  product_runtime="$product_root/runtime"
                  product_tmp="$product_root/tmp"
                  product_appdata="$product_root/appdata"
                  product_localappdata="$product_root/localappdata"
                  profile_root="$product_config/pokecon/profiles/default"
                  mkdir -p \
                    "$product_home" \
                    "$product_config/pokecon" \
                    "$product_data" \
                    "$product_cache" \
                    "$product_state" \
                    "$product_runtime" \
                    "$product_tmp" \
                    "$product_appdata" \
                    "$product_localappdata"
                  chmod 0700 "$product_runtime"

                  product_marker="worker-package-check-lua-marker-AR-13.1-26"
                  printf 'print("%s")\n' "$product_marker" > "$product_config/pokecon/init.lua"
                  product_stdout="$product_root/pokecon.stdout"
                  product_stderr="$product_root/pokecon.stderr"
                  product_trace="$product_root/pokecon.execve"
                  product_port="$(
                    "${pythonEnv}/bin/python" -I -S -c \
                      'import socket; sock = socket.socket(); sock.bind(("127.0.0.1", 0)); print(sock.getsockname()[1]); sock.close()'
                  )"

                  set +e
                  "${pkgs.coreutils}/bin/env" -i \
                    HOME="$product_home" \
                    USERPROFILE="$product_home" \
                    XDG_CONFIG_HOME="$product_config" \
                    XDG_DATA_HOME="$product_data" \
                    XDG_CACHE_HOME="$product_cache" \
                    XDG_STATE_HOME="$product_state" \
                    XDG_RUNTIME_DIR="$product_runtime" \
                    TMPDIR="$product_tmp" \
                    APPDATA="$product_appdata" \
                    LOCALAPPDATA="$product_localappdata" \
                    LANG=C \
                    LC_ALL=C \
                    TZ=UTC \
                    PATH="${lib.makeBinPath [ pkgs.coreutils ]}" \
                    POKECON_WEB_DIR="$package_output/bin/web/dist" \
                    RUST_LOG=info \
                    "${pkgs.coreutils}/bin/timeout" --signal=TERM --kill-after=10s 120s \
                    "${pkgs.strace}/bin/strace" \
                      -f -qq -s 4096 -e trace=execve -o "$product_trace" \
                      "$application" \
                        --ui web \
                        --port "$product_port" \
                        --dynamic-config-language lua \
                        --exit-after-startup \
                      >"$product_stdout" 2>"$product_stderr"
                  product_status=$?
                  set -e

                  show_product_logs() {
                    if [ -s "$product_stdout" ]; then
                      echo "packaged product stdout:" >&2
                      head -n 200 "$product_stdout" >&2
                    fi
                    if [ -s "$product_stderr" ]; then
                      echo "packaged product stderr:" >&2
                      head -n 200 "$product_stderr" >&2
                    fi
                    if [ -s "$product_trace" ]; then
                      echo "packaged product execve trace:" >&2
                      head -n 200 "$product_trace" >&2
                    fi
                  }
                  if [ "$product_status" -ne 0 ]; then
                    echo "packaged product startup failed with status $product_status" >&2
                    show_product_logs
                    exit 1
                  fi
                  if grep -F -- \
                    'dynamic configuration is unavailable; continuing with static settings' \
                    "$product_stdout" "$product_stderr" >/dev/null; then
                    echo "packaged product fell back to static settings after dynamic startup failure" >&2
                    show_product_logs
                    exit 1
                  fi
                  if grep -F -- \
                    'dynamic startup configuration was rejected' \
                    "$product_stdout" "$product_stderr" >/dev/null; then
                    echo "packaged dynamic worker rejected its startup configuration" >&2
                    show_product_logs
                    exit 1
                  fi
                  if ! grep -F -- "$product_marker" "$product_stdout" "$product_stderr" >/dev/null; then
                    echo "packaged dynamic worker did not evaluate the isolated Lua marker" >&2
                    show_product_logs
                    exit 1
                  fi
                  if ! grep -F -- '"message":"dynamic worker stopped"' "$product_stdout" "$product_stderr" >/dev/null; then
                    echo "packaged product did not report dynamic-worker shutdown" >&2
                    show_product_logs
                    exit 1
                  fi
                  if ! grep -F -- '"cooperative_acknowledged":true' "$product_stdout" "$product_stderr" >/dev/null; then
                    echo "packaged product did not report cooperative dynamic-worker stop" >&2
                    show_product_logs
                    exit 1
                  fi
                  expected_exec='execve("'"$worker_binary"'", ["'"$worker_binary"'", "--kind", "dynamic"]'
                  if ! grep -F -- "$expected_exec" "$product_trace" >/dev/null; then
                    echo "product did not exec the exact packaged sibling as its dynamic worker" >&2
                    show_product_logs
                    exit 1
                  fi
                  if [ ! -d "$profile_root" ]; then
                    echo "packaged product did not scaffold its isolated default profile: $profile_root" >&2
                    show_product_logs
                    exit 1
                  fi

                  export POKECON_TEST_WORKER_BINARY="$worker_binary"
                  cargo test --locked --package pokecon-worker --test startup -- --nocapture
                  cargo test --locked --package pokecon-worker --test lifecycle -- --nocapture
                  cargo test --locked --package pokecon-worker --test script_runtime \
                    script_worker_executes_controller_serial_and_output_proxies \
                    -- --exact --nocapture

                  printf '%s\n' \
                    "worker-package-check: PASS" \
                    "store_directory=$store_directory" \
                    "package=$canonical_package" \
                    "application=$application" \
                    "worker=$worker_binary" \
                    "roles=script,dynamic" \
                    "product_profile=$profile_root" \
                    "test_profiles=per-test-temporary-roots" \
                    "product_resolution=exact-sibling-execve+lua-marker" \
                    "ipc=framed-bidirectional" \
                    "script_direction=controller+serial+output" \
                    "dynamic_languages=python,lua" \
                    "packaged_stop=cooperative" \
                    "supervisor_fault_policy=source-built-fixture-force+generation-retention" \
                    "product_stdout=$product_stdout" \
                    "product_stderr=$product_stderr" \
                    "product_execve_trace=$product_trace"
                  grep -h -F -m 1 -- "$product_marker" "$product_stdout" "$product_stderr"
                  grep -h -F -m 1 -- '"message":"dynamic worker stopped"' "$product_stdout" "$product_stderr"
                ''
              else
                ''
                  echo "worker-package-check requires Linux process tracing" >&2
                  exit 2
                ''
            );
          };
          uiPackageSoftwareRenderer = pkgs.mesa;
          uiPackageSessionBusConfig = pkgs.writeTextFile {
            name = "pokecon-ui-package-session-bus-config";
            destination = "/share/dbus-1/session.conf";
            text = ''
              <busconfig>
                <type>session</type>
                <keep_umask/>
                <listen>unix:runtime=yes</listen>
                <auth>EXTERNAL</auth>
                <policy context="default">
                  <allow send_destination="*" eavesdrop="true"/>
                  <allow eavesdrop="true"/>
                  <allow own="*"/>
                </policy>
              </busconfig>
            '';
          };
          uiPackageCheck = mkTask {
            name = "ui-package-check";
            runtimeInputs = [
              pkgs.python314
              pkgs.curl
              pkgs.dbus
              pkgs.diffutils
              pkgs.findutils
              pkgs.gnugrep
              pkgs.jq
              pkgs.procps
              pkgs.util-linux
              pkgs.xauth
              pkgs.xdotool
              pkgs.xprop
              pkgs.xwininfo
              pkgs.xvfb-run
            ];
            text = ''
              if [ "$#" -ne 0 ]; then
                echo "usage: nix run .#ui-package-check" >&2
                exit 2
              fi
            ''
            + (
              if pkgs.stdenv.isLinux then
                ''
                  ${setupSourceGateEnvironment}
                  "${pkgs.bash}/bin/bash" \
                    "${source}/scripts/integration/ui_package_check.sh" \
                    "${self'.packages.pokecon}" \
                    "${pkgs.python314}/bin/python3.14" \
                    ${lib.escapeShellArg builtins.storeDir} \
                    "$gate_home" \
                    "${pkgs.bash}/bin/bash" \
                    "${lib.makeBinPath [ pkgs.coreutils ]}" \
                    "${uiPackageSessionBusConfig}/share/dbus-1/session.conf" \
                    "${uiPackageSoftwareRenderer}" \
                    "${source}/scripts/integration/proc_socket_evidence.py" \
                    "${source}/scripts/integration/pidfd_signal.py" \
                    "${source}/scripts/integration/ewmh_close_relay.py" \
                    "${source}/api/openapi.json"
                ''
              else
                ''
                  echo "ui-package-check requires Linux desktop process inspection" >&2
                  exit 2
                ''
            );
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
          checks.production-routing-audit = productionRoutingAudit;
          checks.production-routing-mutation-audit = productionRoutingMutationAudit;
          checks.web = webPackage;

          apps = {
            default = mkApp "${self'.packages.pokecon}/bin/pokecon";
            fmt = mkApp "${safeFormatter}/bin/pokecon-format";
            cli-help-check = cliHelpCheck;
            ui-package-check = uiPackageCheck;
            worker-package-check = workerPackageCheck;

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
                POKECON_RESOURCE_PROVENANCE=development "${rustToolchain}/bin/cargo" "$@"
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
                export POKECON_RESOURCE_PROVENANCE=development
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

            rust-ci-core = mkTask {
              name = "rust-ci-core";
              runtimeInputs = rustTaskInputs;
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD"
                POKECON_RESOURCE_PROVENANCE=development cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
                POKECON_RESOURCE_PROVENANCE=development \
                cargo build --locked --workspace --all-features
                POKECON_RESOURCE_PROVENANCE=development cargo test --locked --workspace --all-features
                python -m scripts.compatibility.promote --check
                python -m scripts.compatibility.runner \
                  --check \
                  --compatibility-binary "$CARGO_TARGET_DIR/debug/pokecon-compatibility" \
                  --worker "$CARGO_TARGET_DIR/debug/pokecon-worker" \
                  --site-packages "${pythonEnv}/${pkgs.python314.sitePackages}"
              '';
            };

            clippy = mkTask {
              name = "clippy";
              runtimeInputs = rustTaskInputs;
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                POKECON_RESOURCE_PROVENANCE=development cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
              '';
            };

            build-rust = mkTask {
              name = "build-rust";
              runtimeInputs = rustTaskInputs;
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                POKECON_RESOURCE_PROVENANCE=development \
                cargo build --locked --workspace --all-features
              '';
            };

            cargo-test = mkTask {
              name = "cargo-test";
              runtimeInputs = rustTaskInputs;
              text = ''
                ${setupWorkdir}
                ${desktopEnvironment}
                POKECON_RESOURCE_PROVENANCE=development cargo test --locked --workspace --all-features
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
                export POKECON_RESOURCE_PROVENANCE=development
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
                POKECON_RESOURCE_PROVENANCE=development cargo run --locked --package pokecon --bin generate_contracts --features contract-generator -- "$@"
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

            signing-input-manifest = mkTask {
              name = "signing-input-manifest";
              runtimeInputs = [ pythonEnv ];
              text = ''
                ${sanitizeGateEnvironment}
                ${discoverRustWorktree}
                cd "$caller_dir"
                export PYTHONDONTWRITEBYTECODE=1
                python -I "${source}/scripts/release/signing_manifest.py" "$@"
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
                pkgs.gnugrep
                pkgs.jq
                pythonPackageBuildUv
              ];
              text = ''
                ${setupSourceGateEnvironment}
                cd "${source}"
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="${source}/python:${source}"
                export POKECON_TEST_UV="${pythonPackageBuildUv}/bin/uv"
                pytest_arguments=("$@")
                if [ "''${#pytest_arguments[@]}" -eq 0 ]; then
                  pytest_arguments=(tests)
                fi
                python -m pytest \
                  -p no:cacheprovider \
                  -v \
                  --tb=short \
                  "''${pytest_arguments[@]}" \
                  -m "not production_routing_mutation"
              '';
            };

            test-production-routing-mutations = mkTask {
              name = "test-production-routing-mutations";
              text = ''
                exec "${productionRoutingMutationAuditRunner}/bin/pokecon-production-routing-mutation-audit" "$@"
              '';
            };

            tauri-build =
              if system != "x86_64-linux" then
                mkTask {
                  name = "tauri-build";
                  text = ''
                    echo "tauri-build supports only x86_64-linux" >&2
                    exit 2
                  '';
                }
              else
                mkTask {
                  name = "tauri-build";
                  runtimeInputs = rustTaskInputs ++ [
                    pkgs.binutils
                    pkgs.cargo-tauri
                    pkgs.dpkg
                    pkgs.patchelf
                  ];
                  text = ''
                    if [ "$#" -eq 0 ]; then
                      :
                    elif [ "$#" -eq 2 ] && [ "$1" = --bundles ] && [ "$2" = deb ]; then
                      :
                    else
                      echo "usage: nix run .#tauri-build -- [--bundles deb]" >&2
                      exit 2
                    fi
                    bundle_args=(--bundles deb)
                    # Nix realizes this app's interpolated closure before the launcher can
                    # run. At the first executable boundary, hide any prior canonical
                    # artifact before workspace setup and retain it only under a hidden name.
                    ${sanitizeGateEnvironment}
                    ${discoverRustWorktree}
                    artifact_parent="$caller_dir/dist"
                    artifact_replaced=0
                    artifact_stale_canonical_identity=
                    artifact_backup_identity=
                    artifact_publish_identity=
                    artifact_committed_identity=
                    if [ -L "$artifact_parent" ]; then
                      echo "tauri-build artifact parent must not be a symlink: $artifact_parent" >&2
                      exit 2
                    fi
                    "${pkgs.coreutils}/bin/mkdir" -p -- "$artifact_parent"
                    if [ ! -d "$artifact_parent" ] \
                      || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$artifact_parent")" != "$artifact_parent" ]; then
                      echo "tauri-build artifact parent is redirected or invalid: $artifact_parent" >&2
                      exit 2
                    fi
                    exec {artifact_parent_fd}< "$artifact_parent"
                    artifact_parent_anchor="/proc/self/fd/$artifact_parent_fd"
                    if [ ! -d "$artifact_parent_anchor" ]; then
                      echo "tauri-build artifact parent descriptor is not a directory" >&2
                      exit 2
                    fi
                    artifact_parent_path_identity="$(
                      "${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_parent"
                    )"
                    artifact_parent_fd_identity="$(
                      "${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_parent_anchor"
                    )"
                    if [ "$artifact_parent_path_identity" != "$artifact_parent_fd_identity" ]; then
                      echo "tauri-build artifact parent descriptor does not match its path" >&2
                      exit 2
                    fi
                    artifact_parent_anchor_identity_is_owned() {
                      local current_artifact_parent_anchor_identity
                      if [ ! -d "$artifact_parent_anchor" ]; then
                        return 1
                      fi
                      if ! current_artifact_parent_anchor_identity="$(
                        "${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_parent_anchor"
                      )"; then
                        return 1
                      fi
                      if [ "$current_artifact_parent_anchor_identity" \
                        != "$artifact_parent_fd_identity" ]; then
                        return 1
                      fi
                      return 0
                    }
                    artifact_parent_identity_is_current() {
                      local current_artifact_parent_identity current_artifact_parent_path
                      if ! artifact_parent_anchor_identity_is_owned \
                        || [ -L "$artifact_parent" ] || [ ! -d "$artifact_parent" ]; then
                        return 1
                      fi
                      if ! current_artifact_parent_path="$(
                        "${pkgs.coreutils}/bin/readlink" -f -- "$artifact_parent"
                      )"; then
                        return 1
                      fi
                      if [ "$current_artifact_parent_path" != "$artifact_parent" ]; then
                        return 1
                      fi
                      if ! current_artifact_parent_identity="$(
                        "${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_parent"
                      )"; then
                        return 1
                      fi
                      if [ "$current_artifact_parent_identity" != "$artifact_parent_fd_identity" ]; then
                        return 1
                      fi
                      return 0
                    }
                    "${pkgs.util-linux}/bin/flock" -x "$artifact_parent_fd"
                    if ! artifact_parent_identity_is_current; then
                      echo "tauri-build artifact parent changed while waiting for its directory lock" >&2
                      exit 2
                    fi
                    artifact_dir="$artifact_parent_anchor/tauri"
                    artifact_dir_expected="$artifact_parent/tauri"
                    artifact_backup_dir="$artifact_parent_anchor/.tauri-previous"
                    artifact_publish_dir="$artifact_parent_anchor/.tauri-publish"
                    artifact_publish_dir_expected="$artifact_parent/.tauri-publish"
                    artifact_legacy_lock="$artifact_parent_anchor/.tauri-build.lock"

                    remove_saved_artifact_directory() {
                      local artifact_cleanup_actual_identity artifact_cleanup_expected_identity
                      local artifact_cleanup_label artifact_cleanup_target
                      artifact_cleanup_label=$1
                      artifact_cleanup_target=$2
                      artifact_cleanup_expected_identity=$3
                      if ! artifact_parent_anchor_identity_is_owned; then
                        echo "refusing $artifact_cleanup_label cleanup after artifact parent descriptor identity changed" >&2
                        return 1
                      fi
                      if [ -z "$artifact_cleanup_expected_identity" ] \
                        || [ -L "$artifact_cleanup_target" ] \
                        || [ ! -d "$artifact_cleanup_target" ]; then
                        echo "refusing $artifact_cleanup_label cleanup without an owned real directory" >&2
                        return 1
                      fi
                      if ! artifact_cleanup_actual_identity="$(
                        "${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_cleanup_target"
                      )"; then
                        echo "failed to inspect $artifact_cleanup_label before cleanup" >&2
                        return 1
                      fi
                      if [ "$artifact_cleanup_actual_identity" != "$artifact_cleanup_expected_identity" ]; then
                        echo "refusing $artifact_cleanup_label cleanup after its identity changed" >&2
                        return 1
                      fi
                      if ! "${pkgs.coreutils}/bin/rm" -rf -- "$artifact_cleanup_target"; then
                        echo "failed to remove owned $artifact_cleanup_label" >&2
                        return 1
                      fi
                      if [ -e "$artifact_cleanup_target" ] || [ -L "$artifact_cleanup_target" ]; then
                        echo "owned $artifact_cleanup_label remains after cleanup" >&2
                        return 1
                      fi
                      return 0
                    }

                    pokecon_cleanup_task_artifacts() {
                      local artifact_cleanup_status
                      artifact_cleanup_status=0
                      if [ "$artifact_replaced" -eq 1 ] \
                        && { [ -e "$artifact_dir" ] || [ -L "$artifact_dir" ]; }; then
                        remove_saved_artifact_directory \
                          "published canonical artifact" \
                          "$artifact_dir" \
                          "$artifact_committed_identity" \
                          || artifact_cleanup_status=1
                      fi
                      if [ -e "$artifact_publish_dir" ] || [ -L "$artifact_publish_dir" ]; then
                        remove_saved_artifact_directory \
                          "publication staging artifact" \
                          "$artifact_publish_dir" \
                          "$artifact_publish_identity" \
                          || artifact_cleanup_status=1
                      fi
                      if [ -n "$artifact_stale_canonical_identity" ] \
                        && { [ -e "$artifact_dir" ] || [ -L "$artifact_dir" ]; }; then
                        remove_saved_artifact_directory \
                          "stale canonical artifact" \
                          "$artifact_dir" \
                          "$artifact_stale_canonical_identity" \
                          || artifact_cleanup_status=1
                      fi
                      return "$artifact_cleanup_status"
                    }

                    fail_closed_tauri_artifact_preflight() {
                      gate_status=$?
                      trap - EXIT
                      artifact_cleanup_status=0
                      pokecon_cleanup_task_artifacts || artifact_cleanup_status=$?
                      if [ "$gate_status" -eq 0 ] && [ "$artifact_cleanup_status" -ne 0 ]; then
                        gate_status=$artifact_cleanup_status
                      fi
                      exit "$gate_status"
                    }
                    trap fail_closed_tauri_artifact_preflight EXIT
                    for artifact_existing_name in tauri .tauri-previous .tauri-publish; do
                      artifact_existing_dir="$artifact_parent_anchor/$artifact_existing_name"
                      artifact_existing_expected="$artifact_parent/$artifact_existing_name"
                      if [ -e "$artifact_existing_dir" ] || [ -L "$artifact_existing_dir" ]; then
                        if [ -L "$artifact_existing_dir" ] || [ ! -d "$artifact_existing_dir" ] \
                          || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$artifact_existing_dir")" \
                            != "$artifact_existing_expected" ]; then
                          echo "tauri-build artifact boundary is redirected or invalid: $artifact_existing_dir" >&2
                          exit 2
                        fi
                        artifact_existing_identity="$(
                          "${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_existing_dir"
                        )"
                        case "$artifact_existing_name" in
                          tauri)
                            artifact_stale_canonical_identity=$artifact_existing_identity
                            ;;
                          .tauri-previous)
                            artifact_backup_identity=$artifact_existing_identity
                            ;;
                          .tauri-publish)
                            artifact_publish_identity=$artifact_existing_identity
                            ;;
                        esac
                      fi
                    done
                    unset \
                      artifact_existing_dir \
                      artifact_existing_expected \
                      artifact_existing_identity \
                      artifact_existing_name
                    if [ -e "$artifact_legacy_lock" ] || [ -L "$artifact_legacy_lock" ]; then
                      echo "tauri-build refuses legacy persistent lock residue: $artifact_parent/.tauri-build.lock" >&2
                      exit 2
                    fi
                    if [ -e "$artifact_publish_dir" ] || [ -L "$artifact_publish_dir" ]; then
                      remove_saved_artifact_directory \
                        "stale publication staging artifact" \
                        "$artifact_publish_dir" \
                        "$artifact_publish_identity"
                      artifact_publish_identity=
                    fi
                    if [ -d "$artifact_dir" ]; then
                      if [ -d "$artifact_backup_dir" ]; then
                        remove_saved_artifact_directory \
                          "retained previous artifact" \
                          "$artifact_backup_dir" \
                          "$artifact_backup_identity"
                        artifact_backup_identity=
                      fi
                      if ! artifact_parent_identity_is_current; then
                        echo "tauri-build artifact parent changed before stale artifact hiding" >&2
                        exit 2
                      fi
                      "${pkgs.coreutils}/bin/mv" -T -- \
                        "$artifact_dir" "$artifact_backup_dir"
                      if [ -e "$artifact_dir" ] || [ -L "$artifact_dir" ] \
                        || [ -L "$artifact_backup_dir" ] || [ ! -d "$artifact_backup_dir" ] \
                        || [ "$("${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_backup_dir")" \
                          != "$artifact_stale_canonical_identity" ]; then
                        echo "tauri-build did not atomically hide the owned stale artifact" >&2
                        exit 2
                      fi
                      artifact_backup_identity=$artifact_stale_canonical_identity
                      artifact_stale_canonical_identity=
                    fi
                    if ! artifact_parent_identity_is_current \
                      || [ -e "$artifact_dir" ] || [ -L "$artifact_dir" ] \
                      || [ -e "$artifact_publish_dir" ] || [ -L "$artifact_publish_dir" ]; then
                      echo "tauri-build failed to establish a clean artifact preflight boundary" >&2
                      exit 2
                    fi
                    ${setupWorkdir}
                    release_workdir="$gate_home/pokecon-release-workdir"
                    rm -rf -- "$release_workdir"
                    mv "$workdir" "$release_workdir"
                    workdir="$release_workdir"
                    cd "$workdir"
                    test -f "${productionRoutingAudit}/passed"
                    export SOURCE_DATE_EPOCH=0
                    release_python="${linuxReleaseRuntime}/python"
                    release_wheelhouse="${linuxReleaseRuntime}/wheelhouse"
                    ${resetTauriCargoTarget}
                    ${desktopEnvironment}
                    export PYO3_PYTHON="$release_python/bin/python3.14"
                    export CFLAGS="-ffile-prefix-map=$workdir=/build/pokecon -ffile-prefix-map=$release_python=/build/python -ffile-prefix-map=$CARGO_TARGET_DIR=/build/target''${CFLAGS:+ $CFLAGS}"
                    export CXXFLAGS="-ffile-prefix-map=$workdir=/build/pokecon -ffile-prefix-map=$release_python=/build/python -ffile-prefix-map=$CARGO_TARGET_DIR=/build/target''${CXXFLAGS:+ $CXXFLAGS}"
                    export POKECON_RUST_REMAP_SOURCE="${controlledCargoSource}"
                    export POKECON_RUST_REMAP_PYTHON="$release_python"
                    export POKECON_RUST_REMAP_TARGET="$CARGO_TARGET_DIR"
                    export POKECON_BUILD_UV_PATH="${portableUvBinary}"
                    export POKECON_BUILD_UV_VERSION="${portableUvVersion}"
                    unset POKECON_BUILD_PYTHON
                    unset POKECON_RESOURCE_PROVENANCE
                    ${prepareTauriCargoInvocation}
                    (
                      cd "${cargoInvocationRoot}"
                      ${assertNoCargoConfigAncestors}
                      "${rustToolchain}/bin/cargo" build \
                        --manifest-path "$cargo_source_root/Cargo.toml" \
                        --locked \
                        --release \
                        --package pokecon-worker \
                        --bin pokecon-worker
                    )
                    normalized_bin="$workdir/normalized-bin"
                    worker="$CARGO_TARGET_DIR/release/pokecon-worker"
                    normalized_worker="$normalized_bin/pokecon-worker"
                    mkdir -p "$normalized_bin"
                    cp -p "$worker" "$normalized_worker"
                    "${pythonEnv}/bin/python" -I "${source}/scripts/release/normalize_linux_elf.py" \
                      --worker "$normalized_worker" \
                      --python-root "$release_python" \
                      --patchelf "${pkgs.patchelf}/bin/patchelf" \
                      --strip "${pkgs.binutils}/bin/strip" \
                      --objdump "${pkgs.binutils}/bin/objdump" \
                      --ephemeral-build-root "$gate_home"
                    bundle_root="$workdir/bundle-resources"
                    bundle_config="$workdir/tauri.bundle.json"
                    if ! stage_report_json="$(
                      "${pythonEnv}/bin/python" -I "${source}/scripts/release/stage.py" \
                        --web "${webPackage}" \
                        --worker "$normalized_worker" \
                        --uv "${portableUvBinary}" \
                        --wheelhouse "$release_wheelhouse" \
                        --python "$release_python" \
                        --output "$bundle_root" \
                        --config-output "$bundle_config"
                    )"; then
                      echo "release resource staging failed" >&2
                      exit 2
                    fi
                    if ! packaged_resource_digest="$(
                      "${pythonEnv}/bin/python" -I -S -c '
                    import json
                    import re
                    import sys

                    def reject_duplicate_keys(pairs):
                        report = {}
                        for key, value in pairs:
                            if key in report:
                                raise ValueError("duplicate release stage report key")
                            report[key] = value
                        return report

                    try:
                        report = json.loads(
                            sys.argv[1], object_pairs_hook=reject_duplicate_keys
                        )
                    except (json.JSONDecodeError, ValueError) as error:
                        raise SystemExit(
                            "release stage report is not valid strict JSON"
                        ) from error
                    if type(report) is not dict:
                        raise SystemExit(
                            "release stage report must be exactly one JSON object"
                        )
                    if set(report) != {"content_sha256", "file_count", "platform"}:
                        raise SystemExit("release stage report has unexpected keys")
                    platform = report["platform"]
                    if type(platform) is not str or platform != "unix":
                        raise SystemExit(
                            "release stage report platform must be exactly unix"
                        )
                    content_sha256 = report["content_sha256"]
                    if (
                        type(content_sha256) is not str
                        or re.fullmatch(r"[0-9a-f]{64}", content_sha256) is None
                    ):
                        raise SystemExit(
                            "release stage report content_sha256 must be exactly 64 lowercase hexadecimal characters"
                        )
                    file_count = report["file_count"]
                    if type(file_count) is not int or file_count <= 0:
                        raise SystemExit(
                            "release stage report file_count must be a positive integer"
                        )
                    print(content_sha256)
                    ' "$stage_report_json"
                    )"; then
                      echo "release stage report validation failed" >&2
                      exit 2
                    fi
                    ${prepareTauriCargoInvocation}
                    (
                      cd "${cargoInvocationRoot}"
                      ${assertNoCargoConfigAncestors}
                      export POKECON_RESOURCE_PROVENANCE="packaged:$packaged_resource_digest"
                      "${rustToolchain}/bin/cargo" build \
                        --manifest-path "$cargo_source_root/Cargo.toml" \
                        --locked \
                        --release \
                        --package pokecon \
                        --bin pokecon
                    )
                    application="$CARGO_TARGET_DIR/release/pokecon"
                    normalized_application="$normalized_bin/pokecon"
                    application_backup="$normalized_bin/pokecon.raw"
                    cp -p "$application" "$application_backup"
                    cp -p "$application" "$normalized_application"
                    "${pythonEnv}/bin/python" -I "${source}/scripts/release/normalize_linux_elf.py" \
                      --application "$normalized_application" \
                      --patchelf "${pkgs.patchelf}/bin/patchelf" \
                      --strip "${pkgs.binutils}/bin/strip" \
                      --objdump "${pkgs.binutils}/bin/objdump" \
                      --ephemeral-build-root "$gate_home"
                    if ! bundle_config_json="$(
                      "${pythonEnv}/bin/python" -I -S -c '
                    import json
                    import os
                    import sys
                    from pathlib import Path

                    config_path = Path(sys.argv[1])
                    resource_root = Path(sys.argv[2]).resolve(strict=True)
                    frontend_root = Path(sys.argv[3]).resolve(strict=True)
                    if config_path.is_symlink() or not config_path.is_file():
                        raise SystemExit("generated Tauri bundle config is not a real regular file")
                    if not frontend_root.is_dir() or not (frontend_root / "index.html").is_file():
                        raise SystemExit("canonical Tauri frontend distribution is incomplete")
                    expected_generated = {
                        "bundle": {
                            "resources": {f"{resource_root}{os.sep}": ""},
                        },
                    }
                    with config_path.open(encoding="utf-8") as stream:
                        actual = json.load(stream)
                    if actual != expected_generated:
                        raise SystemExit(
                            "generated Tauri bundle config contains unexpected settings"
                        )
                    expected = {
                        "build": {"frontendDist": str(frontend_root)},
                        **expected_generated,
                    }
                    print(json.dumps(expected, sort_keys=True, separators=(",", ":")))
                    ' "$bundle_config" "$bundle_root" "${webPackage}"
                    )"; then
                      echo "generated Tauri bundle config validation failed" >&2
                      exit 2
                    fi
                    restore_release_application() {
                      cp -p "$application_backup" "$application"
                    }
                    cleanup_tauri_build() {
                      gate_status=$?
                      trap - EXIT
                      tauri_cleanup_status=0
                      if ! restore_release_application; then
                        echo "failed to restore the release application during cleanup" >&2
                        tauri_cleanup_status=1
                      fi
                      if [ -n "$workdir" ] \
                        && ! "${pkgs.coreutils}/bin/rm" -rf -- "$workdir"; then
                        echo "failed to remove the Tauri build worktree: $workdir" >&2
                        tauri_cleanup_status=1
                      fi
                      if [ -n "$gate_home" ] \
                        && ! "${pkgs.coreutils}/bin/rm" -rf -- "$gate_home"; then
                        echo "failed to remove the Tauri build gate home: $gate_home" >&2
                        tauri_cleanup_status=1
                      fi
                      if [ "$gate_status" -eq 0 ] && [ "$tauri_cleanup_status" -ne 0 ]; then
                        gate_status=$tauri_cleanup_status
                      fi
                      if [ "$gate_status" -eq 0 ] \
                        && ! artifact_parent_identity_is_current; then
                        echo "tauri-build artifact parent changed before successful cleanup finalization" >&2
                        gate_status=1
                      fi
                      if [ "$gate_status" -ne 0 ]; then
                        pokecon_cleanup_task_artifacts || tauri_cleanup_status=1
                      else
                        artifact_replaced=0
                        artifact_committed_identity=
                      fi
                      exit "$gate_status"
                    }
                    trap cleanup_tauri_build EXIT
                    cp -p "$normalized_application" "$application"
                    ${prepareTauriCargoInvocation}
                    (
                      export PATH="${rustToolchain}/bin:$PATH"
                      cd "$cargo_source_root/rust/pokecon"
                      "${pkgs.cargo-tauri}/bin/cargo-tauri" tauri bundle \
                        --ci \
                        --config "$bundle_config_json" \
                        "''${bundle_args[@]}"
                    )
                    restore_release_application
                    validate_private_tauri_inventory() {
                      local inventory_label inventory_path inventory_resolved
                      inventory_path=$1
                      inventory_label=$2
                      case "$inventory_path" in
                        "$gate_home"/*) ;;
                        *)
                          echo "$inventory_label escaped the private gate home" >&2
                          return 1
                          ;;
                      esac
                      if ! inventory_resolved="$(
                        "${pkgs.coreutils}/bin/readlink" -f -- "$inventory_path"
                      )"; then
                        echo "$inventory_label cannot be resolved" >&2
                        return 1
                      fi
                      if [ -L "$inventory_path" ] || [ ! -f "$inventory_path" ] \
                        || [ "$inventory_resolved" != "$inventory_path" ] \
                        || [ "$("${pkgs.coreutils}/bin/stat" -c %a -- "$inventory_path")" != 600 ]; then
                        echo "$inventory_label is not a private real inventory file" >&2
                        return 1
                      fi
                      return 0
                    }
                    remove_private_tauri_inventory() {
                      local inventory_label inventory_path
                      inventory_path=$1
                      inventory_label=$2
                      if ! validate_private_tauri_inventory "$inventory_path" "$inventory_label"; then
                        return 1
                      fi
                      if ! "${pkgs.coreutils}/bin/rm" -f -- "$inventory_path"; then
                        echo "failed to remove $inventory_label" >&2
                        return 1
                      fi
                      if [ -e "$inventory_path" ] || [ -L "$inventory_path" ]; then
                        echo "$inventory_label remains after removal" >&2
                        return 1
                      fi
                      return 0
                    }

                    bundle_deb_inventory=
                    if ! bundle_deb_inventory="$(
                      "${pkgs.coreutils}/bin/mktemp" \
                        --tmpdir="$gate_home" pokecon-tauri-bundle-debs.XXXXXXXX.nul
                    )"; then
                      echo "failed to create the private Tauri bundle inventory" >&2
                      exit 2
                    fi
                    validate_private_tauri_inventory \
                      "$bundle_deb_inventory" "Tauri bundle inventory"
                    if ! "${pkgs.findutils}/bin/find" -P \
                      "$CARGO_TARGET_DIR/release/bundle" \
                      -name '*.deb' -print0 > "$bundle_deb_inventory"; then
                      echo "tauri-build failed to inventory Debian bundle outputs" >&2
                      exit 2
                    fi
                    bundle_deb_entries=()
                    while IFS= read -r -d "" package; do
                      bundle_deb_entries+=("$package")
                    done < "$bundle_deb_inventory"
                    remove_private_tauri_inventory \
                      "$bundle_deb_inventory" "Tauri bundle inventory"
                    bundle_deb_inventory=
                    if [ "''${#bundle_deb_entries[@]}" -ne 1 ]; then
                      echo "tauri-build expected exactly one Debian package, found ''${#bundle_deb_entries[@]}" >&2
                      exit 2
                    fi
                    package="''${bundle_deb_entries[0]}"
                    if [ -L "$package" ] || [ ! -f "$package" ]; then
                      echo "tauri-build Debian package is not a real regular file: $package" >&2
                      exit 2
                    fi
                    "${pythonEnv}/bin/python" -I "${source}/scripts/release/normalize_debian_package.py" \
                      --dpkg-deb "${pkgs.dpkg}/bin/dpkg-deb" \
                      "$package"
                    if ! artifact_parent_identity_is_current \
                      || [ -e "$artifact_dir" ] || [ -L "$artifact_dir" ] \
                      || [ -e "$artifact_publish_dir" ] || [ -L "$artifact_publish_dir" ] \
                      || [ -e "$artifact_legacy_lock" ] || [ -L "$artifact_legacy_lock" ]; then
                      echo "tauri-build artifact publication boundary changed during the build" >&2
                      exit 2
                    fi
                    "${pkgs.coreutils}/bin/mkdir" -- "$artifact_publish_dir"
                    if [ -L "$artifact_publish_dir" ] || [ ! -d "$artifact_publish_dir" ] \
                      || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$artifact_publish_dir")" \
                        != "$artifact_publish_dir_expected" ]; then
                      echo "tauri-build fixed artifact publication directory is invalid" >&2
                      exit 2
                    fi
                    artifact_publish_identity="$(
                      "${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_publish_dir"
                    )"
                    package_name="''${package##*/}"
                    "${pkgs.coreutils}/bin/cp" -p -- \
                      "$package" "$artifact_publish_dir/$package_name"
                    artifact_publish_inventory=
                    if ! artifact_publish_inventory="$(
                      "${pkgs.coreutils}/bin/mktemp" \
                        --tmpdir="$gate_home" pokecon-tauri-publish.XXXXXXXX.nul
                    )"; then
                      echo "failed to create the private Tauri publication inventory" >&2
                      exit 2
                    fi
                    validate_private_tauri_inventory \
                      "$artifact_publish_inventory" "Tauri publication inventory"
                    if ! "${pkgs.findutils}/bin/find" -P "$artifact_publish_dir" \
                      -mindepth 1 -print0 > "$artifact_publish_inventory"; then
                      echo "tauri-build failed to inventory fixed publication staging" >&2
                      exit 2
                    fi
                    artifact_publish_entries=()
                    while IFS= read -r -d "" published_entry; do
                      artifact_publish_entries+=("$published_entry")
                    done < "$artifact_publish_inventory"
                    remove_private_tauri_inventory \
                      "$artifact_publish_inventory" "Tauri publication inventory"
                    artifact_publish_inventory=
                    if [ "''${#artifact_publish_entries[@]}" -ne 1 ] \
                      || [ "''${artifact_publish_entries[0]}" != "$artifact_publish_dir/$package_name" ] \
                      || [ -L "''${artifact_publish_entries[0]}" ] \
                      || [ ! -f "''${artifact_publish_entries[0]}" ] \
                      || ! "${pkgs.diffutils}/bin/cmp" -s -- \
                        "$package" "''${artifact_publish_entries[0]}"; then
                      echo "tauri-build artifact publication inventory is not exact" >&2
                      exit 2
                    fi
                    if ! artifact_parent_identity_is_current \
                      || [ -e "$artifact_dir" ] || [ -L "$artifact_dir" ] \
                      || [ -L "$artifact_publish_dir" ] || [ ! -d "$artifact_publish_dir" ] \
                      || [ "$("${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_publish_dir")" \
                        != "$artifact_publish_identity" ]; then
                      echo "tauri-build fixed publication staging changed before commit" >&2
                      exit 2
                    fi
                    artifact_committed_identity=$artifact_publish_identity
                    artifact_replaced=1
                    "${pkgs.coreutils}/bin/mv" -T -- \
                      "$artifact_publish_dir" "$artifact_dir"
                    if [ -e "$artifact_publish_dir" ] || [ -L "$artifact_publish_dir" ] \
                      || [ -L "$artifact_dir" ] || [ ! -d "$artifact_dir" ] \
                      || [ "$("${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_dir")" \
                        != "$artifact_committed_identity" ]; then
                      echo "tauri-build committed artifact identity differs from owned staging" >&2
                      exit 2
                    fi
                    artifact_publish_identity=
                    artifact_published_inventory=
                    if ! artifact_published_inventory="$(
                      "${pkgs.coreutils}/bin/mktemp" \
                        --tmpdir="$gate_home" pokecon-tauri-published.XXXXXXXX.nul
                    )"; then
                      echo "failed to create the private Tauri published inventory" >&2
                      exit 2
                    fi
                    validate_private_tauri_inventory \
                      "$artifact_published_inventory" "Tauri published inventory"
                    if ! "${pkgs.findutils}/bin/find" -P "$artifact_dir" \
                      -mindepth 1 -print0 > "$artifact_published_inventory"; then
                      echo "tauri-build failed to inventory the committed canonical artifact" >&2
                      exit 2
                    fi
                    artifact_published_entries=()
                    while IFS= read -r -d "" published_entry; do
                      artifact_published_entries+=("$published_entry")
                    done < "$artifact_published_inventory"
                    remove_private_tauri_inventory \
                      "$artifact_published_inventory" "Tauri published inventory"
                    artifact_published_inventory=
                    if [ -L "$artifact_dir" ] || [ ! -d "$artifact_dir" ] \
                      || [ "$("${pkgs.coreutils}/bin/readlink" -f -- "$artifact_dir")" \
                        != "$artifact_dir_expected" ] \
                      || [ "$("${pkgs.coreutils}/bin/stat" -Lc '%d:%i' -- "$artifact_dir")" \
                        != "$artifact_committed_identity" ] \
                      || [ "''${#artifact_published_entries[@]}" -ne 1 ] \
                      || [ "''${artifact_published_entries[0]}" != "$artifact_dir/$package_name" ] \
                      || [ -L "''${artifact_published_entries[0]}" ] \
                      || [ ! -f "''${artifact_published_entries[0]}" ] \
                      || ! "${pkgs.diffutils}/bin/cmp" -s -- \
                        "$package" "''${artifact_published_entries[0]}"; then
                      echo "tauri-build published artifact inventory is not canonical" >&2
                      exit 2
                    fi
                    if ! artifact_parent_identity_is_current; then
                      echo "tauri-build artifact parent changed before success cleanup" >&2
                      exit 2
                    fi
                    if [ -e "$artifact_backup_dir" ] || [ -L "$artifact_backup_dir" ]; then
                      remove_saved_artifact_directory \
                        "retained previous artifact" \
                        "$artifact_backup_dir" \
                        "$artifact_backup_identity"
                      artifact_backup_identity=
                    fi
                    if ! artifact_parent_identity_is_current \
                      || [ -e "$artifact_backup_dir" ] || [ -L "$artifact_backup_dir" ] \
                      || [ -e "$artifact_publish_dir" ] || [ -L "$artifact_publish_dir" ] \
                      || [ -e "$artifact_legacy_lock" ] || [ -L "$artifact_legacy_lock" ]; then
                      echo "tauri-build success left transient artifact state in dist" >&2
                      exit 2
                    fi
                  '';
                };

            package-smoke = mkTask {
              name = "package-smoke";
              runtimeInputs = [
                pythonEnv
                pkgs.binutils
                pkgs.dpkg
                pkgs.patchelf
                linuxReleaseRuntimeLibraries
              ];
              text = ''
                ${setupSourceGateEnvironment}
                export PYTHONPATH="${source}"
                unset LD_LIBRARY_PATH
                python -m scripts.release.package_smoke \
                  --dpkg-deb "${pkgs.dpkg}/bin/dpkg-deb" \
                  --patchelf "${pkgs.patchelf}/bin/patchelf" \
                  --objdump "${pkgs.binutils}/bin/objdump" \
                  --runtime-library-path "${linuxReleaseRuntimeLibraries}/lib" \
                  "$@"
              '';
            };

            package-reproducibility-check = mkTask {
              name = "package-reproducibility-check";
              runtimeInputs = [
                pkgs.diffutils
                pkgs.findutils
              ];
              text = ''
                if [ "$#" -ne 2 ]; then
                  echo "usage: nix run .#package-reproducibility-check -- PRIMARY_DIR REPRODUCTION_DIR" >&2
                  exit 2
                fi
                ${setupSourceGateEnvironment}
                primary_root=$1
                reproduction_root=$2
                for bundle_root in "$primary_root" "$reproduction_root"; do
                  if [ -L "$bundle_root" ] || [ ! -d "$bundle_root" ]; then
                    echo "package reproducibility input must be a real directory: $bundle_root" >&2
                    exit 2
                  fi
                done
                mapfile -d "" -t primary_bundles < <(
                  find -P "$primary_root" -type f -name '*.deb' -print0
                )
                mapfile -d "" -t reproduction_bundles < <(
                  find -P "$reproduction_root" -type f -name '*.deb' -print0
                )
                if [ "''${#primary_bundles[@]}" -ne 1 ] \
                  || [ "''${#reproduction_bundles[@]}" -ne 1 ]; then
                  echo "expected exactly one primary and one reproduction Debian bundle" >&2
                  exit 1
                fi
                sha256sum -- "''${primary_bundles[0]}" "''${reproduction_bundles[0]}"
                cmp -- "''${primary_bundles[0]}" "''${reproduction_bundles[0]}"
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
                POKECON_RESOURCE_PROVENANCE=development cargo tauri build --debug --no-bundle --ci -- --locked
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
                POKECON_RESOURCE_PROVENANCE=development scripts/quality/generate-api-types.sh "$@"
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
                pkgs.jq
                pkgs.markdownlint-cli
                pythonPackageBuildUv
                pkgs.ripgrep
                pkgs.shellcheck
                pkgs.textlint
                pkgs.textlint-rule-no-start-duplicated-conjunction
                pkgs.typos
              ];
              text = ''
                if [ "''${1:-}" = "--help" ]; then
                  echo "Run aggregate source verification gates; packaged CLI and UI use dedicated apps"
                  exit 0
                fi
                ${setupWorkdir}
                ${desktopEnvironment}
                export NODE_PATH="${pkgs.textlint-rule-no-start-duplicated-conjunction}/lib/node_modules"
                export PYTHONDONTWRITEBYTECODE=1
                export PYTHONPATH="$PWD/python:$PWD"
                export POKECON_TEST_UV="${pythonPackageBuildUv}/bin/uv"
                export CARGO_PROFILE_DEV_DEBUG=line-tables-only
                export CARGO_PROFILE_TEST_DEBUG=line-tables-only
                ${lib.optionalString pkgs.stdenv.isLinux "export RUSTFLAGS='-C link-arg=-Wl,--threads=1'"}
                python -m scripts.quality.source_filter
                actionlint .github/workflows/*.yml
                python -m scripts.release.gate
                POKECON_RESOURCE_PROVENANCE=development cargo run --locked --package pokecon --bin generate_contracts --features contract-generator -- --check
                check-jsonschema --check-metaschema generated/settings.schema.json
                python -m scripts.acceptance.records
                export POKECON_API_NODE_MODULES="${apiBunDependencies}/node_modules"
                POKECON_RESOURCE_PROVENANCE=development scripts/quality/generate-api-types.sh --check
                cp -R "${webBunDependencies}/node_modules" web/
                chmod -R u+w web/node_modules
                bun run --cwd web --bun lint
                bun run --cwd web --bun svelte-check
                bun run --cwd web --bun test
                bun run --cwd web --bun build
                POKECON_RESOURCE_PROVENANCE=development cargo test --locked --workspace --all-features
                POKECON_RESOURCE_PROVENANCE=development \
                cargo build --locked --workspace --all-features --jobs 1
                POKECON_RESOURCE_PROVENANCE=development cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
                python -m scripts.compatibility.promote --check
                python -m scripts.compatibility.runner \
                  --check \
                  --compatibility-binary "$CARGO_TARGET_DIR/debug/pokecon-compatibility" \
                  --worker "$CARGO_TARGET_DIR/debug/pokecon-worker" \
                  --site-packages "${pythonEnv}/${pkgs.python314.sitePackages}"
                bun --bun "${basedpyrightCli}"
                "${pythonEnv}/bin/python" -I \
                  "${source}/scripts/quality/run_parallel_checks.py" \
                  pytest \
                  python -m pytest \
                  -p no:cacheprovider \
                  -m "not production_routing_mutation" \
                  tests \
                  -v \
                  --tb=short \
                  --next \
                  production-routing-mutation-audit \
                  "${productionRoutingMutationAuditRunner}/bin/pokecon-production-routing-mutation-audit"
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
