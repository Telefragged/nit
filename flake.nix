{
  description = "nit — commit-level code review for AI coding agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
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
      # Pin rustc from rust-toolchain.toml so nix and rustup builds match.
      rustToolchainFor = pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      # Cargo.nix is generated — don't hand-edit (regen: docs/dev.md). Build
      # with our pinned rustc, not nixpkgs'; the virtual workspace has no
      # rootCrate, so callers select a member via `workspaceMembers."<name>"`.
      # Our own crates (local `src`, not a crates.io `sha256`) compile with
      # clippy-driver under Cargo.toml's `[workspace.lints]`; deps stay plain.
      cargoNixFor =
        pkgs:
        let
          rustToolchain = rustToolchainFor pkgs;
          lints = (fromTOML (builtins.readFile ./Cargo.toml)).workspace.lints;
        in
        pkgs.callPackage ./Cargo.nix {
          buildRustCrateForPkgs =
            p:
            let
              base = p.buildRustCrate.override {
                rustc = rustToolchain;
                cargo = rustToolchain;
                clippy = rustToolchain;
                # buildRustCrate defaults to codegen-units=1 (serial codegen),
                # ~3x slower per crate than cargo's release default. Match cargo.
                defaultCodegenUnits = 16;
              };
            in
            crate:
            let
              drv = base crate;
            in
            if crate ? sha256 then
              drv
            else
              drv.override (_: {
                useClippy = true;
                inherit lints;
              });
        };

      # Source tree and version shared by the web build (nit-web) and its
      # lint/test checks. System-independent, so it lives outside
      # forAllSystems.
      webArgs = {
        version = "0.1.0";
        src = ./web;
      };

      # The web npm closure, shared via `npmDeps` so the nit-web build and
      # the web-lint / web-test checks don't each refetch it.
      webNpmDepsFor =
        pkgs:
        pkgs.fetchNpmDeps {
          inherit (webArgs) src;
          name = "nit-web-npm-deps";
          hash = "sha256-7NKAoi4RpVq50ZjjeMTk/3//FA4qNNiQRt4zTKG4vrI=";
        };

      # Build metadata for `nit --version`: `+<sha>[.dirty]` from the flake's
      # git state, which the build sandbox can't reach itself (no `.git`).
      # `rev` is set only on a clean tree, `dirtyRev` only on a dirty one;
      # neither on a revless tarball, leaving a bare semver.
      gitSuffix =
        if self ? rev then
          "+${builtins.substring 0 12 self.rev}"
        else if self ? dirtyRev then
          "+${builtins.substring 0 12 self.dirtyRev}.dirty"
        else
          "";
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

              # Regenerates Cargo.nix
              crate2nix

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
          cargoNix = cargoNixFor pkgs;
          webNpmDeps = webNpmDepsFor pkgs;
        in
        rec {
          nit-web = pkgs.buildNpmPackage {
            pname = "nit-web";
            inherit (webArgs) version src;
            npmDeps = webNpmDeps;
            installPhase = "cp -r dist $out";
          };

          # Build only; tests live in the `test` check (the build/verify split).
          # The git suffix rides in as an env var the crate's build.rs reads.
          nit-unwrapped = cargoNix.workspaceMembers."nit".build.overrideAttrs (_: {
            NIT_GIT_SUFFIX = gitSuffix;
          });

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
          cargoNix = cargoNixFor pkgs;
          webNpmDeps = webNpmDepsFor pkgs;
        in
        {
          build = self.packages.${pkgs.system}.nit;
          # Check that all files are formatted (same treefmt as `nix fmt`).
          treefmt = pkgs.stdenvNoCC.mkDerivation {
            name = "treefmt-check";
            src = self;
            nativeBuildInputs = [ self.formatter.${pkgs.system} ];
            buildPhase = "HOME=$TMPDIR treefmt --ci --tree-root .";
            installPhase = "touch $out";
          };
          # Also clippy-checks the test targets (lib/bins are linted in every
          # build). Tests run here, not in `nix build`; the differential test
          # shells out to `git rebase`, so it needs git and a committer identity
          # the sandbox lacks.
          test = cargoNix.workspaceMembers."nit".build.override {
            runTests = true;
            testInputs = [ pkgs.git ];
            testPreRun = ''
              export HOME=$TMPDIR
              export GIT_AUTHOR_NAME=nix GIT_AUTHOR_EMAIL=nix@build
              export GIT_COMMITTER_NAME=nix GIT_COMMITTER_EMAIL=nix@build
            '';
          };
          # nit-types is shared with a future web build, so it must stay
          # wasm-friendly: build and round-trip-test it with NO optional
          # features, the clap-off config the server's `features = ["clap"]`
          # would otherwise mask.
          test-nit-types = cargoNix.workspaceMembers."nit-types".build.override {
            runTests = true;
            features = [ ];
          };
          # The frontend lint (eslint + stylelint + knip) as a validator, the
          # web counterpart to clippy — it mirrors the devShell `npm run lint`.
          # Shares nit-web's source and npm dependency closure (webArgs) but
          # runs the lint in place of the build, so a stylelint, eslint, or
          # knip regression fails `nix flake check` instead of slipping in.
          # The lint report is the derivation's output.
          web-lint = pkgs.buildNpmPackage {
            pname = "nit-web-lint";
            inherit (webArgs) version src;
            npmDeps = webNpmDeps;
            dontNpmBuild = true;
            installPhase = "npm run lint > $out";
          };
          web-test = pkgs.buildNpmPackage {
            pname = "nit-web-test";
            inherit (webArgs) version src;
            npmDeps = webNpmDeps;
            dontNpmBuild = true;
            installPhase = "npm run test > $out";
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
