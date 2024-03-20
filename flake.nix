{
  description = "Build a cargo project";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-analyzer-src.follows = "";
    };

    flake-utils.url = "github:numtide/flake-utils";

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, crane, fenix, flake-utils, advisory-db, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        inherit (pkgs) lib;
        craneLib = crane.lib.${system};

        composeFilters = filters: path: type: lib.any (filter: filter path type) filters;
        src = lib.cleanSourceWith {
          src = craneLib.path ./.;
          filter = composeFilters [
            craneLib.filterCargoSources
            (path: _: builtins.match ".*stpl$" path != null)
          ];
        };

        commonArgs = rec {
          inherit src;
          strictDeps = true;

          nativeBuildInputs = with pkgs; [
            pkg-config

            libxkbcommon
            wlroots
            wayland
            waylandpp
            wayland-utils
            wayland-scanner
            wayland-protocols
            xwayland
            egl-wayland
          ] ++ (with pkgs.xorg; [
            /*
            libX11
            libXcursor
            libXrandr
            libXi
            */
          ]);

          buildInputs = with pkgs; [
          ] ++ nativeBuildInputs ++ lib.optionals pkgs.stdenv.isDarwin [
            libiconv
          ];

          LD_LIBRARY_PATH = "${pkgs.wayland}/lib:${pkgs.libxkbcommon}/lib";
        };

        craneLibLLvmTools = craneLib.overrideToolchain
          (fenix.packages.${system}.complete.withComponents [
            "cargo"
            "llvm-tools"
            "rustc"
          ]);

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        axolotl = craneLib.buildPackage
          (commonArgs // {
            inherit cargoArtifacts;
          });
      in
      {
        checks = {
          # Build the crate as part of `nix flake check` for convenience
          inherit axolotl;

          # Run clippy (and deny all warnings) on the crate source,
          # again, reusing the dependency artifacts from above.
          #
          # Note that this is done as a separate derivation so that
          # we can block the CI if there are issues here, but not
          # prevent downstream consumers from building our crate by itself.
          axolotl-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });

          axolotl-doc = craneLib.cargoDoc (commonArgs // {
            inherit cargoArtifacts;
          });

          # Check formatting
          axolotl-fmt = craneLib.cargoFmt {
            inherit src;
          };

          # Audit dependencies
          axolotl-audit = craneLib.cargoAudit {
            inherit src advisory-db;
          };

          # Audit licenses
          axolotl-deny = craneLib.cargoDeny {
            inherit src;
          };

          # Run tests with cargo-nextest
          # Consider setting `doCheck = false` on `axolotl` if you do not want
          # the tests to run twice
          axolotl-nextest = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
          });
        };

        packages = {
          default = axolotl;
          inherit axolotl;
        } // lib.optionalAttrs (!pkgs.stdenv.isDarwin) {
          axolotl-llvm-coverage = craneLibLLvmTools.cargoLlvmCov (commonArgs // {
            inherit cargoArtifacts;
          });
        };

        apps.default = flake-utils.lib.mkApp {
          drv = axolotl;
        };

        devShells.default = craneLib.devShell {
          # Inherit inputs from checks.
          checks = self.checks.${system};

          # Additional dev-shell environment variables can be set directly
          # MY_CUSTOM_DEVELOPMENT_VAR = "something else";
          LD_LIBRARY_PATH = "${pkgs.wayland}/lib:${pkgs.libxkbcommon}/lib";

          # Extra inputs can be added here; cargo and rustc are provided by default.
          packages = [
            # pkgs.ripgrep
          ];
        };
      });
}
