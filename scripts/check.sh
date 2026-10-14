#!/usr/bin/env bash
# Definition of green: formatting, lints and the full test suite.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> cargo fmt --check"
cargo fmt --check

echo "==> cargo clippy --all-targets --all-features --offline -- -D warnings"
cargo clippy --all-targets --all-features --offline -- -D warnings

echo "==> cargo test --workspace --offline"
cargo test --workspace --offline

echo "==> agentos evaluate"
cargo run --release -p agentos-cli --offline -- evaluate

echo "==> agentos evaluate e2e"
cargo run --release -p agentos-cli --offline -- evaluate e2e

echo "==> all checks passed"
