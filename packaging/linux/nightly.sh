#!/usr/bin/env bash
# App-owned GUI-only nightly tar: no FUSE/AppImage, CLI, association or install hooks.
set -euo pipefail
ARCH=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) [[ $# -ge 2 ]] || { echo 'missing --arch value' >&2; exit 2; }; ARCH="$2"; shift 2 ;;
    -h|--help) echo 'Usage: nightly.sh --arch x86_64'; exit 0 ;;
    *) echo "unsupported nightly argument: $1" >&2; exit 2 ;;
  esac
done
[[ "$ARCH" == x86_64 && "$(uname -s)" == Linux ]] || { echo 'only native Linux x86_64 nightly packaging is supported' >&2; exit 2; }
. "$(dirname "${BASH_SOURCE[0]}")/../nightly-common.sh"
nightly_init linux "$ARCH" x86_64-unknown-linux-gnu
OUTPUT="$DIST/designcraft-$VERSION-linux-x86_64-nightly.tar.gz"
[[ ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] || nightly_die 'refusing to replace an existing nightly archive'
nightly_build_gui
STAGE="$NIGHTLY_WORK/stage"
mkdir -p "$STAGE"
cp "$NIGHTLY_GUI" "$STAGE/designcraft-bin"
chmod 755 "$STAGE/designcraft-bin"
# An unpacked nightly starts the flavor the way every other Linux package does (the Launcher's
# selector/root pair), so it never falls back to stable's ~/.config/designcraft profile.
cat >"$STAGE/designcraft" <<'LAUNCHER'
#!/bin/sh
set -eu
here="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"
real_bin="$here/designcraft-bin"
if [ "${1:-}" = "--launch-profile" ] || [ "${1:-}" = "--profile-root" ]; then
  # The Launcher already supplied the protocol: pass its command line through untouched.
  exec "$real_bin" "$@"
fi
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "-V" ]; then
  exec "$real_bin" "$@"
fi
profile_root="${XDG_DATA_HOME:-${HOME:?HOME is required}/.local/share}/designcraft-nightly"
if [ "${1:-}" = "--" ]; then
  # The caller's own separator stays the only one.
  exec "$real_bin" --launch-profile nightly-v1 --profile-root "$profile_root" "$@"
fi
exec "$real_bin" --launch-profile nightly-v1 --profile-root "$profile_root" -- "$@"
LAUNCHER
chmod 755 "$STAGE/designcraft"
cp "$ROOT/assets/app-icon/designcraft-nightly.png" "$STAGE/designcraft-nightly.png"
nightly_copy_notices "$STAGE"
# The staged launcher must reach the built flavor, not just the binary it names.
packed_version="$("$STAGE/designcraft" --version)" || nightly_die 'the archive launcher could not start the built binary'
[[ "$packed_version" == "DesignCraft $VERSION (nightly-v1)" ]] || nightly_die 'the archive launcher is not the prepared nightly identity'
# The private mktemp cwd is the helper authority root, not a CLI path parent.
(cd "$NIGHTLY_WORK" && python3 "$ROOT/packaging/nightly-archive.py" linux stage archive.tar.gz)
nightly_publish_archive "$NIGHTLY_WORK/archive.tar.gz" "$OUTPUT"
