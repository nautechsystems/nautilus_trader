# nautilus-lighter

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](http://nautilustrader.io) adapter for the **Lighter Exchange** perpetuals venue.

This crate follows the Rust-first adapter blueprint used across Nautilus and will house the HTTP
and WebSocket clients, signing/auth helpers, and PyO3 bindings consumed by the Python surface.

> Status: scaffolding (PR0) — structure, constants, and bindings are in place; networked
> functionality will land in subsequent PRs per the implementation plan.

## Feature flags

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module (used with `python`).

## License

The source code for NautilusTrader is available on GitHub under the
[GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard
[Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---
