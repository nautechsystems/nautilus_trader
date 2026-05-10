# nautilus-blockchain

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-blockchain)](https://docs.rs/nautilus-blockchain/latest/nautilus-blockchain/)
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

- `hypersync`: Enables the [HyperSync](https://envio.dev/#hypersync) client integration.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module.
- `turmoil`: Enables deterministic network simulation testing with [turmoil](https://github.com/tokio-rs/turmoil).

## Scripts

You can run some example scripts and provide target RPC environment variables. These examples demonstrate how to connect to blockchain nodes and subscribe to various events.

You can configure the required environment variables in two ways:

1. **Using a `.env` file in the project root:**
   Create a file named `.env` in the project root directory with the following content:

   ```
   CHAIN=Ethereum
   RPC_WSS_URL=wss://mainnet.infura.io/ws/v3/YOUR_INFURA_API_KEY
   RPC_HTTP_URL=https://mainnet.infura.io/v3/YOUR_INFURA_API_KEY
   ```

2. **Providing variables directly in the command line:**

   ```
   CHAIN=Ethereum RPC_WSS_URL=wss://your-node-endpoint cargo run --bin live_blocks_rpc
   ```

### Watch live blocks

The scripts will connect to the specified blockchain and log information about each new block received for both the RPC version and only Hypersync.

```
cargo run --bin live_blocks_rpc --features hypersync
```

```
cargo run --bin live_blocks_hypersync --features hypersync
```

For RPC example, the output should be:

```
Running `target/debug/live_blocks_rpc`
2025-04-25T14:54:41.394620000Z [INFO] TRADER-001.nautilus_blockchain::rpc::core: Subscribing to new blocks on chain Ethereum
2025-04-25T14:54:48.951608000Z [INFO] TRADER-001.nautilus_blockchain::data: Block(chain=Ethereum, number=22346765, timestamp=2025-04-25T14:54:47+00:00, hash=0x18a3c9f1e3eec06b45edc1f632565e5c23089dc4ad0892b00fda9e4ffcc9bf91)
2025-04-25T14:55:00.646992000Z [INFO] TRADER-001.nautilus_blockchain::data: Block(chain=Ethereum, number=22346766, timestamp=2025-04-25T14:54:59+00:00, hash=0x110436e41463daeacd1501fe53d38c310573abc136672a12054e1f33797ffeb9)
2025-04-25T14:55:14.369337000Z [INFO] TRADER-001.nautilus_blockchain::data: Block(chain=Ethereum, number=22346767, timestamp=2025-04-25T14:55:11+00:00, hash=0x54e7dbcfc14c058e22c70cbacabe4872e84bd6d3b976258f0d364ae99226b314)
2025-04-25T14:55:38.314022000Z [INFO] TRADER-001.live_blocks: Shutdown signal received, shutting down...

```

### Sync dex, tokens and pool for Uniswap V3 on Ethereum

This script demonstrates how to use the blockchain data client to discover and cache Uniswap V3 pools and their associated tokens. It queries the Ethereum blockchain for pool creation events emitted by the Uniswap V3 factory contract, retrieves token metadata (name, symbol, decimals) for each token in the pools via smart contract calls, and stores everything in a local Postgres database.

```
cargo run --bin sync_tokens_pools --features hypersync
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
