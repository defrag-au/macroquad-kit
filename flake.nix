{
  description = "macroquad-kit — macroquad apps, widgets and games for defrag-au";

  inputs = {
    # Reuses `rust-worker-stack` so the toolchain (rustc, cargo, clippy,
    # rustfmt, wasm-bindgen) is in lock-step with shared-crates and the
    # consumers. The kit needs the wasm32 target for every client and the
    # native GL stack for the storybook's headless captures.
    defrag-nix.url = "github:defrag-au/defrag-nix";
  };

  outputs =
    { defrag-nix, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];
      mkShells =
        system: {
          default = defrag-nix.devShells.${system}.rust-worker-stack;
        };
    in
    {
      devShells = builtins.listToAttrs (
        map (system: {
          name = system;
          value = mkShells system;
        }) systems
      );
    };
}
