# The web frontend: the bundle that the nit binary serves, and the web
# checks. flake.nix calls this with callPackage once per system.
{
  lib,
  buildNpmPackage,
  fetchNpmDeps,
  nodejs,
  playwright-driver,
  makeFontsConf,
  liberation_ttf,
  dejavu_fonts,
  version,
  # The WebAssembly fold, which every web derivation copies into src/wasm.
  wasm,
}:
let
  fs = lib.fileset;

  # The web tree for each web derivation, without the parts that it cannot
  # use, so an edit rebuilds only the derivations that use the edited file.
  src =
    let
      named = suffixes: fs.fileFilter (f: builtins.any (s: lib.hasSuffix s f.name) suffixes) ./.;
      tests = fs.unions [
        (named [
          ".test.ts"
          ".test.tsx"
          "test-setup.ts"
        ])
        ./src/test
      ];
      stories = named [ ".story.tsx" ];
      without =
        parts:
        fs.toSource {
          root = ./.;
          fileset = fs.difference ./. (fs.unions ([ ./default.nix ] ++ parts));
        };
    in
    {
      npm = fs.toSource {
        root = ./.;
        fileset = fs.unions [
          ./package.json
          ./package-lock.json
        ];
      };
      bundle = without [
        tests
        stories
        ./gallery
        ./screenshots
      ];
      lint = without [ ];
      test = without [
        stories
        ./gallery
        ./screenshots
      ];
      screenshots = without [
        tests
        stories
        (fs.difference ./gallery ./gallery/act.ts)
      ];
      gallery = without [
        tests
        ./screenshots
      ];
    };

  # The web npm closure, shared via `npmDeps` so the nit-web build and
  # the web-lint / web-test checks don't each refetch it.
  npmDeps = fetchNpmDeps {
    src = src.npm;
    inherit nodejs;
    name = "nit-web-npm-deps";
    hash = "sha256-vgfWJhLwJ9ERTBJSZMF1k1Qnib7CHw+jWLar5XGlyzs=";
  };

  # Drop the generated wasm glue into a web build's tree before it runs.
  # web/src/wasm is gitignored (so absent from the npm-package source), so
  # every web derivation injects it the way `gen-wasm` does for local dev
  # — the artifact is built by nix, never committed.
  injectWasm = ''
    mkdir -p src/wasm
    cp ${wasm}/* src/wasm/
  '';

  # The screenshot harness (web/screenshots/capture.mjs) and the gallery
  # (web/gallery/) launch only chromium, so the browser set has no
  # firefox and no webkit.
  playwrightBrowsers = playwright-driver.selectBrowsers {
    withFirefox = false;
    withWebkit = false;
  };

  browserEnv = {
    PLAYWRIGHT_BROWSERS_PATH = playwrightBrowsers;
    PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
    # The build sandbox has no user namespaces and no writable
    # /dev/shm.
    NIT_SCREENSHOT_NO_SANDBOX = "1";
    # The bundled fonts (src/styles.ts) cover Latin text. A character
    # outside them uses the generic sans or mono family, which resolves
    # to no font in the sandbox. Map both families to real fonts, so
    # that the captures show such a character and not an empty box.
    FONTCONFIG_FILE = makeFontsConf {
      fontDirectories = [
        liberation_ttf
        dejavu_fonts
      ];
    };
  };

  # A web validator that runs `script` against `src.<name>`. It sets
  # dontNpmBuild, which disables the build phase (and its preBuild hook),
  # so the wasm goes in at the head of the install phase.
  webCheck =
    name: env: script:
    buildNpmPackage {
      pname = "nit-web-${name}";
      inherit version;
      src = src.${name};
      inherit nodejs;
      inherit env npmDeps;
      dontNpmBuild = true;
      installPhase = injectWasm + script;
    };
in
{
  inherit playwrightBrowsers;

  bundle = buildNpmPackage {
    pname = "nit-web";
    inherit version npmDeps;
    src = src.bundle;
    inherit nodejs;
    preBuild = injectWasm;
    installPhase = "cp -r dist $out";
  };

  checks = {
    # The frontend type check and lint (tsc, eslint, stylelint, knip) as a
    # validator, the web counterpart to clippy. It runs the devShell
    # `npm run lint`, so `nix flake check` fails on a type, eslint,
    # stylelint or knip error.
    web-lint = webCheck "lint" { } "npm run lint > $out";
    web-test = webCheck "test" { } "npm run test > $out";
    # The same `capture.mjs` agents run locally, but against the nix
    # `playwright-driver` browsers, so a driver/npm-pin skew fails the
    # check. Its output is the PNGs, not a marker file.
    web-screenshots = webCheck "screenshots" browserEnv ''
      export HOME=$TMPDIR
      NIT_SCREENSHOT_OUT_DIR=$out node screenshots/capture.mjs
    '';
    # Runs the gallery specs (docs/web.md "Component gallery"). Its output
    # is the PNG and the page of every story.
    web-gallery = webCheck "gallery" browserEnv ''
      export HOME=$TMPDIR
      npm run gallery -- --output $out
    '';
  };
}
