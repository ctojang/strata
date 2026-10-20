#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all --check
cargo check --all-targets --all-features --workspace
cargo clippy --all-targets --all-features --workspace -- -D warnings
# Display-dependent tests are exercised by test-headless.py and e2e.sh.
env -u DISPLAY -u WAYLAND_DISPLAY GDK_BACKEND=x11 STRATA_REQUIRE_DEVICE_TESTS=1 cargo test --all-targets --all-features --workspace

if command -v cargo-deny >/dev/null 2>&1; then
  cargo deny check
else
  echo "note: cargo-deny is not installed; skipping dependency policy checks" >&2
fi

if command -v typos >/dev/null 2>&1; then
  typos
else
  echo "note: typos is not installed; skipping spell checks" >&2
fi
