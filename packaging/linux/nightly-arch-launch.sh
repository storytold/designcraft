#!/bin/sh
# Start the compiled nightly-v1 flavor from the Arch package. The profile is the Launcher's
# protocol (the app refuses a DESIGNCRAFT_CONFIG_DIR override), so this launcher passes the same
# pair the Launcher will.
set -eu
here="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"
real_bin="$here/.designcraft-nightly/designcraft"
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
