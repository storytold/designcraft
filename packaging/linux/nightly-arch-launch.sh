#!/bin/sh
set -eu

real_bin="$(dirname "$0")/.designcraft-nightly/designcraft"
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "-V" ]; then
  exec "$real_bin" "$@"
fi
if [ -z "${DESIGNCRAFT_CONFIG_DIR+x}" ]; then
  profile_base="${XDG_DATA_HOME:-${HOME:?HOME is required}/.local/share}"
  export DESIGNCRAFT_CONFIG_DIR="$profile_base/designcraft-nightly"
fi
exec "$real_bin" "$@"
