{
  description = "Poke Controller Modified Extension development environment";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      finalSystem: let
        pkgs = import nixpkgs {
          inherit finalSystem;
          overlays = [
            (import rust-overlay)
            (import ./nix/rust-overlay.nix)
            (import ./nix/tauri.nix)
          ];
        };
      in {
        devShells.default = import ./nix/devshell.nix {inherit pkgs;};
      }
    );
}
