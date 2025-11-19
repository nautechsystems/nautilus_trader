# Bootstrap your dev environment

This quick checklist helps you prepare a fresh machine to build, lint, and test the project.

## Prerequisites
- Git, curl
- Python 3.11+ and pip
- Rust toolchain 1.91.0 (rustup)
- Node.js 20+ (for web)
- Docker (for integration tests)
- Optional (Hypersync-only): capnp (capnproto >= 0.5.2)

## Install
- Rust toolchain
  - macOS: `brew install rustup && rustup-init -y && rustup toolchain install 1.91.0`
  - Linux: `curl https://sh.rustup.rs -sSf | sh -s -- -y && rustup toolchain install 1.91.0`
- Python tools
  - `pip install uv ruff mypy`
- Node
  - macOS: `brew install node`
  - Linux: use your distro’s package manager or nvm
- Optional Hypersync
  - macOS: `brew install capnp`
  - Ubuntu/Debian: `sudo apt-get install -y capnproto`

## Sync Python deps
- Fast path (no package build): `make install-just-deps`
- Full install (debug): `make install-debug`

## Lint and build
- Rust: `make clippy` (default features)
- Python: `make ruff`
- Build: `make build-debug`

## Tests
- Rust: `make cargo-test` (no hypersync), or `WITH_HYPERSYNC=true make cargo-test-hypersync`
- Python: `make pytest`
- API-only tests: `make api-tests`

## Notes
- To run CI-equivalent checks locally: `make pre-flight`
- To lint everything including tests/benches: `make clippy-all`
- Hypersync builds require `capnp` to be installed.
