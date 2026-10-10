# Decides whether HEAD is a release, and checks its version when it is.
# HEAD is a release when the version in Cargo.toml differs from the newest
# release tag, or when no release tag exists yet. Writes `version=` and
# `release=true|false` to $GITHUB_OUTPUT when it is set. Run it with
# `nix run .#release-check`.
cd "$(git rev-parse --show-toplevel)"

version=$(taplo get -f Cargo.toml workspace.package.version)
last=$(git tag --list '*.*.*' --sort=-v:refname | head -n 1)

if [ -z "$last" ]; then
  echo "$version is the first release"
  release=true
elif [ "$version" = "$last" ]; then
  echo "$version is released already"
  release=false
else
  nix build -f release --argstr last "$last" --no-link '^*'
  echo "$version is a release after $last"
  release=true
fi

printf 'version=%s\nrelease=%s\n' "$version" "$release" >>"${GITHUB_OUTPUT:-/dev/null}"
