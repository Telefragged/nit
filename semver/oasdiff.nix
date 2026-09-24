# oasdiff is not in nixpkgs; semver/default.nix diffs the OpenAPI documents
# with it. `nix run .#bump-oasdiff` moves oasdiff.json to the latest
# release.
{
  buildGoModule,
  fetchFromGitHub,
  lib,
}:
let
  pin = lib.importJSON ./oasdiff.json;
in
buildGoModule {
  pname = "oasdiff";
  inherit (pin) version vendorHash;
  src = fetchFromGitHub {
    owner = "oasdiff";
    repo = "oasdiff";
    tag = "v${pin.version}";
    inherit (pin) hash;
  };
  doCheck = false;
}
