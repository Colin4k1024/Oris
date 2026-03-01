#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo-deny >/dev/null 2>&1; then
  echo "cargo-deny is required. Install it with: cargo install cargo-deny --locked"
  exit 1
fi

cargo deny check advisories licenses bans sources
