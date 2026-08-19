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
              overlays = [
                rust-overlay.overlays.default
                # Build the npm-deps prefetcher with our pinned toolchain, not a
                # second stock rustc. auditable off: cargo-auditable is its last
                # edge back.
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
      # Pin rustc from rust-toolchain.toml so nix and rustup builds match.
      rustToolchainFor = pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      # Rustdoc, run from inside the crate's own build. `runTests` covers
      # every test kind but the doc examples, and no build runs rustdoc at
      # all: buildRustCrate drives rustc directly, so nothing extracts the
      # examples from doc-comments or resolves the intra-doc links between
      # them. But the flags rustdoc wants are the ones the lib build just
      # used, and build-crate.nix leaves them in the shell it hands to
      # `postBuild` — so re-run the compiler it already configured, one flag
      # different. Gated on `buildTests` to ride the test build, whose
      # dependency set includes the dev-dependencies cargo also lends to doc
      # examples. `docTests` is false for a cdylib, which has no rlib for an
      # example to link against — cargo skips its doc tests for the same
      # reason, but still documents it.
      rustdocPostBuild = docTests: ''
        # Every other variable read here would fail the rustdoc calls loudly
        # if it went away; an unset $buildTests would just skip them forever,
        # so make that case an error instead.
        : "''${buildTests?buildRustCrate no longer sets buildTests}"
        if [ -n "$buildTests" ]; then
          docLib="$LIB_PATH"
          [ -e "$docLib" ] || docLib=src/lib.rs
          if [ -e "$docLib" ]; then
            docOpts=""
            for opt in $LIB_RUSTC_OPTS; do
              case "$opt" in
                --remap-path-prefix=*) ;;
                *) docOpts="$docOpts $opt" ;;
              esac
            done
            # Rendering the docs is what resolves every intra-doc link, so a
            # broken or private one is a rustdoc lint here — denied by the
            # `[workspace.lints]` flags already in $LIB_RUSTC_OPTS. The
            # rendered tree is the by-product; the exit code is the point.
            # No $EXTRA_LIB: a crate is not an extern of its own docs.
            rustdoc "$docLib" \
              --crate-name "$CRATE_NAME" \
              --out-dir target/doc \
              --cap-lints "$CAP_LINTS" \
              --color always \
              -L dependency=target/deps \
              $docOpts $LINK $BUILD_OUT_DIR $EXTRA_FEATURES
            ${nixpkgs.lib.optionalString docTests ''
              echo "Doc-testing $CRATE_NAME ($docLib)"
              # $EXTRA_LIB is the --extern for this crate's own rlib, built
              # by build_lib before the --test pass; the rest is verbatim
              # what build_lib passes rustc.
              rustdoc --test "$docLib" \
                --crate-name "$CRATE_NAME" \
                --cap-lints "$CAP_LINTS" \
                --color always \
                -L dependency=target/deps \
                $docOpts $EXTRA_LIB $LINK $BUILD_OUT_DIR $EXTRA_FEATURES
            ''}
          fi
        fi
      '';

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
                postBuild = rustdocPostBuild (nixpkgs.lib.elem "lib" (crate.type or [ "lib" ]));
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
          hash = "sha256-ye7jtF2Ggn3aptOK4Mggen7X220Ovge4wLIkwFLgX/k=";
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

      # The web's wire types, generated from nit-types: a native `cargo test`
      # (the `ts`-feature exporter) writes every web-facing type's ts-rs
      # declaration into one module, prettier-formatted like any source file. A
      # pinned, offline derivation so `gen-types` (writes it) and `types-drift`
      # (diffs it) share one source of truth. `TS_RS_LARGE_INT=number` maps
      # u64/i64 to the wire's `number`, not bigint.
      wireTypesTs =
        pkgs:
        pkgs.stdenv.mkDerivation {
          name = "nit-wire-types.gen.ts";
          src = nixpkgs.lib.fileset.toSource {
            root = ./.;
            fileset = nixpkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./crates
            ];
          };
          nativeBuildInputs = [
            (rustToolchainFor pkgs)
            pkgs.prettier
            pkgs.rustPlatform.cargoSetupHook
          ];
          cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
          buildPhase = ''
            TS_RS_LARGE_INT=number TYPES_GEN_OUT="$PWD/types.gen.ts" \
              cargo test --offline --features ts -p nit-types \
              -- --exact export::write_wire_types
            prettier --write types.gen.ts
          '';
          installPhase = "mv types.gen.ts $out";
          dontFixup = true;
        };

      # The shared change fold compiled to WebAssembly: nit-wasm built for
      # wasm32, then run through wasm-bindgen (`--target bundler`) into the JS
      # glue + `.wasm` the web imports. Offline and pinned like wireTypesTs; the
      # `rust-toolchain.toml` wasm32 target and the wasm-bindgen-cli version
      # (which must match nit-wasm's `wasm-bindgen` dep) come from this flake.
      wasmPkg =
        pkgs:
        pkgs.stdenv.mkDerivation {
          name = "nit-wasm-pkg";
          # Only nit-wasm and the nit-types it depends on: the server crate's
          # sources are excluded so a server change leaves this derivation —
          # and every web check that injects it — untouched.
          src = nixpkgs.lib.fileset.toSource {
            root = ./.;
            fileset = nixpkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./rust-toolchain.toml
              ./crates/nit-types
              ./crates/nit-wasm
              ./crates/nit/Cargo.toml
            ];
          };
          nativeBuildInputs = [
            (rustToolchainFor pkgs)
            pkgs.wasm-bindgen-cli
            pkgs.rustPlatform.cargoSetupHook
          ];
          cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
          buildPhase = ''
            # cargo loads every workspace member's manifest and rejects one
            # that discovers no target, so the sourceless member gets an
            # empty stand-in lib.
            mkdir -p crates/nit/src
            : > crates/nit/src/lib.rs

            cargo build --offline --release --target wasm32-unknown-unknown -p nit-wasm
            wasm-bindgen target/wasm32-unknown-unknown/release/nit_wasm.wasm \
              --target bundler --out-dir pkg
          '';
          installPhase = "cp -r pkg $out";
          dontFixup = true;
        };

      # Drop the generated wasm glue into a web build's tree before it runs.
      # web/src/wasm is gitignored (so absent from the npm-package source), so
      # every web derivation injects it the way `gen-wasm` does for local dev
      # — the artifact is built by nix, never committed.
      injectWasm = pkgs: ''
        mkdir -p src/wasm
        cp ${wasmPkg pkgs}/* src/wasm/
      '';

      # Writes the WebAssembly fold into web/src/wasm for local dev. One
      # derivation shared two ways: `nix run .#gen-wasm` (no devShell needed)
      # and, added to the devShell below, a bare `gen-wasm` on PATH inside
      # `nix develop` — so generating it doesn't require `nix run`. Finds the
      # tree via git so it also works from a worktree or the web/ subdir
      # (`npm run` scripts shell out to it with that cwd).
      genWasmApp =
        pkgs:
        pkgs.writeShellApplication {
          name = "gen-wasm";
          runtimeInputs = [ pkgs.git ];
          text = ''
            cd "$(git rev-parse --show-toplevel)"
            rm -rf web/src/wasm
            install -d web/src/wasm
            install -m644 ${wasmPkg pkgs}/* web/src/wasm/
            echo "wrote web/src/wasm/"
          '';
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

              # Regenerates Cargo.nix
              crate2nix

              # `gen-wasm`, on PATH here, writes web/src/wasm (docs/dev.md).
              # wasm-bindgen-cli's version is pinned to nit-wasm's
              # wasm-bindgen dep.
              wasm-bindgen-cli
              (genWasmApp pkgs)

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
            preBuild = injectWasm pkgs;
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
          # Compile the WebAssembly fold — surfaces a wasm32 build break or a
          # wasm-bindgen version skew independently of the web build.
          wasm-build = wasmPkg pkgs;
          # `wasm-build` runs a bare `cargo build`, so it is the one crate the
          # workspace lints and tests would otherwise miss. Built here through
          # crate2nix (host target) to put it under clippy like every other
          # crate and to run its unit tests; its lib has no `cfg(target_arch)`
          # branch, so the host build covers the same code wasm32 compiles.
          test-nit-wasm = cargoNix.workspaceMembers."nit-wasm".build.override {
            runTests = true;
          };
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
          # the sandbox lacks. Rustdoc rides the same build
          # (rustdocPostBuild): the docs render and the doc tests run.
          test = cargoNix.workspaceMembers."nit".build.override {
            runTests = true;
            testInputs = [ pkgs.gitMinimal ];
            testPreRun = ''
              export HOME=$TMPDIR
              export GIT_AUTHOR_NAME=nix GIT_AUTHOR_EMAIL=nix@build
              export GIT_COMMITTER_NAME=nix GIT_COMMITTER_EMAIL=nix@build
            '';
          };
          # Build and round-trip-test nit-types with NO optional features —
          # the serde-only baseline an optional feature (the server's
          # `features = ["clap"]`, the web's `features = ["ts"]`) would mask.
          test-nit-types = cargoNix.workspaceMembers."nit-types".build.override {
            runTests = true;
            features = [ ];
          };
          # The frontend lint (eslint + stylelint + knip) as a validator, the
          # web counterpart to clippy — it mirrors the devShell `npm run lint`,
          # so a stylelint, eslint, or knip regression fails `nix flake check`
          # instead of slipping in.
          # web-lint/web-test set dontNpmBuild, which disables the build phase
          # (and its preBuild hook), so inject the wasm at the head of the
          # install phase that runs the lint/test instead.
          web-lint = pkgs.buildNpmPackage {
            pname = "nit-web-lint";
            inherit (webArgs) version src;
            npmDeps = webNpmDeps;
            dontNpmBuild = true;
            installPhase = injectWasm pkgs + "npm run lint > $out";
          };
          web-test = pkgs.buildNpmPackage {
            pname = "nit-web-test";
            inherit (webArgs) version src;
            npmDeps = webNpmDeps;
            dontNpmBuild = true;
            installPhase = injectWasm pkgs + "npm run test > $out";
          };
          # The same `capture.mjs` agents run locally, but against the nix
          # `playwright-driver` browsers, so a driver/npm-pin skew fails the
          # check. Its output is the PNGs, not a marker file.
          web-screenshots = pkgs.buildNpmPackage {
            pname = "nit-web-screenshots";
            inherit (webArgs) version src;
            npmDeps = webNpmDeps;
            dontNpmBuild = true;
            env = {
              PLAYWRIGHT_BROWSERS_PATH = pkgs.playwright-driver.browsers;
              PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
              # No user namespaces / writable /dev/shm in the build sandbox.
              NIT_SCREENSHOT_NO_SANDBOX = "1";
              # The system font stacks (tokens.css) resolve to nothing in the
              # sandbox; map the generic sans/mono families to real fonts so
              # the captures render text, not tofu.
              FONTCONFIG_FILE = pkgs.makeFontsConf {
                fontDirectories = [
                  pkgs.liberation_ttf
                  pkgs.dejavu_fonts
                ];
              };
            };
            installPhase = injectWasm pkgs + ''
              export HOME=$TMPDIR
              NIT_SCREENSHOT_OUT_DIR=$out node screenshots/capture.mjs
            '';
          };
          types-drift = pkgs.runCommand "types-drift-check" { } ''
            if ! diff -u ${./web/src/api/types.gen.ts} ${wireTypesTs pkgs}; then
              echo "web/src/api/types.gen.ts is stale — run: nix run .#gen-types" >&2
              exit 1
            fi
            touch $out
          '';
        }
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
                install -m644 ${wireTypesTs pkgs} web/src/api/types.gen.ts
                echo "wrote web/src/api/types.gen.ts"
              '';
            }
          }/bin/gen-types";
        };

        gen-wasm = {
          type = "app";
          program = "${genWasmApp pkgs}/bin/gen-wasm";
        };
      });

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
