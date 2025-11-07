{
  description = "Build a cargo workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };

    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
    dioxus.url = "github:DioxusLabs/dioxus";
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    rust-overlay,
    flake-utils,
    advisory-db,
    dioxus,
    ...
  }: let
    name = "weaver";
  in
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          (import rust-overlay)
          (_: prev: {
            dioxus-cli = dioxus.packages.${prev.system}.dioxus-cli;
          })
        ];
      };
      inherit (pkgs) lib;

      rustToolchainFor = p:
        p.rust-bin.selectLatestNightlyWith (toolchain:
          toolchain.default.override {
            # Set the build targets supported by the toolchain,
            # wasm32-unknown-unknown is required for trunk.
            targets = ["wasm32-unknown-unknown" "wasm32-wasip1" "wasm32-wasip2"];
            extensions = [
              "rust-src"
              "rust-analyzer"
              "clippy"
            ];
          });
      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchainFor;
      src = craneLib.cleanCargoSource ./.;

      # Common arguments can be set here to avoid repeating them later
      commonArgs = {
        inherit src;
        strictDeps = true;

        buildInputs = with pkgs;
          [
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
            (lib.fileset.maybeMissing ./crates/weaver-app/Dioxus.lock)
            (craneLib.fileset.commonCargoSources ./crates/weaver-common)
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
          pname = "${name}";
          cargoExtraArgs = "-p ${name}-cli";
          src = fileSetForCrate ./crates/weaver-cli;
        });
      weaver-app = craneLib.buildPackage (individualCrateArgs
        // {
          pname = "${name}-app";
          cargoExtraArgs = "-p ${name}-app";
          src = fileSetForCrate ./crates/weaver-app;
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
        inherit weaver-cli weaver-app weaver-renderer;

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
      };

      packages =
        {
          inherit weaver-cli weaver-app;
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
        weaver-app = flake-utils.lib.mkApp {
          drv = weaver-app;
        };
      };

      devShells.default = let
        # dioxus-cli = pkgs.dioxus-cli.overrideAttrs (_: {
        #   postPatch = ''
        #     rm Cargo.lock
        #     cp ${./crates/weaver-app/Dioxus.lock} Cargo.lock
        #   '';
        #   cargoDeps = pkgs.rustPlatform.importCargoLock {
        #     lockFile = ./crates/weaver-app/Dioxus.lock;
        #   };
        # });
        cargoLock = builtins.fromTOML (builtins.readFile ./Cargo.lock);

        wasmBindgen =
          pkgs.lib.findFirst
          (pkg: pkg.name == "wasm-bindgen")
          (throw "Could not find wasm-bindgen package")
          cargoLock.package;

        wasm-bindgen-cli = pkgs.buildWasmBindgenCli rec {
          src = pkgs.fetchCrate {
            pname = "wasm-bindgen-cli";
            version = wasmBindgen.version;
            hash = "sha256-zLPFFgnqAWq5R2KkaTGAYqVQswfBEYm9x3OPjx8DJRY=";
          };

          cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
            inherit src;
            inherit (src) pname version;
            hash = "sha256-a2X9bzwnMWNt0fTf30qAiJ4noal/ET1jEtf5fBFj5OU=";
          };
        };
      in
        craneLib.devShell {
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
            nixd
            alejandra
            diesel-cli
            postgresql
            cargo-insta
            jq
            dioxus-cli
            wasm-bindgen-cli
            wasm-pack
            binaryen.out
          ];
        };
    });
}
