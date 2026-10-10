# Checks the version in Cargo.toml against the contract changes since the
# release tagged `last`, before the next release merges:
#
#   nix build -f release --argstr last <tag> '^*'
#
# The build fails when the version is not the next patch, minor or major
# version of `last`, or when it is a smaller step than the changes need.
# The `out` output holds the kind of the changes, `breaking`, `additive` or
# `none`, and the `changes` output one line per change that decided it.
# The HTTP API's changes come from oasdiff and the CLI's from classify.py.
# A behavior change behind an unchanged contract is invisible here, so the
# needed step is a floor for review to raise.
{ last }:
let
  repo = toString ../.;
  system = builtins.currentSystem;
  contract = flake: flake.packages.${system}.contract;
  head = builtins.getFlake repo;
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).workspace.package.version;
  old = contract (builtins.getFlake "git+file://${repo}?ref=refs/tags/${last}");
  new = contract head;
  pkgs = head.inputs.nixpkgs.legacyPackages.${system};
in
# The binary takes its version from Cargo.nix, so a release must not
# merge while Cargo.nix holds a different one.
assert
  head.packages.${system}.nit.version == version
  || throw "Cargo.nix holds version ${head.packages.${system}.nit.version}, run crate2nix generate";
pkgs.runCommand "nit-semver"
  {
    outputs = [
      "out"
      "changes"
    ];
    nativeBuildInputs = [
      (pkgs.callPackage ./oasdiff.nix { })
      pkgs.python3
    ];
  }
  ''
    oasdiff changelog --format json ${old}/openapi.json ${new}/openapi.json > api.json
    python3 ${./classify.py} api.json ${old}/cli.json ${new}/cli.json $changes $out
    if ! python3 ${./gate.py} ${last} ${version} "$(cat $out)"; then
      cat $changes >&2
      exit 1
    fi
  ''
