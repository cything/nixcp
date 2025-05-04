{
  description = "nixcp flake";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs@{ nixpkgs, flake-utils, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            (import inputs.rust-overlay)
          ];
        };
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain(_: toolchain);
        lib = pkgs.lib;

        # don't clean cpp files
        cppFilter = path: _type: builtins.match ".*(cpp|hpp)$" path != null;
        cppOrCargo = path: type:
          (cppFilter path type) || (craneLib.filterCargoSources path type);
        src = lib.cleanSourceWith {
          src = ./.;
          filter = cppOrCargo;
          name = "source";
        };

        commonArgs = {
          inherit src;
          strictDeps = true;
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
          buildInputs = with pkgs; [
            toolchain
            openssl
            nix
            boost
          ];
          # for cpp bindings to work
          NIX_INCLUDE_PATH = "${lib.getDev pkgs.nix}/include";
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        nixcp = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });
      in
      {
        devShells.default = craneLib.devShell {
          inputsFrom = [ nixcp ];

          RUST_BACKGRACE = 1;
          # for cpp bindings to work
          NIX_INCLUDE_PATH = "${lib.getDev pkgs.nix}/include";

          packages = with pkgs; [
            tokio-console
            cargo-udeps
          ];
        };

        packages.default = nixcp;
      }
    );
}
