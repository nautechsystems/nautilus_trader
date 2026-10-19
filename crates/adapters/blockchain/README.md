# nautilus-blockchain

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-blockchain)](https://docs.rs/nautilus-blockchain/latest/nautilus_blockchain/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-blockchain.svg)](https://crates.io/crates/nautilus-blockchain)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) blockchain adapter for DeFi data ingestion.

The `nautilus-blockchain` crate provides a high-performance, universal, extensible adapter for ingesting
DeFi data from decentralized exchanges (DEXs), liquidity pools, and on-chain events. It enables you to
power analytics pipelines and trading strategies with real-time and historical on-chain data.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `extension-module`: Builds as a Python extension module.
- `hypersync`: Enables the [`hypersync-client`](https://crates.io/crates/hypersync-client) integration.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `turmoil`: Enables deterministic network simulation testing with [turmoil](https://crates.io/crates/turmoil).

## Examples

Run the commands below from the repository root.

### Watch live blocks and pool events

The [data tester](examples/node_data_tester.rs) subscribes to blocks, swaps, and liquidity updates
for a Uniswap V3 pool on Arbitrum. Edit `CHAIN`, `DEX_TYPE`, and `POOL_INSTRUMENT_ID` in the example
to select another chain and pool.

Set the required environment variables, or add them to a Git-ignored `.env` file in the repository root:

```dotenv
ENVIO_API_TOKEN=YOUR_ENVIO_API_TOKEN
RPC_HTTP_URL=https://your-arbitrum-rpc.example
RPC_WSS_URL=wss://your-arbitrum-rpc.example
```

The example reads all three variables at startup. It uses HyperSync for live data by default.
To use WSS RPC live streams, set `use_hypersync_for_live_data(false)` in the example. Both RPC
endpoints must match the selected chain.

Prepare the Postgres cache and target pool for the selected chain and DEX using the
[pool sync runbook](../../../docs/integrations/blockchain.md#runbook-validate-a-live-pool-sync).
The example uses the default local Postgres connection; edit `postgres_cache_database_config`
if your database settings differ.

```bash
cargo run -p nautilus-blockchain --example blockchain-data-tester --features hypersync
```

### Discover Uniswap V3 pools and tokens on Ethereum

The CLI reads pool-creation events from HyperSync, fetches ERC-20 metadata through HTTP RPC,
and stores pools and tokens in Postgres. Set `ENVIO_API_TOKEN` and an Ethereum `RPC_HTTP_URL`,
and initialize the database as described in the
[integration guide](../../../docs/integrations/blockchain.md#local-services).

```bash
cargo run -p nautilus-cli --features defi --bin nautilus -- \
    blockchain sync-dex --chain ethereum --dex UniswapV3
```

## Testing

The crate includes both standard integration tests and deterministic network simulation tests using turmoil.

To run standard tests:

```bash
cargo nextest run -p nautilus-blockchain
```

To run turmoil network simulation tests:

```bash
cargo nextest run -p nautilus-blockchain --features turmoil
```

The turmoil tests simulate various network conditions (reconnections, partitions, etc.) in a deterministic way, allowing reliable testing of network failure scenarios without flakiness.

## Documentation

See [the docs](https://docs.rs/nautilus-blockchain) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
