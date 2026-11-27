#!/usr/bin/env bash
# Build and package DesignCraft for Linux (<arch> is x86_64 or aarch64, or riscv64 when cross-compiling):
#
#   $DIST/designcraft-<version>-linux-<arch>.AppImage  any distro with glibc >= the build host's
#   $DIST/designcraft-<version>-linux-<arch>.AppImage.zsync  delta updates (needs zsyncmake)
#   $DIST/designcraft-<version>-linux-<arch>.deb       Debian, Ubuntu, Mint, Pop!_OS, ...
#   $DIST/designcraft-<version>-linux-<arch>.rpm       Fedora, openSUSE, RHEL, ...
#   $DIST/designcraft-<version>-linux-<arch>.tar.gz    plain FHS-style tree (bin/, share/)
#
# The Arch package (.pkg.tar.zst) is not built here. The release workflow builds it with
# craft-common-actions/actions/arch-package from this script's tarball, which names it after the
# .desktop bundle id (ai.storyteller.designcraft, not designcraft, so an AUR helper does not claim
# it) and declares provides/conflicts against the AUR packages. Building a second one here under
# the same asset name is what made the published package depend on upload order.
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm tar"] [--arch-tar-dir DIRECTORY]
#
# Needs: cargo; nfpm for deb/rpm (https://nfpm.goreleaser.com); appimagetool for the AppImage
# (downloaded into $CARGO_TARGET_DIR if missing). Build on an old distro (CI: Ubuntu 22.04,
# glibc 2.35) so the binaries run on newer ones. Optional: desktop-file-validate, appstreamcli,
# zsyncmake (the zsync package) for the AppImage's .zsync.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"
APP_ID=ai.storyteller.designcraft

SKIP_BUILD=0
FORMATS="appimage deb rpm tar"
ARCH_TAR_DIR=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    --arch-tar-dir)
      [[ -n "${2:-}" ]] || { echo "error: --arch-tar-dir needs a directory" >&2; exit 2; }
      ARCH_TAR_DIR="$2"
      shift 2
      ;;
    -h | --help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="${CROSS_ARCH:-$(uname -m)}"
case "$ARCH" in
  riscv64) DEB_ARCH=riscv64 ;;
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
export DESIGNCRAFT_MAINTAINER="${DESIGNCRAFT_MAINTAINER:-DesignCraft maintainers <designcraft@storyteller.ai>}"
IS_NIGHTLY=0
if [[ "${DESIGNCRAFT_BUILD_CHANNEL:-}" == nightly-v1 || "$VERSION" == *-nightly* ]]; then
  IS_NIGHTLY=1
  export DESIGNCRAFT_BUILD_CHANNEL=nightly-v1
  export DESIGNCRAFT_OVERRIDE_VERSION="$VERSION"
fi
BASENAME="designcraft-$VERSION-linux-$ARCH"

if [[ -n "$ARCH_TAR_DIR" && "$IS_NIGHTLY" != 1 ]]; then
  echo "error: --arch-tar-dir is only for Nightly builds" >&2
  exit 2
fi

echo "==> DesignCraft $VERSION for Linux $ARCH ($FORMATS, channel=${DESIGNCRAFT_BUILD_CHANNEL:-stable})"

has() {
  local fmt="$1"
  case " $FORMATS " in *" $fmt "*) return 0 ;; *) return 1 ;; esac
}

if has arch; then
  echo "error: the Arch package is built by the release workflow's makepkg job, not by this script" >&2
  exit 2
fi

if [[ "$SKIP_BUILD" = 0 ]]; then
  if [[ -n "${CROSS_TARGET:-}" ]]; then
    (cd "$ROOT" && cargo build --release --locked -p designcraft -p designcraft-cli --target "$CROSS_TARGET")
  else
    (cd "$ROOT" && cargo build --release --locked -p designcraft -p designcraft-cli)
  fi
fi
BIN="$CARGO_TARGET_DIR/${CROSS_TARGET:+$CROSS_TARGET/}release"
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- stage an FHS tree (shared by every format) -------------------------------------------------
install -Dm755 "$BIN/designcraft" "$STAGE/usr/bin/designcraft"
install -Dm755 "$BIN/designcraft-cli" "$STAGE/usr/bin/designcraft-cli"
"${CROSS_COMPILE:-}strip" "$STAGE/usr/bin/designcraft" "$STAGE/usr/bin/designcraft-cli" 2>/dev/null || true
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
if [[ "$IS_NIGHTLY" = 1 ]]; then
  sed -e 's|^StartupWMClass=.*|StartupWMClass=ai.storyteller.designcraft.nightly-v1|' \
      "$STAGE/usr/share/applications/$APP_ID.desktop" >"$STAGE/usr/share/applications/$APP_ID.desktop.tmp"
  mv "$STAGE/usr/share/applications/$APP_ID.desktop.tmp" "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
install -Dm644 "$HERE/$APP_ID.mime.xml" "$STAGE/usr/share/mime/packages/$APP_ID.xml"
install -Dm644 "$HERE/$APP_ID.thumbnailer" "$STAGE/usr/share/thumbnailers/$APP_ID.thumbnailer"
mkdir -p "$STAGE/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$DESIGNCRAFT_BUILD_DATE/g" \
  "$HERE/$APP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/usr/share/icons/"
mkdir -p "$STAGE/usr/share/doc/designcraft"
copy_docs "$STAGE/usr/share/doc/designcraft"

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
fi

# ---- .tar.gz ------------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# The Arch handoff keeps the same basename as the plain tarball in a separate directory.
# Flatpak, OCI and direct tar consumers still get the original binary, not this launcher.
if [[ -n "$ARCH_TAR_DIR" ]]; then
  mkdir -p "$ARCH_TAR_DIR"
  if [[ "$(cd "$ARCH_TAR_DIR" && pwd -P)" == "$(cd "$DIST" && pwd -P)" ]]; then
    echo "error: the Arch tar directory must differ from the release directory" >&2
    exit 2
  fi
  bash "$HERE/nightly-arch-tar.sh" "$STAGE/usr" "$WORK/arch-tar" "$ARCH_TAR_DIR" "$BASENAME"
fi

# ---- .deb / .rpm --------------------------------------------------------------------------------
if has deb || has rpm; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export VERSION
  export NFPM_ARCH="$DEB_ARCH"
  NFPM_STAGE="$STAGE"
  export NFPM_NAME="designcraft"
  export NFPM_BIN="designcraft"
  export NFPM_CLI="designcraft-cli"
  if [[ "$IS_NIGHTLY" = 1 ]]; then
    NIGHTLY_DESK_ID="$APP_ID.nightly-v1"
    export NFPM_NAME="designcraft-nightly"
    export NFPM_BIN="designcraft-nightly"
    export NFPM_CLI="designcraft-nightly-cli"
    NFPM_STAGE="$WORK/nfpm-root"
    rm -rf "$NFPM_STAGE"
    cp -R "$STAGE" "$NFPM_STAGE"
    install -Dm755 "$NFPM_STAGE/usr/bin/designcraft" "$NFPM_STAGE/usr/lib/designcraft-nightly/designcraft"
    cat >"$NFPM_STAGE/usr/bin/$NFPM_BIN" <<'WRAPPER'
#!/bin/sh
# Start the compiled nightly-v1 flavor. Its profile is the Launcher's protocol (the app refuses a
# DESIGNCRAFT_CONFIG_DIR override), so this wrapper passes the same pair the Launcher will.
set -eu
real_bin=/usr/lib/designcraft-nightly/designcraft
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
WRAPPER
    chmod 755 "$NFPM_STAGE/usr/bin/$NFPM_BIN"
    rm -f "$NFPM_STAGE/usr/bin/designcraft"
    mv "$NFPM_STAGE/usr/bin/designcraft-cli" "$NFPM_STAGE/usr/bin/$NFPM_CLI"
    mv "$NFPM_STAGE/usr/share/doc/designcraft" "$NFPM_STAGE/usr/share/doc/$NFPM_NAME"
    sed -e "s#^Exec=designcraft\([[:space:]]\|$\)#Exec=$NFPM_BIN\1#" \
        -e "s|^TryExec=designcraft$|TryExec=$NFPM_BIN|" \
        -e "s|^Icon=$APP_ID$|Icon=$NIGHTLY_DESK_ID|" \
        -e "s|^Name=DesignCraft$|Name=DesignCraft Nightly|" \
        -e '/^MimeType=/d' \
        "$NFPM_STAGE/usr/share/applications/$APP_ID.desktop" >"$NFPM_STAGE/usr/share/applications/$NIGHTLY_DESK_ID.desktop"
    rm -f "$NFPM_STAGE/usr/share/applications/$APP_ID.desktop"
    sed -e "s|<id>$APP_ID</id>|<id>$NIGHTLY_DESK_ID</id>|g" \
        -e "s|<name>DesignCraft</name>|<name>DesignCraft Nightly</name>|g" \
        -e "s|<launchable type=\"desktop-id\">$APP_ID.desktop</launchable>|<launchable type=\"desktop-id\">$NIGHTLY_DESK_ID.desktop</launchable>|g" \
        -e "s|<icon type=\"stock\">$APP_ID</icon>|<icon type=\"stock\">$NIGHTLY_DESK_ID</icon>|g" \
        -e "s|<binary>designcraft</binary>|<binary>$NFPM_BIN</binary>|g" \
        "$NFPM_STAGE/usr/share/metainfo/$APP_ID.metainfo.xml" >"$NFPM_STAGE/usr/share/metainfo/$NIGHTLY_DESK_ID.metainfo.xml"
    rm -f "$NFPM_STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
    rm -rf "$NFPM_STAGE/usr/share/mime" "$NFPM_STAGE/usr/share/thumbnailers"
    NIGHTLY_ICON="$ROOT/assets/app-icon/designcraft-nightly.png"
    if [[ ! -f "$NIGHTLY_ICON" ]]; then
      NIGHTLY_ICON="$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png"
    fi
    rm -rf "$NFPM_STAGE/usr/share/icons/hicolor"
    install -Dm644 "$NIGHTLY_ICON" \
      "$NFPM_STAGE/usr/share/icons/hicolor/256x256/apps/$NIGHTLY_DESK_ID.png"
  fi
  # nfpm expands env vars in fields like `version` and `arch`, but not in `contents[].src`.
  DROP_LIB_TREE=
  # shellcheck disable=SC2016
  [[ -d "$NFPM_STAGE/usr/lib" ]] || DROP_LIB_TREE='\|^  - src: \${STAGE}/usr/lib/|,+2d;'
  # shellcheck disable=SC2016
  sed "${DROP_LIB_TREE}s|\${STAGE}|$NFPM_STAGE|g;s|\${NFPM_BIN}|$NFPM_BIN|g;s|\${NFPM_CLI}|$NFPM_CLI|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  for fmt in deb rpm; do
    if has "$fmt"; then (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt"); fi
  done
fi

# ---- AppImage -----------------------------------------------------------------------------------
if has appimage; then
  APPIMAGETOOL_VERSION=1.9.1
  case "$ARCH" in
    x86_64) APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0 ;;
    aarch64) APPIMAGETOOL_SHA256=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158 ;;
    # No pinned download (e.g. riscv64): set APPIMAGETOOL or put appimagetool on PATH.
    *) APPIMAGETOOL_SHA256="" ;;
  esac
  APPDIR="$WORK/DesignCraft.AppDir"
  cp -R "$STAGE" "$APPDIR"
  # Keep sctk-adwaita's MIT notice in the AppImage.
  install -Dm644 "$ROOT/NOTICE" "$APPDIR/usr/share/doc/designcraft/NOTICE"
  if [[ "$IS_NIGHTLY" = 1 ]]; then
    APPIMAGE_ID="$APP_ID.nightly-v1"
    cat >"$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
# Start the compiled nightly-v1 flavor. A real file, never a symlink to the binary: the flavor
# needs the Launcher's profile pair, and the app refuses a DESIGNCRAFT_CONFIG_DIR override.
set -eu
APPDIR="${APPDIR:-$(cd "$(dirname "$0")" && pwd)}"
real_bin="$APPDIR/usr/bin/designcraft"
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
APPRUN
    chmod 755 "$APPDIR/AppRun"
    sed -e "s|^Icon=$APP_ID$|Icon=$APPIMAGE_ID|" \
        -e "s|^Name=DesignCraft$|Name=DesignCraft Nightly|" \
        -e '/^MimeType=/d' \
        "$APPDIR/usr/share/applications/$APP_ID.desktop" >"$APPDIR/usr/share/applications/$APPIMAGE_ID.desktop"
    rm -f "$APPDIR/usr/share/applications/$APP_ID.desktop"
    cp "$APPDIR/usr/share/applications/$APPIMAGE_ID.desktop" "$APPDIR/$APPIMAGE_ID.desktop"
    NIGHTLY_ICON="$ROOT/assets/app-icon/designcraft-nightly.png"
    if [[ ! -f "$NIGHTLY_ICON" ]]; then
      NIGHTLY_ICON="$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png"
    fi
    rm -rf "$APPDIR/usr/share/icons/hicolor"
    install -Dm644 "$NIGHTLY_ICON" \
      "$APPDIR/usr/share/icons/hicolor/256x256/apps/$APPIMAGE_ID.png"
    cp "$NIGHTLY_ICON" "$APPDIR/$APPIMAGE_ID.png"
    ln -s "$APPIMAGE_ID.png" "$APPDIR/.DirIcon"
  else
    ln -s usr/bin/designcraft "$APPDIR/AppRun"
    cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
    cp "$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"
    ln -s "$APP_ID.png" "$APPDIR/.DirIcon"
  fi

  TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
  if [[ -z "$TOOL" ]]; then
    TOOL="$CARGO_TARGET_DIR/appimagetool-$APPIMAGETOOL_VERSION-$ARCH.AppImage"
    if [[ ! -x "$TOOL" ]]; then
      download="$TOOL.download"
      curl --proto '=https' -fsSL -o "$download" \
        "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$ARCH.AppImage"
      printf '%s  %s\n' "$APPIMAGETOOL_SHA256" "$download" | sha256sum -c -
      chmod 755 "$download"
      mv "$download" "$TOOL"
    fi
    printf '%s  %s\n' "$APPIMAGETOOL_SHA256" "$TOOL" | sha256sum -c -
  fi
  # Absolute, because appimagetool runs in $DIST below (CARGO_TARGET_DIR or APPIMAGETOOL may be
  # relative, e.g. target/agent-<name>).
  OUT="$(cd "$DIST" && pwd)/$BASENAME.AppImage"
  APPDIR="$(cd "$APPDIR" && pwd)"
  TOOL="$(cd "$(dirname "$TOOL")" && pwd)/$(basename "$TOOL")"
  # A .zsync left by an earlier run would hide a missing zsyncmake and describe another file.
  rm -f "$OUT.zsync"
  # Update information: AppImageUpdate, AppImageLauncher and the like read it from the file and
  # fetch only the blocks that changed in a newer release, through the .zsync published next to
  # each AppImage on GitHub Releases. `latest` is the newest published release that is not a
  # pre-release. A fork's builds point at its own releases through GITHUB_REPOSITORY.
  REPO="${GITHUB_REPOSITORY:-storytold/designcraft}"
  UPDATE_ARGS=()
  if [[ "$IS_NIGHTLY" = 0 ]]; then
    UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest|designcraft-*-linux-$ARCH.AppImage.zsync"
    UPDATE_ARGS=(-u "$UPDATE_INFO")
  else
    # Nightly releases are GitHub pre-releases (`latest` only selects stable releases), so point
    # zsync update metadata at pre-release tags rather than stable `latest`.
    NIGHTLY_ZSYNC_PATTERN="designcraft-*-nightly*-linux-$ARCH.AppImage.zsync"
    if [[ "$VERSION" != *-nightly* ]]; then
      NIGHTLY_ZSYNC_PATTERN="designcraft-*-linux-$ARCH.AppImage.zsync"
    fi
    UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest-pre|$NIGHTLY_ZSYNC_PATTERN"
    UPDATE_ARGS=(-u "$UPDATE_INFO")
  fi
  # Extract-and-run: works without FUSE (containers, CI). The output embeds the static runtime,
  # so users don't need libfuse2 either. With zsyncmake on the host (CI installs the zsync
  # package) appimagetool also writes the .zsync, into its working directory, hence the cd.
  (cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "${UPDATE_ARGS[@]}" "$APPDIR" "$OUT")
  echo "wrote $OUT"
  if [[ -s "$OUT.zsync" ]]; then
    echo "wrote $OUT.zsync"
  else
    warn "zsyncmake not found, so $OUT.zsync was not written; AppImage delta updates need it"
  fi
fi

if [[ -z "${CROSS_TARGET:-}" ]]; then
  "$STAGE/usr/bin/designcraft-cli" --version
elif [[ -n "${EMULATOR:-}" ]]; then
  "$EMULATOR" "$STAGE/usr/bin/designcraft-cli" --version
fi
echo "==> done"
ls -lh "$DIST"
