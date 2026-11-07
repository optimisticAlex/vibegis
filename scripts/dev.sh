#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if ! command -v trunk >/dev/null && [ -f "$HOME/.cargo/env" ]; then
  source "$HOME/.cargo/env"
fi
python3 scripts/serve.py --port 8788 &
data_pid=$!
trap 'kill "$data_pid" 2>/dev/null || true' EXIT INT TERM
trunk serve --release
