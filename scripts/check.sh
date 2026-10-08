#!/usr/bin/env bash
# Definition of green: formatting, lints and the full test suite.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> cargo fmt --check"
cargo fmt --check

echo "==> cargo clippy --all-targets --all-features -- -D warnings"
cargo clippy --all-targets --all-features -- -D warnings

echo "==> cargo test --workspace"
cargo test --workspace

echo "==> all checks passed"
