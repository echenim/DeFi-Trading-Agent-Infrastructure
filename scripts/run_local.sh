#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

echo "==> Building workspace..."
cargo build --workspace

echo "==> Starting orchestrator in dry-run mode..."
RUST_LOG="${RUST_LOG:-info}" cargo run -p orchestrator -- \
    --config-dir ./configs \
    --dry-run \
    "$@"
