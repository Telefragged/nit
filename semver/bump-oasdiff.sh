# Moves semver/oasdiff.json to the latest oasdiff release. Run from anywhere in
# the repo: `nix run .#bump-oasdiff`.

cd "$(git rev-parse --show-toplevel)"
pin=semver/oasdiff.json

tag=$(curl -fsSL https://api.github.com/repos/oasdiff/oasdiff/releases/latest |
  jq -r .tag_name)
hash=$(nix flake prefetch --json "github:oasdiff/oasdiff/$tag" | jq -r .hash)

# Go modules have no prefetcher, so build them against a placeholder hash
# and take the hash that nix reports instead.
jq -n --arg version "${tag#v}" --arg hash "$hash" \
  '{version: $version, hash: $hash, vendorHash: "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}' \
  >"$pin"
modules="with (builtins.getFlake \"$PWD\").inputs.nixpkgs.legacyPackages.\${builtins.currentSystem};
  (callPackage $PWD/semver/oasdiff.nix { }).goModules"
vendor_hash=$(nix build --impure --no-link --expr "$modules" 2>&1 |
  sed -n 's/^ *got: *//p' || true)
if [ -z "$vendor_hash" ]; then
  echo "could not read the vendor hash of oasdiff $tag" >&2
  exit 1
fi
jq --arg vendorHash "$vendor_hash" '.vendorHash = $vendorHash' "$pin" >"$pin.new"
mv "$pin.new" "$pin"
echo "oasdiff ${tag#v}"
