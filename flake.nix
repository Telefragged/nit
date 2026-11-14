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
        "aarch64-darwin"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [
                rust-overlay.overlays.default
                # Build the npm-deps prefetcher with the pinned toolchain, so
                # the build closure contains one rustc. cargo-auditable is
                # built with the stock rustc, so auditable is off.
                (final: prev: {
                  prefetch-npm-deps = prev.prefetch-npm-deps.override {
                    rustPlatform =
                      let
                        tc = final.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
                        base = final.makeRustPlatform {
                          cargo = tc;
                          rustc = tc;
                        };
                      in
                      base
                      // {
                        buildRustPackage = args: base.buildRustPackage (args // { auditable = false; });
                      };
                  };
                })
              ];
            }
          )
        );
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

      rustFor = pkgs: pkgs.callPackage ./rust.nix { };

      webFor =
        pkgs:
        pkgs.callPackage ./web {
          inherit (rustFor pkgs) version wasm;
        };
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            (rustFor pkgs).toolchain
            pkg-config
            sqlite
            zlib

            # `cargo nextest run` applies the per-test time limit of the
            # flake checks (.config/nextest.toml).
            cargo-nextest
            # Regenerates Cargo.nix
            crate2nix
            # Regenerates crates/workspace-hack (docs/generated.md).
            cargo-hakari

            # `gen-wasm`, on PATH here, writes web/src/wasm (docs/generated.md).
            # wasm-bindgen-cli's version is pinned to nit-wasm's
            # wasm-bindgen dep.
            wasm-bindgen-cli
            (rustFor pkgs).genWasm

            # Web frontend. Every npm derivation below takes this same
            # `pkgs.nodejs`, so the shell and the build share an npm and
            # read each other's lock file.
            nodejs

            # Formatting — treefmt drives the per-language formatters
            # configured in treefmt.toml (rustfmt from the toolchain above)
            treefmt
            nixfmt
            prettier
            shfmt
            taplo

            playwright-driver
          ];

          env = {
            PLAYWRIGHT_BROWSERS_PATH = (webFor pkgs).playwrightBrowsers;
            PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
            # Pin for package.json: npm playwright must match the driver.
            PLAYWRIGHT_DRIVER_VERSION = pkgs.playwright-driver.version;
          };
        };
      });

      packages = forAllSystems (
        pkgs:
        let
          nit-web = (webFor pkgs).bundle;
        in
        rec {
          inherit nit-web;
          # Build only; tests live in the `test` check. The crate's build.rs
          # uses the git suffix, and it compiles in the web UI at NIT_WEB_DIST.
          nit = (rustFor pkgs).cargoNix.workspaceMembers."nit".build.overrideAttrs (_: {
            NIT_GIT_SUFFIX = gitSuffix;
            NIT_WEB_DIST = nit-web;
          });
          default = nit;
          # The release drops the `+<sha>` suffix, so `nit --version` tells
          # a published release from a build of a git checkout.
          nit-release = nit.overrideAttrs { NIT_GIT_SUFFIX = ""; };
          contract = (rustFor pkgs).contract;
          # The plugin directory at the store path's root, the shape that
          # home-manager's `programs.claude-code.plugins` accepts.
          claude-plugin = pkgs.runCommandLocal "nit-claude-plugin" { } "cp -r ${./plugins/nit} $out";
        }
      );

      checks = forAllSystems (
        pkgs:
        let
          rust = rustFor pkgs;
        in
        {
          # The release build carries no `+<sha>` suffix, so a commit that
          # leaves the source unchanged reuses the cached build.
          build = self.packages.${pkgs.stdenv.hostPlatform.system}.nit-release;
          # Compile the WebAssembly fold — surfaces a wasm32 build break or a
          # wasm-bindgen version skew independently of the web build.
          wasm-build = rust.wasm;
          # `wasm-build` compiles nit-wasm without clippy and runs no tests.
          # Built here for the host target to put it under clippy like every
          # other crate and to run its unit tests; its lib has no
          # `cfg(target_arch)` branch, so the host build covers the same code
          # wasm32 compiles.
          test-nit-wasm = rust.nextest "nit-wasm";
          treefmt = pkgs.stdenvNoCC.mkDerivation {
            name = "treefmt-check";
            src = self;
            nativeBuildInputs = [ self.formatter.${pkgs.stdenv.hostPlatform.system} ];
            buildPhase = "HOME=$TMPDIR treefmt --ci --tree-root .";
            installPhase = "touch $out";
          };
          # Also clippy-checks the test targets (lib/bins are linted in every
          # build). Rustdoc runs in the same build (rustdocPostBuild in
          # rust.nix): the docs render and the doc tests run.
          test = rust.nextest "nit";
          # Build and round-trip-test nit-types with NO optional features —
          # the no-feature baseline an optional feature (the server's
          # `features = ["clap"]`, the web's `features = ["ts"]`) would mask.
          test-nit-types = rust.nextest "nit-types";
          contract = rust.contract;
          types-drift = pkgs.runCommand "types-drift-check" { } ''
            if ! diff -u ${./web/src/api/types.gen.ts} ${rust.wireTypes}; then
              echo "web/src/api/types.gen.ts is stale — run: nix run .#gen-types" >&2
              exit 1
            fi
            touch $out
          '';
          # crate2nix resolves features once per root crate, so without the
          # workspace-hack crate, a dependency that two roots use with
          # different features compiles twice.
          hakari = pkgs.stdenvNoCC.mkDerivation {
            name = "hakari-check";
            src = rust.workspaceSrc;
            nativeBuildInputs = [
              rust.toolchain
              pkgs.cargo-hakari
              pkgs.rustPlatform.cargoSetupHook
            ];
            cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
            buildPhase = ''
              cargo hakari generate --diff
              cargo hakari manage-deps --dry-run
            '';
            installPhase = "touch $out";
          };
        }
        // (webFor pkgs).checks
      );

      # `nix run .#gen-types` regenerates the web's wire types into the tree.
      apps = forAllSystems (pkgs: {
        gen-types = {
          type = "app";
          program = "${
            pkgs.writeShellApplication {
              name = "gen-types";
              text = ''
                if [ ! -e Cargo.toml ] || [ ! -d crates/nit-types ]; then
                  echo "run from the repo root" >&2
                  exit 1
                fi
                install -m644 ${(rustFor pkgs).wireTypes} web/src/api/types.gen.ts
                echo "wrote web/src/api/types.gen.ts"
              '';
            }
          }/bin/gen-types";
        };

        gen-wasm = {
          type = "app";
          program = "${(rustFor pkgs).genWasm}/bin/gen-wasm";
        };

        release-check = {
          type = "app";
          program = "${
            pkgs.writeShellApplication {
              name = "release-check";
              runtimeInputs = [
                pkgs.git
                pkgs.taplo
              ];
              text = builtins.readFile ./release/check.sh;
            }
          }/bin/release-check";
        };
        bump-oasdiff = {
          type = "app";
          program = "${
            pkgs.writeShellApplication {
              name = "bump-oasdiff";
              runtimeInputs = [
                pkgs.curl
                pkgs.git
                pkgs.jq
              ];
              text = builtins.readFile ./release/bump-oasdiff.sh;
            }
          }/bin/bump-oasdiff";
        };
      });

      homeManagerModules.default = import ./home-manager.nix self;

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
