#!/usr/bin/env bash
# Run max-explorer from the build tree with the CEF runtime it was built against.
# Extra args go to the browser; MAX_EXPLORER_SMOKE=3 opens three test tabs.
set -euo pipefail
cd "$(dirname "$0")/.."

CEF_DIR=$(find target/debug/build -maxdepth 3 -type d -name 'cef_linux_*' | head -1)
if [ -z "${CEF_DIR:-}" ]; then
  echo "CEF runtime not found — run: cargo build -p max-explorer-app" >&2
  exit 1
fi

BIN=${BIN:-target/debug/max-explorer}
export LD_LIBRARY_PATH="$PWD/$CEF_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export MAX_EXPLORER_CEF_DIR="$PWD/$CEF_DIR"
exec "$BIN" "$@"
