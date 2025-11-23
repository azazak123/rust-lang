{
  inputs = {
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      utils,
      fenix,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ fenix.overlays.default ];
        };
        rustToolchain = (
          fenix.packages.${system}.fromToolchainFile {
            dir = ./.;
            sha256 = "sha256-SJwZ8g0zF2WrKDVmHrVG3pD2RGoQeo24MEXnNx5FyuI=";
          }
        );
      in
      {
        devShell =
          with pkgs;
          mkShell {
            LD_LIBRARY_PATH = "${pkgs.stdenv.cc.cc.lib}/lib";
            nativeBuildInputs = [ pkgs.pkg-config ];
            packages = [
              rustToolchain
              rust-analyzer-nightly

              clang
              cmake

              nixfmt-rfc-style

              python3
              python313Packages.matplotlib
              # python312Packages.numpy

              cargo-flamegraph
              hotspot
            ];
            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            LIBCLANG_PATH = "${libclang.lib}/lib";
            BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${llvmPackages.libclang.lib}/lib/clang/${lib.getVersion clang}/include";
          };
      }
    );
}
