# nautilus-pyo3

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Python bindings for [NautilusTrader](https://nautilustrader.io).

The `nautilus-pyo3` crate provides all [PyO3](https://pyo3.rs) Python bindings for the
main `nautilus_trader` Python package, built via [maturin](https://github.com/PyO3/maturin).

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `adapters` (default): Enables all adapter Python bindings except Betfair, CoinAPI, and blockchain.
- `arrow`: Enables Apache Arrow support in dependent crates.
- `betfair`: Enables the Betfair adapter and its Python bindings.
- `coinapi`: Enables the CoinAPI adapter and its Python bindings.
- `defi`: Enables DeFi (Decentralized Finance) support, including blockchain adapters.
- `extension-module`: Builds as a Python extension module and is automatically enabled by `maturin`.
- `high-precision`: Uses 128-bit value types throughout the workspace.
- `hypersync`: Enables [`hypersync-client`](https://crates.io/crates/hypersync-client) support for the blockchain adapter.
- `mimalloc`: Sets [mimalloc](https://crates.io/crates/mimalloc) as Rust's global allocator.
- `postgres`: Enables PostgreSQL (sqlx) back-ends in dependent crates.
- `redis`: Enables Redis based infrastructure in dependent crates.
- `tracing-bridge`: Enables the `tracing` subscriber bridge for log integration.

## Embedding Python

Use git or path dependencies for `nautilus-pyo3`, which is not published to crates.io.
All NautilusTrader dependencies must use the same source and revision to share types.
Enable `nautilus-live`'s `python` feature and leave `extension-module` disabled when embedding.
For core bindings without the standard adapters, set `default-features = false` on `nautilus-pyo3`.

In the host workspace's root `Cargo.toml`, add a `[patch.crates-io]` entry for `pyo3-stub-gen` with
`path` pointing to this checkout's [patched crate](../../patches/pyo3-stub-gen).
Cargo does not inherit patches from dependency workspaces.

Add the directory containing the matching `nautilus_trader` Python package to Python's import path
(the `python/` directory in the same checkout). Make your component and configuration modules importable too.

1. Call `register_embedded_module()` from `nautilus-pyo3` before `pyo3::Python::initialize()`.
1. After initialization, import `_libnautilus` with `py.import("_libnautilus")` inside
   `pyo3::Python::attach`, before importing `nautilus_trader` or any component module. This registers
   the `nautilus_trader._libnautilus` alias so imports use the embedded bindings.
1. Before starting the node, register the components from their importable configurations:
   - Actors: `nautilus_system::trader::Trader::add_actor_from_importable_config` with
     `nautilus_common::actor::data_actor::ImportableActorConfig`, through
     `node.kernel().trader().borrow_mut()`.
   - Execution algorithms: `nautilus_live::node::LiveNode::add_python_exec_algorithm_from_config` with
     `nautilus_trading::ImportableExecutionAlgorithmConfig`.
   - Strategies: `nautilus_live::node::LiveNode::add_python_strategy_from_config` with
     `nautilus_trading::ImportableStrategyConfig`.

Use `module.path:ClassName` for `actor_path`, `exec_algorithm_path`, `strategy_path`, and `config_path`.
To construct a component without a configuration argument, leave both `config_path` and the `config`
dictionary empty.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
