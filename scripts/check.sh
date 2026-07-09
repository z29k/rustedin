#!/bin/sh
# Run the exact checks CI runs, locally, before pushing.
# Usage:  ./scripts/check.sh
# Also invoked automatically by the pre-push hook (see CONTRIBUTING.md).
set -e

echo "==> cargo fmt --all --check"
cargo fmt --all --check

echo "==> cargo clippy --all-targets --all-features -- -D warnings"
cargo clippy --all-targets --all-features -- -D warnings

echo "==> cargo build --release"
cargo build --release

echo "==> cargo test"
cargo test

echo "✅ All checks passed — safe to push."
