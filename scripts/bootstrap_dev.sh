#!/usr/bin/env bash
set -euo pipefail

ok() { printf "\033[0;32m✓\033[0m %s\n" "$1"; }
warn() { printf "\033[0;33m!\033[0m %s\n" "$1"; }
err() { printf "\033[0;31m✗\033[0m %s\n" "$1"; }

need() { command -v "$1" >/dev/null 2>&1; }

# Rust
if need rustc; then
  v=$(rustc --version)
  ok "rustc present: $v"
else
  warn "rustc not found. Install via rustup (https://rustup.rs)"
fi

# Python
if need python3; then
  ok "python: $(python3 --version)"
else
  warn "python3 not found"
fi

# uv
if need uv; then
  ok "uv present: $(uv --version)"
else
  warn "uv not found (pip install uv)"
fi

# Node
if need node; then
  ok "node: $(node --version)"
else
  warn "node not found"
fi

# Docker
if need docker; then
  ok "docker: $(docker --version | head -n1)"
else
  warn "docker not found (needed for integration tests)"
fi

# Optional: capnp
if need capnp; then
  ok "capnp: $(capnp --version)"
else
  warn "capnp not found (only required for hypersync builds)"
fi

printf "\nNext steps:\n"
printf "  1) Sync deps: make install-just-deps\n"
printf "  2) Lint/check: make clippy && make ruff\n"
printf "  3) Tests: make cargo-test && make pytest\n"
printf "\nHypersync build: install capnp then run WITH_HYPERSYNC=true make pre-flight\n"