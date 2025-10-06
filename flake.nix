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
            sha256 = "sha256-VZZnlyP69+Y3crrLHQyJirqlHrTtGTsyiSnZB8jEvVo=";
          }
        );
      in
      {
        devShell =
          with pkgs;
          mkShell {
            nativeBuildInputs = [ pkgs.pkg-config ];
            packages = [
              rustToolchain
              rust-analyzer-nightly

              clang

              nixfmt-rfc-style

              python3

              cargo-flamegraph
            ];
            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            LIBCLANG_PATH = "${libclang.lib}/lib";
            BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${llvmPackages.libclang.lib}/lib/clang/${lib.getVersion clang}/include";
          };
      }
    );
}
