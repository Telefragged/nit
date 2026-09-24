# The smallest semver bump that covers the contract changes since the
# commit `base`, for a release to check before it merges:
#
#   nix build -f semver --argstr base "$(git rev-parse <tag>)" '^*'
#
# The `out` output holds the bump, `major`, `minor` or `patch`, and the
# `changes` output one line per change that decided it. The HTTP API's
# changes come from oasdiff and the CLI's from classify.py. A behavior
# change behind an unchanged contract is invisible here, so the bump is a
# floor for review to raise.
{ base }:
let
  repo = toString ../.;
  system = builtins.currentSystem;
  contract = flake: flake.packages.${system}.contract;
  head = builtins.getFlake repo;
  old = contract (builtins.getFlake "git+file://${repo}?rev=${base}");
  new = contract head;
  pkgs = head.inputs.nixpkgs.legacyPackages.${system};
in
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
  ''
