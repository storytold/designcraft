#!/usr/bin/env bash
# Build and package DesignCraft for FreeBSD:
#
#   $DIST/designcraft-<version>-freebsd-x86_64.tar.gz   a /usr/local-style tree:
#       designcraft-<version>-freebsd-x86_64/{bin, share/applications, share/icons, share/mime,
#       share/metainfo, share/doc/designcraft}
#
# Install by copying the tree's contents into /usr/local:
#   tar -xzf designcraft-<version>-freebsd-x86_64.tar.gz --strip-components 1 -C /usr/local
#
# Usage: packaging/freebsd/package.sh [--skip-build] [--dry-run]
#   --dry-run   stage the tree from stub binaries and list it, without building (works on any OS)
#
# Needs: bash, cargo, and the packages installed in .github/workflows/freebsd.yml.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
LINUX="$ROOT/packaging/linux"
APP_ID=ai.storyteller.designcraft

SKIP_BUILD=0
DRY_RUN=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --dry-run) DRY_RUN=1; SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

# Release file names use x86_64 and aarch64 like the Linux and Windows ones.
release_arch() {
  local arch="$1"
  case "$arch" in
    amd64 | x86_64) echo x86_64 ;;
    arm64 | aarch64) echo aarch64 ;;
    *) echo "unsupported architecture $arch" >&2; return 2 ;;
  esac
  return
}
# The release workflow cross-compiles aarch64 in an x86_64 VM with CARGO_BUILD_TARGET set to
# aarch64-unknown-freebsd: name the archive after the target, not the host's uname -m.
case "${CARGO_BUILD_TARGET:-}" in
  "") ARCH="$(release_arch "$(uname -m)")" ;;
  aarch64-unknown-freebsd | x86_64-unknown-freebsd) ARCH="$(release_arch "${CARGO_BUILD_TARGET%%-*}")" ;;
  *) echo "CARGO_BUILD_TARGET=$CARGO_BUILD_TARGET is not a supported FreeBSD target" >&2; exit 2 ;;
esac
# Only binaries for this machine can run here (the --version check at the end).
HOST_ARCH="$(release_arch "$(uname -m)" 2>/dev/null)" || HOST_ARCH=""
if [[ "$DRY_RUN" = 0 ]] && [[ "$(uname -s)" != FreeBSD ]]; then
  echo "error: build on FreeBSD (or pass --dry-run to check the tree layout)" >&2
  exit 2
fi
BASENAME="designcraft-$VERSION-freebsd-$ARCH"

echo "==> DesignCraft $VERSION for FreeBSD $ARCH"

# The release VM has 12 GB; full parallelism on the biggest crates runs it out of memory.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
if [[ "$SKIP_BUILD" = 0 ]]; then
  (cd "$ROOT" && cargo build --release --locked ${CARGO_BUILD_TARGET:+--target "$CARGO_BUILD_TARGET"} \
    -p designcraft -p designcraft-cli)
  # The man-and-completions example writes designcraft-cli's man pages and shell completions below. It is built in
  # its own cargo run: built with the binaries, its dev-dependencies' features would be unified into them.
  (cd "$ROOT" && cargo build --release --locked ${CARGO_BUILD_TARGET:+--target "$CARGO_BUILD_TARGET"} \
    -p designcraft-cli --example man-and-completions)
fi
WORK="$CARGO_TARGET_DIR/freebsd-package"
STAGE="$WORK/$BASENAME"
rm -rf "$WORK"
mkdir -p "$WORK"

BIN="$CARGO_TARGET_DIR/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release"
OUT_DIR="$DIST"
if [[ "$DRY_RUN" = 1 ]]; then
  # Stub binaries, and the tarball stays in the work dir so dist/ only ever holds real builds.
  BIN="$WORK/stub-bin"
  OUT_DIR="$WORK"
  mkdir -p "$BIN"
  for b in designcraft designcraft-cli; do
    printf '#!/bin/sh\necho "%s %s (dry-run stub)"\n' "$b" "$VERSION" >"$BIN/$b"
    chmod 755 "$BIN/$b"
  done
  # A stub of the example: it writes the files the real one does.
  mkdir -p "$BIN/examples"
  cat >"$BIN/examples/man-and-completions" <<'STUB'
#!/bin/sh
mkdir -p "$1"
for f in designcraft-cli.bash _designcraft-cli designcraft-cli.fish designcraft-cli.elv _designcraft-cli.ps1 designcraft-cli.1 designcraft-cli-run.1; do
  echo "dry-run stub" >"$1/$f"
done
STUB
  chmod 755 "$BIN/examples/man-and-completions"
fi

# ---- stage a /usr/local-style tree --------------------------------------------------------------
# FreeBSD's install(1) has no -D: create the directories first.
mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$STAGE/share/mime/packages" \
  "$STAGE/share/metainfo" "$STAGE/share/icons" "$STAGE/share/doc/designcraft"
install -m 755 "$BIN/designcraft" "$BIN/designcraft-cli" "$STAGE/bin/"
strip "$STAGE/bin/designcraft" "$STAGE/bin/designcraft-cli" 2>/dev/null || true
# designcraft-cli's man pages and shell completions, from its clap definition, written by the example built above (a
# --skip-build run needs it built too). Under /usr/local, man pages go in man/ and the completions in the directories
# bash, zsh and fish read there.
CLI_DOCS="$BIN/examples/man-and-completions"
if [[ ! -x "$CLI_DOCS" ]]; then
  echo "error: $CLI_DOCS is not built (cargo build --release -p designcraft-cli --example man-and-completions)" >&2
  exit 1
fi
"$CLI_DOCS" "$WORK/cli-docs"
mkdir -p "$STAGE/man/man1" "$STAGE/share/bash-completion/completions" \
  "$STAGE/share/zsh/site-functions" "$STAGE/share/fish/vendor_completions.d"
install -m 644 "$WORK"/cli-docs/*.1 "$STAGE/man/man1/"
install -m 644 "$WORK/cli-docs/designcraft-cli.bash" "$STAGE/share/bash-completion/completions/designcraft-cli"
install -m 644 "$WORK/cli-docs/_designcraft-cli" "$STAGE/share/zsh/site-functions/_designcraft-cli"
install -m 644 "$WORK/cli-docs/designcraft-cli.fish" "$STAGE/share/fish/vendor_completions.d/designcraft-cli.fish"
# The desktop entry, MIME type, metainfo and icons are the freedesktop files Linux ships.
install -m 644 "$LINUX/$APP_ID.desktop" "$STAGE/share/applications/$APP_ID.desktop"
install -m 644 "$LINUX/$APP_ID.mime.xml" "$STAGE/share/mime/packages/$APP_ID.xml"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$DESIGNCRAFT_BUILD_DATE/g" \
  "$LINUX/$APP_ID.metainfo.xml.in" >"$STAGE/share/metainfo/$APP_ID.metainfo.xml"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/share/icons/"
copy_docs "$STAGE/share/doc/designcraft"
for f in NOTICE ATTRIBUTION.md; do
  if [[ -f "$ROOT/$f" ]]; then install -m 644 "$ROOT/$f" "$STAGE/share/doc/designcraft/"; fi
done

for f in bin/designcraft bin/designcraft-cli "share/applications/$APP_ID.desktop" \
  "share/icons/hicolor/256x256/apps/$APP_ID.png" "share/icons/hicolor/scalable/apps/$APP_ID.svg" \
  share/doc/designcraft/LICENSE-MIT share/doc/designcraft/LICENSE-APACHE \
  man/man1/designcraft-cli.1 share/bash-completion/completions/designcraft-cli share/zsh/site-functions/_designcraft-cli \
  share/fish/vendor_completions.d/designcraft-cli.fish; do
  if [[ ! -e "$STAGE/$f" ]]; then echo "error: $f is missing from the package" >&2; exit 1; fi
done

# ---- .tar.gz ------------------------------------------------------------------------------------
mkdir -p "$OUT_DIR"
tar -C "$WORK" -czf "$OUT_DIR/$BASENAME.tar.gz" "$BASENAME"
echo "wrote $OUT_DIR/$BASENAME.tar.gz"

if [[ "$ARCH" = "$HOST_ARCH" ]]; then
  "$STAGE/bin/designcraft-cli" --version
else
  echo "skipping --version: $ARCH binaries do not run on this $HOST_ARCH host"
fi
if [[ "$DRY_RUN" = 1 ]]; then
  echo "==> dry run: tarball contents"
  tar -tzvf "$OUT_DIR/$BASENAME.tar.gz"
fi
echo "==> done"
