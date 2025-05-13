{
  description = "Build a cargo workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane";

    flake-utils.url = "github:numtide/flake-utils";

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    rust-overlay,
    flake-utils,
    advisory-db,
    ...
  }: let
    name = "weaver";
  in
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(import rust-overlay)];
      };
      inherit (pkgs) lib;

      rustToolchainFor = p:
        p.rust-bin.stable.latest.default.override {
          # Set the build targets supported by the toolchain,
          # wasm32-unknown-unknown is required for trunk.
          targets = ["wasm32-unknown-unknown"];
          extensions = ["llvm-tools"];
        };
      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchainFor;
      # When filtering sources, we want to allow assets other than .rs files
      unfilteredRoot = ./.; # The original, unfiltered source
      src = lib.fileset.toSource {
        root = unfilteredRoot;
        fileset = lib.fileset.unions [
          # Default files from crane (Rust and cargo files)
          (craneLib.fileset.commonCargoSources unfilteredRoot)
          (
            lib.fileset.fileFilter
            (file: lib.any file.hasExt ["html" "scss"])
            unfilteredRoot
          )
          # Example of a folder for images, icons, etc
          (lib.fileset.maybeMissing ./assets)
        ];
      };

      # Common arguments can be set here to avoid repeating them later
      commonArgs = {
        inherit src;
        strictDeps = true;

        buildInputs =
          with pkgs; [
            # Add additional build inputs here
            sqlite
            pkg-config
            openssl
          ]
          ++ lib.optionals pkgs.stdenv.isDarwin [
            # Additional darwin specific inputs can be set here
            pkgs.libiconv
          ];
        nativeBuildInputs = with pkgs; [
          sqlite
          pkg-config
          openssl
        ];
        # Additional environment variables can be set directly
        # MY_CUSTOM_VAR = "some value";
      };

      # Wasm packages

      # it's not possible to build the server on the
      # wasm32 target, so we only build the client.
      wasmArgs =
        commonArgs
        // {
          pname = "trunk-workspace-wasm";
          cargoExtraArgs = "--package=client";
          CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
        };

      cargoArtifactsWasm = craneLib.buildDepsOnly (wasmArgs
        // {
          doCheck = false;
        });

      # craneLibLLvmTools = craneLib.overrideToolchain
      #   (fenix.packages.${system}.complete.withComponents [
      #     "cargo"
      #     "llvm-tools"
      #     "rustc"
      #   ]);

      # Build *just* the cargo dependencies (of the entire workspace),
      # so we can reuse all of that work (e.g. via cachix) when running in CI
      # It is *highly* recommended to use something like cargo-hakari to avoid
      # cache misses when building individual top-level-crates
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      individualCrateArgs =
        commonArgs
        // {
          inherit cargoArtifacts;
          inherit (craneLib.crateNameFromCargoToml {inherit src;}) version;
          # NB: we disable tests since we'll run them all via cargo-nextest
          doCheck = false;
        };

      fileSetForCrate = crate:
        lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./Cargo.toml
            ./Cargo.lock
            (craneLib.fileset.commonCargoSources ./crates/weaver-common)
            (craneLib.fileset.commonCargoSources ./crates/weaver-workspace-hack)
            (craneLib.fileset.commonCargoSources crate)
          ];
        };

      # Build the top-level crates of the workspace as individual derivations.
      # This allows consumers to only depend on (and build) only what they need.
      # Though it is possible to build the entire workspace as a single derivation,
      # so this is left up to you on how to organize things
      #
      # Note that the cargo workspace must define `workspace.members` using wildcards,
      # otherwise, omitting a crate (like we do below) will result in errors since
      # cargo won't be able to find the sources for all members.
      weaver-cli = craneLib.buildPackage (individualCrateArgs
        // {
          pname = "${name}-cli";
          cargoExtraArgs = "-p ${name}-cli";
          src = fileSetForCrate ./crates/weaver-cli;
        });
      weaver-server = craneLib.buildPackage (individualCrateArgs
        // {
          pname = "${name}-server";
          cargoExtraArgs = "-p ${name}-server";
          src = fileSetForCrate ./crates/weaver-server;
        });
      weaver-renderer = craneLib.buildPackage (individualCrateArgs
        // {
          pname = "${name}-renderer";
          cargoExtraArgs = "-p ${name}-renderer";
          src = fileSetForCrate ./crates/weaver-renderer;
        });
    in {
      checks = {
        # Build the crates as part of `nix flake check` for convenience
        inherit weaver-cli weaver-server weaver-renderer;

        # Run clippy (and deny all warnings) on the workspace source,
        # again, reusing the dependency artifacts from above.
        #
        # Note that this is done as a separate derivation so that
        # we can block the CI if there are issues here, but not
        # prevent downstream consumers from building our crate by itself.
        "${name}-workspace-clippy" = craneLib.cargoClippy (commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });

        "${name}-workspace-doc" = craneLib.cargoDoc (commonArgs
          // {
            inherit cargoArtifacts;
          });

        # Check formatting
        "${name}-workspace-fmt" = craneLib.cargoFmt {
          inherit src;
        };

        # "${name}-workspace-toml-fmt" = craneLib.taploFmt {
        #   src = pkgs.lib.sources.sourceFilesBySuffices src [".toml"];
        #   # taplo arguments can be further customized below as needed
        #   taploExtraArgs = "--config ./taplo.toml";
        # };

        # Audit dependencies
        "${name}-workspace-audit" = craneLib.cargoAudit {
          inherit src advisory-db;
        };

        # Audit licenses
        "${name}-workspace-deny" = craneLib.cargoDeny {
          inherit src;
        };

        # Run tests with cargo-nextest
        # Consider setting `doCheck = false` on other crate derivations
        # if you do not want the tests to run twice
        "${name}-workspace-nextest" = craneLib.cargoNextest (commonArgs
          // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
            cargoNextestPartitionsExtraArgs = "--no-tests=pass";
          });

        # Ensure that cargo-hakari is up to date
        "${name}-workspace-hakari" = craneLib.mkCargoDerivation {
          inherit src;
          pname = "${name}-workspace-hakari";
          cargoArtifacts = null;
          doInstallCargoArtifacts = false;

          buildPhaseCargoCommand = ''
            cargo hakari generate --diff  # workspace-hack Cargo.toml is up-to-date
            cargo hakari manage-deps --dry-run  # all workspace crates depend on workspace-hack
            cargo hakari verify
          '';

          nativeBuildInputs = with pkgs; [
            cargo-hakari
            sqlite
            pkg-config
            openssl
          ];
        };
      };

      packages =
        {
          inherit weaver-cli weaver-server;
        }
        // lib.optionalAttrs (!pkgs.stdenv.isDarwin) {
          # weaver-workspace-llvm-coverage = craneLibLLvmTools.cargoLlvmCov (commonArgs // {
          #   inherit cargoArtifacts;
          # });
        };

      apps = {
        weaver-cli = flake-utils.lib.mkApp {
          drv = weaver-cli;
        };
        weaver-server = flake-utils.lib.mkApp {
          drv = weaver-server;
        };
      };

      devShells.default = craneLib.devShell {
        inherit name;
        # Inherit inputs from checks.
        checks = self.checks.${system};
        NIX_LD_LIBRARY_PATH = with pkgs;
          lib.makeLibraryPath [
            stdenv.cc.cc
            openssl
            # ...
          ];
        NIX_LD = lib.fileContents "${pkgs.stdenv.cc}/nix-support/dynamic-linker";

        LD_LIBRARY_PATH = "$LD_LIBRARY_PATH:$NIX_LD_LIBRARY_PATH";

        # Additional dev-shell environment variables can be set directly
        # MY_CUSTOM_DEVELOPMENT_VAR = "something else";

        # Extra inputs can be added here; cargo and rustc are provided by default.
        packages = with pkgs; [
          cargo-hakari
          nixd
          alejandra
        ];
      };
    });
}
