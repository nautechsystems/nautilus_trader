# Developer Guide

Welcome to the developer guide for NautilusTrader!

Here you'll find guidance on developing and extending NautilusTrader to meet your trading needs or to contribute improvements back to the project.

:::info
This guide is structured so that automated tooling can consume it alongside human readers.
:::

We believe in using the right tool for the job. The overall design philosophy is to fully utilize
the high-level power of Python, with its rich ecosystem of frameworks and libraries, whilst
leveraging Rust for performance-critical components and comprehensive type safety.

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
- [Docs Style](docs.md)
- [Release Notes](releases.md)
- [Adapters](adapters.md)
- [Benchmarking](benchmarking.md)
- [FFI Memory Contract](ffi.md)
