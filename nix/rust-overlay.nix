final: prev: {
  rust-bin = prev.rust-bin.overrideScope' (self: super: {
    fromRustupToolchainFile = path:
      super.fromRustupToolchainFile path;
  });
}
