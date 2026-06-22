{
  description = "nit — commit-level code review for AI coding agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      crane,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );
      # The pinned toolchain (rust-toolchain.toml) drives both the devShell
      # and the build, so contributors and CI compile with the same rustc.
      rustToolchainFor = pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      # crane wired to that toolchain: the shared source filter, the
      # deps-only artifact cache, and the args every build and check reuse.
      craneScopeFor =
        pkgs:
        let
          rustToolchain = rustToolchainFor pkgs;
          # crane builds its internal `crane-utils` helper with nixpkgs'
          # default Rust, pulling a second full toolchain (plus LLVM) into the
          # build closure beside our pinned one. Point it at the pinned
          # toolchain and skip the SBOM (auditable) build it does not need.
          craneUtilsRustPlatform =
            let
              base = pkgs.makeRustPlatform {
                cargo = rustToolchain;
                rustc = rustToolchain;
              };
            in
            base
            // {
              buildRustPackage = args: base.buildRustPackage (args // { auditable = false; });
            };
          craneLib = ((crane.mkLib pkgs).overrideToolchain rustToolchain).overrideScope (
            _final: prev: {
              craneUtils = prev.craneUtils.override { rustPlatform = craneUtilsRustPlatform; };
            }
          );
          commonArgs = {
            # The workspace root Cargo.toml has no [package], so crane cannot
            # infer a name from it; set it here for every crane derivation
            # (deps, build, clippy, test) instead of the placeholder.
            pname = "nit";
            version = "0.1.0";
            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = craneLib.fileset.commonCargoSources ./.;
            };
            strictDeps = true;
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [
              pkgs.libgit2
              pkgs.sqlite
              pkgs.zlib
            ];
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in
        {
          inherit craneLib commonArgs cargoArtifacts;
        };
      # Shared by the web build (nit-web) and its lint check (web-lint):
      # one source tree and one npm dependency closure (npmDepsHash), two
      # scripts. System-independent, so it lives outside forAllSystems.
      webArgs = {
        version = "0.1.0";
        src = ./web;
        npmDepsHash = "sha256-7NKAoi4RpVq50ZjjeMTk/3//FA4qNNiQRt4zTKG4vrI=";
      };
    in
    {
      devShells = forAllSystems (
        pkgs:
        let
          rustToolchain = rustToolchainFor pkgs;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              # Rust backend — rustc/cargo/clippy/rustfmt/rust-analyzer all
              # come from the one pinned toolchain.
              rustToolchain
              pkg-config
              libgit2
              sqlite
              zlib

              # Web frontend
              nodejs_22

              # Formatting — treefmt drives the per-language formatters
              # configured in treefmt.toml (rustfmt from the toolchain above)
              treefmt
              nixfmt
              prettier
              shfmt
              taplo

              # Screenshot harness / frontend checking
              playwright-driver
            ];

            env = {
              PLAYWRIGHT_BROWSERS_PATH = pkgs.playwright-driver.browsers;
              PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
              # Pin for package.json: npm playwright must match the driver.
              PLAYWRIGHT_DRIVER_VERSION = pkgs.playwright-driver.version;
            };
          };
        }
      );

      packages = forAllSystems (
        pkgs:
        let
          inherit (craneScopeFor pkgs) craneLib commonArgs cargoArtifacts;
        in
        rec {
          nit-web = pkgs.buildNpmPackage {
            pname = "nit-web";
            inherit (webArgs) version src npmDepsHash;
            installPhase = "cp -r dist $out";
          };

          nit-unwrapped = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              # Tests run as a discrete `nix flake check` validator (the `test`
              # check below), not as part of building the product.
              doCheck = false;
            }
          );

          # The real product: nit with the built web UI baked in via env.
          nit =
            pkgs.runCommand "nit"
              {
                nativeBuildInputs = [ pkgs.makeWrapper ];
              }
              ''
                mkdir -p $out/bin
                makeWrapper ${nit-unwrapped}/bin/nit $out/bin/nit \
                  --set-default NIT_WEB_DIST ${nit-web}
              '';

          default = nit;
        }
      );

      checks = forAllSystems (
        pkgs:
        let
          inherit (craneScopeFor pkgs) craneLib commonArgs cargoArtifacts;
        in
        {
          build = self.packages.${pkgs.system}.nit;
          # Clippy as a crane validator, mirroring the devShell lint command
          # (cargo clippy --all-targets -- -D warnings). The workspace sets
          # clippy::pedantic = warn; -D warnings turns any lint into a failure.
          clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- -D warnings";
            }
          );
          # The test suite as a discrete validator. It builds real repos and
          # runs `git rebase` in the differential test, so it needs git and a
          # committer identity the build sandbox otherwise lacks.
          test = craneLib.cargoTest (
            commonArgs
            // {
              inherit cargoArtifacts;
              nativeCheckInputs = [ pkgs.git ];
              preCheck = ''
                export HOME=$TMPDIR
                export GIT_AUTHOR_NAME=nix GIT_AUTHOR_EMAIL=nix@build
                export GIT_COMMITTER_NAME=nix GIT_COMMITTER_EMAIL=nix@build
              '';
            }
          );
          # The frontend lint (eslint + stylelint + knip) as a validator, the
          # web counterpart to clippy — it mirrors the devShell `npm run lint`.
          # Shares nit-web's source and npm dependency closure (webArgs) but
          # runs the lint in place of the build, so a stylelint, eslint, or
          # knip regression fails `nix flake check` instead of slipping in.
          # The lint report is the derivation's output.
          web-lint = pkgs.buildNpmPackage {
            pname = "nit-web-lint";
            inherit (webArgs) version src npmDepsHash;
            dontNpmBuild = true;
            installPhase = "npm run lint > $out";
          };
        }
      );

      # `nix fmt` = the same whole-tree treefmt the devShell runs,
      # self-contained (formatters on PATH without entering the shell).
      formatter = forAllSystems (
        pkgs:
        pkgs.writeShellApplication {
          name = "treefmt";
          runtimeInputs = with pkgs; [
            treefmt
            rustfmt
            nixfmt
            prettier
            shfmt
            taplo
          ];
          text = ''exec treefmt "$@"'';
        }
      );
    };
}
