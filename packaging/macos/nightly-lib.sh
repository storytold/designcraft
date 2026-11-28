# shellcheck shell=bash
# Shared checks for the Apple-silicon Nightly scripts (nightly.sh, nightly-sign.sh, nightly-verify.sh).
# Stable packaging (package.sh, verify.sh) does not source this file. The text checks take tool output
# as arguments, so packaging/macos/nightly_sign_tests.py runs them without macOS.

MAC_NIGHTLY_APP='DesignCraft Nightly.app'
MAC_NIGHTLY_ID='ai.storyteller.designcraft.nightly'
# The release environment's names, as stable release.yml passes them (packaging/macos/import-cert.sh).
MAC_NIGHTLY_SECRETS=(APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD KEYCHAIN_PASSWORD APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID)

mac_nightly_die() { printf 'error: %s\n' "$*" >&2; exit 2; }

# The exact prepared version from the feed asset name, or failure.
mac_nightly_version_from_asset() {
  local name="$1" version
  [[ "$name" =~ ^designcraft-(.+)-macos-aarch64-nightly\.zip$ ]] || return 1
  version="${BASH_REMATCH[1]}"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+-nightly\.[0-9]{8}\.[0-9]+\.[0-9]+\.g[0-9a-f]{12}$ ]] || return 1
  printf '%s\n' "$version"
}

# Every signing and notarization value is set, and the team is one Apple team ID. No ad-hoc fallback.
mac_nightly_require_secrets() {
  local name
  for name in "${MAC_NIGHTLY_SECRETS[@]}"; do
    [[ -n "${!name:-}" ]] || mac_nightly_die "$name is not set: Nightly signing and notarization are required, with no ad-hoc fallback"
  done
  [[ "$APPLE_TEAM_ID" =~ ^[A-Z0-9]{10}$ ]] || mac_nightly_die 'APPLE_TEAM_ID is not an Apple team ID'
  if [[ -n "${NIGHTLY_APPLE_TEAM_ID:-}" && "$NIGHTLY_APPLE_TEAM_ID" != "$APPLE_TEAM_ID" ]]; then
    mac_nightly_die 'the signing team differs from the reviewed Nightly team (NIGHTLY_APPLE_TEAM_ID)'
  fi
}

# One line of `security find-identity -v -p codesigning`: the identity is a Developer ID Application of the team.
mac_nightly_identity_is_team() {
  local line="$1" identity="$2" team="$3"
  [[ "$line" == *" $identity "* && "$line" == *'"Developer ID Application: '*" ($team)\""* ]]
}

# `codesign -d --verbose=2` of the signed bundle: the Nightly identifier, a Developer ID Application of the
# team, a secure timestamp and the hardened runtime (the checks of verify.sh, made strict).
mac_nightly_check_signature() {
  local details="$1" team="$2" ok=0
  grep -Fqx "Identifier=$MAC_NIGHTLY_ID" <<<"$details" || { echo "error: the bundle is not signed as $MAC_NIGHTLY_ID" >&2; ok=1; }
  grep -q '^Authority=Developer ID Application:' <<<"$details" || { echo 'error: not signed with a Developer ID Application identity' >&2; ok=1; }
  grep -qx "TeamIdentifier=$team" <<<"$details" || { echo 'error: signed by a different team' >&2; ok=1; }
  grep -q '^Timestamp=' <<<"$details" || { echo 'error: the signature has no secure timestamp' >&2; ok=1; }
  grep -Eq '^CodeDirectory .*flags=0x[0-9a-f]+\([^)]*runtime' <<<"$details" || { echo 'error: not signed with the hardened runtime' >&2; ok=1; }
  return "$ok"
}

# `spctl --assess --type execute -vv`: Gatekeeper accepts the bundle as notarized.
mac_nightly_check_assessment() {
  grep -q 'accepted' <<<"$1" && grep -qx 'source=Notarized Developer ID' <<<"$1"
}

# Limits on an archive handed to `ditto -x`, set well above a normal Nightly bundle (the stable macOS
# app is about 280 MB packed) and low enough that a hostile archive cannot fill the runner's disk.
MAC_NIGHTLY_ZIP_MAX_ARCHIVE=$((1024 * 1024 * 1024))
MAC_NIGHTLY_ZIP_MAX_EXPANDED=$((2 * 1024 * 1024 * 1024))
MAC_NIGHTLY_ZIP_MAX_ENTRIES=20000

# Before anything is unpacked: every ZIP entry is a regular file or directory inside the one Nightly
# bundle, with no absolute path, `..`, backslash or empty component, and no link or special file.
# The archive is also bounded: its size, entry count and compressed bytes (so entries cannot overlap),
# and the bytes each entry really expands to, which are counted by inflating it here, not taken from
# the header that `ditto` would trust. Only stored and deflated entries are accepted.
mac_nightly_check_zip_entries() {
  python3 -I - "$1" "$MAC_NIGHTLY_APP" "$MAC_NIGHTLY_ZIP_MAX_ARCHIVE" "$MAC_NIGHTLY_ZIP_MAX_EXPANDED" "$MAC_NIGHTLY_ZIP_MAX_ENTRIES" <<'PY' \
    || mac_nightly_die 'the archive has an entry outside the bundle, a link, a special file or more data than a Nightly bundle holds'
import os, stat, struct, sys, zipfile, zlib
archive, app = sys.argv[1:3]
max_archive, max_expanded, max_entries = map(int, sys.argv[3:6])
if os.path.getsize(archive) > max_archive:
    sys.exit("the archive is larger than a Nightly bundle")
CHUNK = 1 << 20
with zipfile.ZipFile(archive) as z, open(archive, "rb") as raw:
    infos = z.infolist()
    if len(infos) > max_entries:
        sys.exit(f"too many entries: {len(infos)}")
    if sum(info.compress_size for info in infos) > os.path.getsize(archive):
        sys.exit("entries share compressed data")
    expanded = 0
    for info in infos:
        name = info.filename
        parts = name.rstrip("/").split("/")
        mode = info.external_attr >> 16
        if (name.startswith("/") or "\\" in name or "\0" in name or parts[0] != app
                or any(part in ("", ".", "..") for part in parts)
                or stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR)):
            sys.exit(f"refused entry: {name!r}")
        if info.is_dir():
            if info.file_size or info.compress_size:
                sys.exit(f"directory with data: {name!r}")
            continue
        if not info.file_size and not info.compress_size:
            continue
        if info.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
            sys.exit(f"unsupported compression in {name!r}")
        if info.file_size > max_expanded - expanded:
            sys.exit("the archive expands to more than a Nightly bundle holds")
        # Inflate the entry's own compressed bytes and count what comes out.
        raw.seek(info.header_offset)
        header = raw.read(30)
        if len(header) != 30 or header[:4] != b"PK\x03\x04":
            sys.exit(f"bad local header: {name!r}")
        name_len, extra_len = struct.unpack("<HH", header[26:30])
        raw.seek(info.header_offset + 30 + name_len + extra_len)
        left = info.compress_size
        if info.compress_type == zipfile.ZIP_STORED:
            if info.compress_size != info.file_size:
                sys.exit(f"stored entry with a wrong size: {name!r}")
            total = info.file_size
        else:
            inflater = zlib.decompressobj(-15)
            total = 0
            while left > 0 and not inflater.eof:
                data = raw.read(min(CHUNK, left))
                if not data:
                    sys.exit(f"truncated entry: {name!r}")
                left -= len(data)
                pending = data
                while pending:
                    out = inflater.decompress(pending, CHUNK)
                    total += len(out)
                    if total > info.file_size or total > max_expanded - expanded:
                        sys.exit(f"entry expands past its declared size: {name!r}")
                    pending = inflater.unconsumed_tail
            if not inflater.eof or left or inflater.unused_data or total != info.file_size:
                sys.exit(f"entry size does not match its data: {name!r}")
        expanded += total
PY
}

# An unpacked payload directory holds exactly the GUI-only Nightly bundle: one thin arm64 DesignCraft
# executable, the Nightly identity and the exact version (no numeric fallback), no CLI, no links or special
# files, no document or URL types.
mac_nightly_check_bundle() {
  local payload="$1" version="$2" app entries plist key value
  app="$payload/$MAC_NIGHTLY_APP"
  entries="$(ls -A "$payload")"
  [[ "$entries" == "$MAC_NIGHTLY_APP" && -d "$app" && ! -L "$app" ]] || mac_nightly_die "the archive must hold exactly $MAC_NIGHTLY_APP"
  [[ -z "$(find "$payload" ! -type f ! -type d -print -quit)" ]] || mac_nightly_die 'the bundle contains links or special files'
  [[ "$(ls -A "$app/Contents/MacOS")" == DesignCraft ]] || mac_nightly_die 'Contents/MacOS must hold only the DesignCraft GUI (no CLI)'
  [[ -f "$app/Contents/MacOS/DesignCraft" && -x "$app/Contents/MacOS/DesignCraft" ]] || mac_nightly_die 'missing executable GUI'
  [[ "$(lipo -archs "$app/Contents/MacOS/DesignCraft")" == arm64 ]] || mac_nightly_die 'the GUI must be thin arm64'
  plist="$app/Contents/Info.plist"
  for key in CFBundleIdentifier CFBundleExecutable DesignCraftVersion; do
    value="$(plutil -extract "$key" raw -o - "$plist")" || mac_nightly_die "Info.plist has no $key"
    case "$key" in
      CFBundleIdentifier) [[ "$value" == "$MAC_NIGHTLY_ID" ]] ;;
      CFBundleExecutable) [[ "$value" == DesignCraft ]] ;;
      DesignCraftVersion) [[ "$value" == "$version" ]] ;;
    esac || mac_nightly_die "Info.plist $key is not the prepared Nightly identity"
  done
  for key in CFBundleDocumentTypes CFBundleURLTypes UTExportedTypeDeclarations UTImportedTypeDeclarations; do
    if plutil -extract "$key" xml1 -o - "$plist" >/dev/null 2>&1; then mac_nightly_die "Nightly declares $key"; fi
  done
}

# `stapler validate` asks Apple's CloudKit service, which sometimes drops the connection (error 68)
# though the ticket is there (verify.sh).
mac_nightly_validate_staple() {
  local target="$1" attempt
  for attempt in 1 2 3 4 5; do
    xcrun stapler validate "$target" && return 0
    [[ "$attempt" = 5 ]] || sleep $((attempt * 20))
  done
  return 1
}
