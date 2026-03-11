{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "nixpkgs/nixos-unstable";
  };

  outputs =
    {
      self,
      flake-utils,
      fenix,
      nixpkgs,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            fenix.overlays.default
            (import ./overlay.nix)
          ];
        };
      in
      {
        formatter = pkgs.nixfmt-tree;

        packages = {
          default = self.packages.${system}.labaclaw;
          inherit (pkgs)
            labaclaw
            zeroclaw
            ;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ pkgs.labaclaw ];
          packages = [
            pkgs.rust-analyzer
          ];
        };
      }
    )
    // {
      overlays.default = import ./overlay.nix;
    };
}
