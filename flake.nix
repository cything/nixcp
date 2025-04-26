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
        nativeBuildInputs = with pkgs; [
          pkg-config
        ];
        buildInputs = with pkgs; [
          toolchain
          openssl
          nix
          boost
        ];
        env = {
          # for cpp bindings to work
          NIX_INCLUDE_PATH = "${lib.getDev pkgs.nix}/include";
        };
      in
      {
        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          inherit nativeBuildInputs;
          packages = with pkgs; [
            tokio-console
            cargo-udeps
          ];
          env = env // {
            RUST_LOG = "nixcp=debug";
            RUST_BACKGRACE = 1;
          };
        };

        packages.default = craneLib.buildPackage {
          inherit nativeBuildInputs;
          inherit buildInputs;
          inherit env;
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
        };
      }
    );
}
