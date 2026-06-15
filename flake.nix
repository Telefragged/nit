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
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchainFor;
          commonArgs = {
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
            version = "0.1.0";
            src = ./web;
            npmDepsHash = "sha256-DUUz79xX9cTDY/DV7eSfSTJ04YV565pS9/Cc4Zbevh0=";
            installPhase = ''
              runHook preInstall
              cp -r dist $out
              runHook postInstall
            '';
          };

          nit-unwrapped = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              # The test suite builds real repos and runs `git rebase` in the
              # differential test; the sandbox has no git or identity config.
              nativeCheckInputs = [ pkgs.git ];
              preCheck = ''
                export HOME=$TMPDIR
                export GIT_AUTHOR_NAME=nix GIT_AUTHOR_EMAIL=nix@build
                export GIT_COMMITTER_NAME=nix GIT_COMMITTER_EMAIL=nix@build
              '';
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
