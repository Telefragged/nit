# The web frontend: the bundle that the nit binary serves, and the web
# checks. flake.nix calls this with callPackage once per system.
{
  lib,
  stdenvNoCC,
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

  npmDeps = fetchNpmDeps {
    src = src.npm;
    inherit nodejs;
    name = "nit-web-npm-deps";
    hash = "sha256-eloY97d+h8NMWGC1hr7TLK8Qyg28mnsBzmh0e+p8Few=";
  };

  # The installed npm dependencies. Every web derivation copies them, so
  # `npm ci` runs once and not once per derivation.
  nodeModules = buildNpmPackage {
    pname = "nit-web-node-modules";
    inherit version nodejs npmDeps;
    src = src.npm;
    dontNpmBuild = true;
    installPhase = "cp -r node_modules $out";
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

  # A web derivation that runs `script` against `src.<name>`, with the npm
  # dependencies and the wasm in place. The tools write their caches into
  # node_modules, so its top directory must be writable.
  webBuild =
    name: env: script:
    stdenvNoCC.mkDerivation {
      pname = if name == "bundle" then "nit-web" else "nit-web-${name}";
      inherit version env;
      src = src.${name};
      nativeBuildInputs = [ nodejs ];
      configurePhase = ''
        export HOME=$TMPDIR
        cp -r ${nodeModules} node_modules
        chmod u+w node_modules
      ''
      + injectWasm;
      buildPhase = script;
      dontInstall = true;
    };
in
{
  inherit playwrightBrowsers;

  bundle = webBuild "bundle" { } ''
    npm run build
    cp -r dist $out
  '';

  checks = {
    # The frontend type check and lint (tsc, eslint, stylelint, knip) as a
    # validator, the web counterpart to clippy. It runs the devShell
    # `npm run lint`, so `nix flake check` fails on a type, eslint,
    # stylelint or knip error.
    web-lint = webBuild "lint" { } "npm run lint > $out";
    web-test = webBuild "test" { } "npm run test > $out";
    # The same `capture.mjs` agents run locally, but against the nix
    # `playwright-driver` browsers, so a driver/npm-pin skew fails the
    # check.
    web-screenshots = webBuild "screenshots" browserEnv ''
      NIT_SCREENSHOT_OUT_DIR=$TMPDIR/screenshots node screenshots/capture.mjs
      touch $out
    '';
    # Runs the gallery specs (docs/web.md "Component gallery").
    web-gallery = webBuild "gallery" browserEnv ''
      npm run gallery -- --output $TMPDIR/gallery
      touch $out
    '';
  };
}
