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
# An unpacked nightly starts through the canonical shared launcher template
# (packaging/linux/nightly-launch.sh, rendered from craft-common-actions's
# scripts/nightly-launcher.sh.in): `designcraft` starts `designcraft-bin` beside it on the
# nightly-v1 profile under the Nightly package identity.
install -m755 "$ROOT/packaging/linux/nightly-launch.sh" "$STAGE/designcraft"
cp "$ROOT/assets/app-icon/designcraft-nightly.png" "$STAGE/designcraft-nightly.png"
nightly_copy_notices "$STAGE"
# The staged launcher must reach the built flavor, not just the binary it names.
packed_version="$("$STAGE/designcraft" --version)" || nightly_die 'the archive launcher could not start the built binary'
[[ "$packed_version" == "DesignCraft $VERSION (nightly-v1)" ]] || nightly_die 'the archive launcher is not the prepared nightly identity'
# The private mktemp cwd is the helper authority root, not a CLI path parent.
(cd "$NIGHTLY_WORK" && python3 "$ROOT/packaging/nightly-archive.py" linux stage archive.tar.gz)
nightly_publish_archive "$NIGHTLY_WORK/archive.tar.gz" "$OUTPUT"
