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

          devShells.default = pkgs.mkShell {
            name = "pokecon-devshell";

            packages = with pkgs; [
              rustToolchain
              cargo-expand
              cargo-flamegraph
              rust-analyzer
              clippy
              rustfmt

              python314
              python314Packages.pip
              python314Packages.pytest
              python314Packages.ruff

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
            '';
          };
        };
    };
}
