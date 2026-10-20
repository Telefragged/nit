#!/bin/sh
# Installs the latest nit release for this user, or replaces the
# installed one.
#
#   curl -fsSL https://github.com/Telefragged/nit/releases/latest/download/install.sh | sh
#
# It installs to $NIT_INSTALL_DIR, default ~/.local/bin. Set
# NIT_NO_MODIFY_PATH=1 to keep it from adding that directory to PATH in
# the shell startup files.
set -eu

url=https://github.com/Telefragged/nit/releases/latest/download
dir=${NIT_INSTALL_DIR:-$HOME/.local/bin}

case $(uname -sm) in
"Linux x86_64") asset=nit-x86_64-linux ;;
*)
  echo "install.sh: nit publishes no build for $(uname -sm)" >&2
  exit 1
  ;;
esac

mkdir -p "$dir"
# The download goes to the install directory, so the rename below is
# atomic. A running server reads that rename as its signal to restart.
staging=$(mktemp -d "$dir/.nit-install.XXXXXX")
trap 'rm -rf "$staging"' EXIT
curl -fsSL -o "$staging/$asset" "$url/$asset"
curl -fsSL -o "$staging/$asset.sha256" "$url/$asset.sha256"
(cd "$staging" && sha256sum --check --quiet "$asset.sha256")
chmod 755 "$staging/$asset"
mv -f "$staging/$asset" "$dir/nit"
echo "installed nit $("$dir/nit" --client-version) to $dir/nit"

if [ -z "${NIT_NO_MODIFY_PATH:-}" ]; then
  case ":$PATH:" in
  *":$dir:"*) ;;
  *)
    line="export PATH=\"$dir:\$PATH\""
    # A login shell of sh or bash reads .profile.
    [ -e "$HOME/.profile" ] || : >"$HOME/.profile"
    for rc in "$HOME/.profile" "$HOME/.bash_profile" "$HOME/.bashrc" "$HOME/.zshenv"; do
      [ -e "$rc" ] || continue
      if grep -qsxF "$line" "$rc"; then
        :
      elif [ -w "$rc" ]; then
        echo "$line" >>"$rc"
        echo "added $dir to PATH in $rc; open a new shell to use it"
      else
        # For example, home-manager links its files from the read-only
        # nix store.
        echo "cannot write $rc, so add $dir to PATH there yourself"
      fi
    done
    ;;
  esac
fi

"$dir/nit" install-plugin
