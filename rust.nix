# The Rust workspace: the crate2nix build of every crate, and the
# derivations that run cargo on a crate directly. flake.nix calls this with
# callPackage once per system.
{
  lib,
  pkgs,
  pkgsStatic,
  stdenv,
  rustPlatform,
  runCommand,
  writeShellApplication,
  git,
  prettier,
  wasm-bindgen-cli,
}:
let
  # Pin rustc from rust-toolchain.toml so nix and rustup builds match.
  toolchainFor = p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

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
        ${lib.optionalString docTests ''
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

  cargoToml = fromTOML (builtins.readFile ./Cargo.toml);

  # cargo loads every workspace member's manifest and rejects one that
  # discovers no target, so a `src` holding only the crates a derivation
  # builds hands the members it left out an empty stand-in lib.
  stubMembers = lib.concatMapStrings (member: ''
    mkdir -p ${member}/src
    : > ${member}/src/lib.rs
  '');
in
rec {
  inherit (cargoToml.workspace.package) version;
  toolchain = toolchainFor pkgs;

  # Cargo.nix is generated — don't hand-edit (regen: docs/generated.md). Build
  # with our pinned rustc, not nixpkgs'; the virtual workspace has no
  # rootCrate, so callers select a member via `workspaceMembers."<name>"`.
  # Our own crates (local `src`, not a crates.io `sha256`) compile with
  # clippy-driver under Cargo.toml's `[workspace.lints]`; deps stay plain.
  # On Linux every crate links statically against musl, as the release
  # binary does, so the checks test what ships. Darwin has no
  # pkgsStatic and builds natively.
  cargoNix =
    let
      cargoPkgs = if stdenv.hostPlatform.isLinux then pkgsStatic else pkgs;
      rustToolchain = toolchainFor cargoPkgs.buildPackages;
      lints = cargoToml.workspace.lints;
    in
    cargoPkgs.callPackage ./Cargo.nix {
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
            # The nit crate's source is its own directory only, so the
            # plugin that it compiles in comes by path (crates/nit/build.rs).
            defaultCrateOverrides = p.defaultCrateOverrides // {
              nit = _: { NIT_PLUGIN_DIR = ./plugins/nit; };
            };
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
            postBuild = rustdocPostBuild (lib.elem "lib" (crate.type or [ "lib" ]));
          });
    };

  # The web's wire types, generated from nit-types: a native `cargo test`
  # (the `ts`-feature exporter) writes every web-facing type's ts-rs
  # declaration into one module, prettier-formatted like any source file. A
  # pinned, offline derivation so `gen-types` (writes it) and `types-drift`
  # (diffs it) share one source of truth. `TS_RS_LARGE_INT=number` maps
  # u64/i64 to the wire's `number`, not bigint.
  wireTypes = stdenv.mkDerivation {
    name = "nit-wire-types.gen.ts";
    src = lib.fileset.toSource {
      root = ./.;
      fileset = lib.fileset.unions [
        ./Cargo.toml
        ./Cargo.lock
        ./crates/nit-types
        ./crates/nit/Cargo.toml
        ./crates/nit-contract/Cargo.toml
        ./crates/nit-wasm/Cargo.toml
      ];
    };
    nativeBuildInputs = [
      toolchain
      prettier
      rustPlatform.cargoSetupHook
    ];
    cargoDeps = rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
    buildPhase =
      stubMembers [
        "crates/nit"
        "crates/nit-contract"
        "crates/nit-wasm"
      ]
      + ''
        TS_RS_LARGE_INT=number TYPES_GEN_OUT="$PWD/types.gen.ts" \
          cargo test --offline --features ts -p nit-types \
          -- --exact export::write_wire_types
        prettier --write types.gen.ts
      '';
    installPhase = "mv types.gen.ts $out";
    dontFixup = true;
  };

  # The public contract: the OpenAPI document the routes' utoipa
  # annotations produce and the CLI's clap syntax, written by
  # `nit-contract` into one directory.
  contract = runCommand "nit-contract" { } ''
    ${cargoNix.workspaceMembers."nit-contract".build}/bin/nit-contract $out
  '';

  # The shared change fold compiled to WebAssembly: nit-wasm built for
  # wasm32, then run through wasm-bindgen (`--target web`) into the JS
  # glue + `.wasm` the web imports. Offline and pinned like wireTypes; the
  # `rust-toolchain.toml` wasm32 target and the wasm-bindgen-cli version
  # (which must match nit-wasm's `wasm-bindgen` dep) come from this flake.
  wasm = stdenv.mkDerivation {
    name = "nit-wasm-pkg";
    # Only nit-wasm and the nit-types it depends on: the server crate's
    # sources are excluded so a server change leaves this derivation —
    # and every web check that injects it — untouched.
    src = lib.fileset.toSource {
      root = ./.;
      fileset = lib.fileset.unions [
        ./Cargo.toml
        ./Cargo.lock
        ./rust-toolchain.toml
        ./crates/nit-types
        ./crates/nit-wasm
        ./crates/nit/Cargo.toml
        ./crates/nit-contract/Cargo.toml
      ];
    };
    nativeBuildInputs = [
      # With rust-src in the sysroot, rustc writes its store path into
      # std's panic locations, and the server binary embeds this wasm.
      (toolchain.override { extensions = [ ]; })
      wasm-bindgen-cli
      rustPlatform.cargoSetupHook
    ];
    cargoDeps = rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
    buildPhase =
      stubMembers [
        "crates/nit"
        "crates/nit-contract"
      ]
      + ''
        cargo build --offline --release --target wasm32-unknown-unknown -p nit-wasm
        wasm-bindgen target/wasm32-unknown-unknown/release/nit_wasm.wasm \
          --target web --out-dir pkg
      '';
    installPhase = "cp -r pkg $out";
    dontFixup = true;
  };

  # Writes the WebAssembly fold into web/src/wasm for local dev. One
  # derivation shared two ways: `nix run .#gen-wasm` (no devShell needed)
  # and, added to the devShell below, a bare `gen-wasm` on PATH inside
  # `nix develop` — so generating it doesn't require `nix run`. Finds the
  # tree via git so it also works from a worktree or the web/ subdir
  # (`npm run` scripts shell out to it with that cwd).
  genWasm = writeShellApplication {
    name = "gen-wasm";
    runtimeInputs = [ git ];
    text = ''
      cd "$(git rev-parse --show-toplevel)"
      rm -rf web/src/wasm
      install -d web/src/wasm
      install -m644 ${wasm}/* web/src/wasm/
      echo "wrote web/src/wasm/"
    '';
  };
}
