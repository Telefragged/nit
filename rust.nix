# The Rust workspace: the crate2nix build of every crate, and the
# derivations that run cargo on a crate directly. flake.nix calls this with
# callPackage once per system.
{
  lib,
  pkgs,
  pkgsStatic,
  stdenv,
  stdenvNoCC,
  runCommand,
  cargo-nextest,
  jq,
  writeShellApplication,
  git,
  prettier,
  wasm-bindgen-cli,
}:
let
  # Pin rustc from rust-toolchain.toml so nix and rustup builds match.
  toolchainFor = p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

  # Rustdoc, run from inside the crate's own build. nextest runs every
  # test kind but the doc examples, and no build runs rustdoc at all:
  # buildRustCrate drives rustc directly, so nothing extracts the
  # examples from doc-comments or resolves the intra-doc links between
  # them. But the flags that rustdoc needs are the ones the lib build
  # just used, and build-crate.nix leaves them in the shell that runs
  # `postBuild` — so re-run the compiler it already configured, one flag
  # different. Gated on `buildTests`, so it runs in the test build, whose
  # dependency set includes the dev-dependencies that cargo also links
  # into doc examples. `docTests` is false for a cdylib, which has no rlib for an
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

  cargoPkgs = if stdenv.hostPlatform.isLinux then pkgsStatic else pkgs;
  cargoToolchain = toolchainFor cargoPkgs.buildPackages;

  # Our own crates (local `src`, not a crates.io `sha256`) compile with
  # clippy-driver under Cargo.toml's `[workspace.lints]`; deps stay plain.
  buildRustCrateForPkgs =
    let
      lints = cargoToml.workspace.lints;
    in
    p:
    let
      base = p.buildRustCrate.override {
        rustc = cargoToolchain;
        cargo = cargoToolchain;
        clippy = cargoToolchain;
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
in
rec {
  inherit (cargoToml.workspace.package) version;
  toolchain = toolchainFor pkgs;

  # Cargo.nix is generated — don't hand-edit (regen: docs/generated.md). Build
  # with our pinned rustc, not nixpkgs'; the virtual workspace has no
  # rootCrate, so callers select a member via `workspaceMembers."<name>"`.
  # On Linux every crate links statically against musl, as the release
  # binary does, so the checks test what ships. Darwin has no
  # pkgsStatic and builds natively.
  cargoNix = cargoPkgs.callPackage ./Cargo.nix { inherit buildRustCrateForPkgs; };

  # The test targets of a workspace member, compiled but not run.
  # crate2nix exposes this build only through `internal`.
  testBuild =
    packageId: features:
    (cargoNix.internal.builtRustCratesWithFeatures {
      inherit packageId features;
      runTests = true;
      buildRustCrateForPkgsFunc = buildRustCrateForPkgs;
    }).crates.${packageId}.override
      (_: {
        buildTests = true;
      });

  # The workspace that `cargoMetadata`, the test runs and the `hakari`
  # check load: the manifests, the lock file, the configs in `.config`,
  # and every Rust source as an empty file. cargo finds a target by its
  # file name. No test loads a source file. An edit to a source file thus
  # leaves this tree unchanged. Only a new or removed source file causes
  # the tests to run again.
  workspaceSrc =
    let
      manifests = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          ./Cargo.toml
          ./Cargo.lock
          ./.config
          (lib.fileset.fileFilter (f: f.name == "Cargo.toml") ./crates)
        ];
      };
      sources = map (lib.path.removePrefix ./.) (
        lib.fileset.toList (lib.fileset.fileFilter (f: f.hasExt "rs") ./crates)
      );
    in
    runCommand "nit-workspace" { } ''
      cp -r ${manifests} $out
      chmod -R u+w $out
      cd $out
      for f in ${lib.escapeShellArgs sources}; do
        mkdir -p "$(dirname "$f")"
        touch "$f"
      done
    '';

  # The package graph that nextest loads. Without it, nextest runs
  # `cargo metadata` itself. With `--no-deps`, cargo resolves no
  # dependency, so this derivation needs no vendored crates.
  cargoMetadata = runCommand "nit-cargo-metadata.json" { nativeBuildInputs = [ toolchain ]; } ''
    cd ${workspaceSrc}
    CARGO_HOME=$TMPDIR cargo metadata --no-deps --format-version 1 > $out
  '';

  # Runs the tests of a workspace member under nextest. The binary list
  # that nextest loads comes from the targets in `cargoMetadata` and the
  # files of the test build, so no cargo build runs.
  nextest =
    packageId:
    let
      tests = testBuild packageId [ "default" ];
      libTest = "${tests}/tests/${lib.replaceStrings [ "-" ] [ "_" ] tests.libName}-${tests.metadata}";
    in
    stdenvNoCC.mkDerivation {
      name = "nextest-${packageId}";
      src = workspaceSrc;
      nativeBuildInputs = [
        cargo-nextest
        jq
      ];
      RUST_BACKTRACE = 1;
      buildPhase = ''
        jq --arg pkg ${packageId} --arg dir ${tests}/tests --arg lib ${libTest} \
          --arg target "$PWD/target" '
          (.packages[] | select(.name == $pkg)) as $p
          | {
              "rust-build-meta": {
                "target-directory": $target,
                "base-output-directories": [],
                "non-test-binaries": {},
                "linked-paths": []
              },
              "rust-binaries": INDEX(
                $p.targets[]
                | select(.test)
                | (if any(.kind[]; test("lib$")) then "lib" else .kind[0] end) as $kind
                | {
                    "binary-id": (
                      if $kind == "lib" then $pkg
                      elif $kind == "test" then "\($pkg)::\(.name)"
                      else "\($pkg)::\($kind)/\(.name)" end),
                    "binary-name": .name,
                    "package-id": $p.id,
                    kind: $kind,
                    "binary-path": (if $kind == "lib" then $lib else "\($dir)/\(.name)" end),
                    "build-platform": "target"
                  };
                ."binary-id")
            }' ${cargoMetadata} > binaries.json
        cargo-nextest nextest run \
          --cargo-metadata ${cargoMetadata} \
          --binaries-metadata binaries.json \
          --workspace-remap "$PWD" \
          --no-fail-fast \
          --show-progress none
      '';
      installPhase = "touch $out";
    };

  # The web's wire types, generated from nit-types: the test binary of
  # the `ts`-feature build runs the ts-rs exporter, which writes every
  # web-facing type's declaration into one module, prettier-formatted like
  # any source file. `gen-types` writes it and `types-drift` diffs it.
  # `TS_RS_LARGE_INT=number` maps u64/i64 to the wire's `number`, not
  # bigint.
  wireTypes = runCommand "nit-wire-types.gen.ts" { nativeBuildInputs = [ prettier ]; } ''
    TS_RS_LARGE_INT=number TYPES_GEN_OUT="$PWD/types.gen.ts" \
      ${testBuild "nit-types" [ "ts" ]}/tests/nit_types-* --exact export::write_wire_types
    prettier --write types.gen.ts
    mv types.gen.ts $out
  '';

  # The public contract: the OpenAPI document the routes' utoipa
  # annotations produce and the CLI's clap syntax, written by
  # `nit-contract` into one directory.
  contract = runCommand "nit-contract" { } ''
    ${cargoNix.workspaceMembers."nit-contract".build}/bin/nit-contract $out
  '';

  # The shared change fold compiled to WebAssembly: the crate2nix build of
  # nit-wasm for wasm32, run through wasm-bindgen (`--target web`) into
  # the JS glue + `.wasm` the web imports. The wasm-bindgen-cli version
  # must match nit-wasm's `wasm-bindgen` dep.
  wasm =
    let
      wasmPkgs = pkgs.pkgsCross.wasm32-unknown-none;
      # With rust-src in the sysroot, rustc writes its store path into
      # std's panic locations, and the server binary embeds this wasm.
      rustToolchain = cargoToolchain.override { extensions = [ ]; };
      cargoNixWasm = wasmPkgs.callPackage ./Cargo.nix {
        # The proc-macros and build scripts run on the build platform, so
        # they reuse the derivations of the host build.
        buildRustCrateForPkgs =
          p:
          if !p.stdenv.hostPlatform.isWasm then
            buildRustCrateForPkgs p
          else
            crate:
            p.buildRustCrate.override
              {
                rustc = rustToolchain;
                cargo = rustToolchain;
                defaultCodegenUnits = 16;
              }
              (
                crate
                // {
                  # buildRustCrate leaves CARGO_CFG_TARGET_FEATURE unset, so
                  # without this, wasm-bindgen's build script disables the
                  # externref support that rustc enables on wasm32.
                  preConfigure = ''
                    export CARGO_CFG_TARGET_FEATURE=$(
                      rustc --print cfg --target ${p.stdenv.hostPlatform.rust.rustcTarget} |
                        sed -n 's/^target_feature="\(.*\)"$/\1/p' | paste -sd, -
                    )
                  '';
                }
              );
      };
    in
    runCommand "nit-wasm-pkg" { nativeBuildInputs = [ wasm-bindgen-cli ]; } ''
      wasm-bindgen ${cargoNixWasm.workspaceMembers."nit-wasm".build.lib}/lib/nit_wasm-*.wasm \
        --target web --out-name nit_wasm --out-dir $out
    '';

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
