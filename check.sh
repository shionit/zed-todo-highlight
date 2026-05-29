#!/usr/bin/env bash
# Single command to verify both crates are healthy.
# Run this before marking any task complete.
set -euo pipefail

echo "==> tests..."
cargo test --workspace

echo "==> coverage (>= 80% lines required)..."
cargo llvm-cov --package todo-highlight-lsp --fail-under-lines 80

echo "==> clippy..."
cargo clippy -p todo-highlight-lsp -- -D warnings

echo "==> extension (wasm32-wasip1)..."
cargo check -p todo-highlight-extension --target wasm32-wasip1

echo "All checks passed."
