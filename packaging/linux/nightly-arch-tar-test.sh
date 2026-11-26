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

# A local argv recorder stands in for the compiled binary. It never opens a UI.
cat >"$stage/bin/designcraft" <<'RECORDER'
#!/bin/sh
printf 'DESIGNCRAFT_CONFIG_DIR=<%s>\n' "${DESIGNCRAFT_CONFIG_DIR-UNSET}"
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
# The nightly profile is the Launcher's selector/root pair, never an environment override.
pair() {
  printf 'DESIGNCRAFT_CONFIG_DIR=<UNSET>\n<--launch-profile>\n<nightly-v1>\n<--profile-root>\n<%s>\n<-->\n%s' "$1" "$2"
}
expect "$(pair "$HOME/.local/share/designcraft-nightly" '')"
export XDG_DATA_HOME="$work/xdg with space"
expect "$(pair "$XDG_DATA_HOME/designcraft-nightly" "$(printf '<%s>\n<%s>\n' 'doc with spaces.dc' '--other-name.dc')")" \
  'doc with spaces.dc' '--other-name.dc'
# Version queries stay plain so a Launcher or a runner can ask before any profile exists.
expect "$(printf 'DESIGNCRAFT_CONFIG_DIR=<UNSET>\n<--version>\n')" --version
expect "$(printf 'DESIGNCRAFT_CONFIG_DIR=<UNSET>\n<-V>\n')" -V
# A caller that already uses the protocol (the Launcher) reaches the binary untouched.
expect "$(printf 'DESIGNCRAFT_CONFIG_DIR=<UNSET>\n<--launch-profile>\n<nightly-v1>\n<--profile-root>\n<%s>\n<-->\n<%s>\n' \
   "$work/launcher root" 'doc.dc')" \
  --launch-profile nightly-v1 --profile-root "$work/launcher root" -- doc.dc
# A caller's own separator stays the only one, and a flag after it is a document.
expect "$(pair "$XDG_DATA_HOME/designcraft-nightly" "$(printf '<%s>\n' '--version')")" -- --version
# The launcher never sets or clears the stable override itself: it passes the profile pair and
# leaves an inherited value alone, which the app then refuses with its own message.
DESIGNCRAFT_CONFIG_DIR="$work/explicit with space" expect \
  "$(printf 'DESIGNCRAFT_CONFIG_DIR=<%s>\n<--launch-profile>\n<nightly-v1>\n<--profile-root>\n<%s>\n<-->\n<%s>\n' \
     "$work/explicit with space" "$XDG_DATA_HOME/designcraft-nightly" 'sample.dc')" sample.dc

echo "PASS: Arch Nightly handoff starts the flavor with the Launcher's profile and arguments"
