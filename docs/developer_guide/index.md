# Developer Guide

Guidance on developing and extending NautilusTrader, or contributing back to the project.

NautilusTrader uses a **Rust core with Python bindings** architecture:

- **Rust** handles networking, data parsing, order matching, and other performance-critical operations.
- **Python** provides the user-facing API for strategy development, configuration, and system integration.
- **PyO3** bridges the two, exposing Rust functionality to Python with minimal overhead.

This approach combines Python's simplicity and ecosystem with Rust's performance and memory safety.

## Contents

- [Environment Setup](environment_setup.md)
- [Coding Standards](coding_standards.md)
- [Rust](rust.md)
- [Python](python.md)
- [Testing](testing.md)
- [Test Datasets](test_datasets.md)
- [Docs Style](docs.md)
- [Release Notes](releases.md)
- [Adapters](adapters.md)
- [Benchmarking](benchmarking.md)
- [FFI Memory Contract](ffi.md)
