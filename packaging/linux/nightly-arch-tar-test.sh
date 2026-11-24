#!/usr/bin/env bash
# Validate the staged Arch launcher without building or opening DesignCraft.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/source"
dst="$work/dist"
base=designcraft-0.0.1-nightly.1-linux-x86_64
mkdir -p "$stage/bin" "$dst" "$work/extract"

# A local env + argv recorder stands in for the compiled binaries. It never opens a UI.
cat >"$stage/bin/designcraft" <<'RECORDER'
#!/bin/sh
printf 'CONFIG=<%s>\n' "${DESIGNCRAFT_CONFIG_DIR-UNSET}"
for arg in "$@"; do
  printf '<%s>\n' "$arg"
done
RECORDER
chmod 755 "$stage/bin/designcraft"
cp "$stage/bin/designcraft" "$stage/bin/designcraft-cli"

bash "$HERE/nightly-arch-tar.sh" "$stage" "$work/stage" "$dst" "$base"
tar -xzf "$dst/$base.tar.gz" -C "$work/extract"
bin="$work/extract/$base/bin"
# Simulate the existing Arch bin-tree copy and public launcher rename.
mkdir -p "$work/installed"
cp -R "$bin" "$work/installed/bin"
bin="$work/installed/bin"
mv "$bin/designcraft" "$bin/designcraft-nightly"
mv "$bin/designcraft-cli" "$bin/designcraft-nightly-cli"

[[ ! -e "$stage/bin/.designcraft-nightly" ]]
cmp "$stage/bin/designcraft" "$bin/.designcraft-nightly/designcraft"
cmp "$stage/bin/designcraft-cli" "$bin/designcraft-nightly-cli"
export HOME="$work/home with space"
unset XDG_DATA_HOME DESIGNCRAFT_CONFIG_DIR
expect() {
  expected="$1"
  shift
  actual=$(PATH="$bin:$PATH" designcraft-nightly "$@")
  if [[ "$actual" != "$expected" ]]; then
    printf 'expected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
    exit 1
  fi
}
expect "CONFIG=<$HOME/.local/share/designcraft-nightly>"
export XDG_DATA_HOME="$work/xdg with space"
expect "$(printf 'CONFIG=<%s>\n<%s>\n<%s>\n' "$XDG_DATA_HOME/designcraft-nightly" 'doc with spaces.dc' '--other-name.dc')" \
  'doc with spaces.dc' '--other-name.dc'
expect "$(printf 'CONFIG=<UNSET>\n<--version>\n')" --version
expect "$(printf 'CONFIG=<UNSET>\n<-V>\n')" -V
DESIGNCRAFT_CONFIG_DIR="$work/explicit with space" expect \
  "$(printf 'CONFIG=<%s>\n<%s>\n' "$work/explicit with space" 'sample.dc')" sample.dc

echo "PASS: Arch Nightly handoff isolates the profile and preserves launch arguments"
