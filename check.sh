#!/usr/bin/env bash
# Single command to verify both crates are healthy.
# Run this before marking any task complete.
set -euo pipefail

echo "==> tests..."
cargo test --workspace

echo "==> coverage (>= 80% lines required)..."
cargo llvm-cov --package todo-highlighter-lsp --fail-under-lines 80

echo "==> clippy..."
cargo clippy -p todo-highlighter-lsp -- -D warnings

echo "==> extension (wasm32-wasip1)..."
cargo check -p todo-highlighter-extension --target wasm32-wasip1

echo "All checks passed."
