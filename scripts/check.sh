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

# Each platform must also build on its own, or the Cargo features are a lie.
echo "==> cargo check --no-default-features --features linkedin"
cargo check --no-default-features --features linkedin --all-targets

echo "==> cargo check --no-default-features --features meta"
cargo check --no-default-features --features meta --all-targets

echo "✅ All checks passed — safe to push."
