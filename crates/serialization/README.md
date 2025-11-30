# nautilus-serialization

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-serialization)](https://docs.rs/nautilus-serialization/latest/nautilus-serialization/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-serialization.svg)](https://crates.io/crates/nautilus-serialization)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Data serialization and format conversion for [NautilusTrader](http://nautilustrader.io).

The `nautilus-serialization` crate provides comprehensive data serialization capabilities for converting
trading data between different formats including Apache Arrow, Parquet, and Cap'n Proto.
This enables efficient data storage, retrieval, and interoperability across different systems:

- **Apache Arrow integration**: Schema definitions and encoding/decoding for market data types.
- **Parquet file operations**: High-performance columnar storage for historical data analysis.
- **Record batch processing**: Efficient batch operations for time-series data.
- **Schema management**: Type-safe schema definitions with metadata preservation.
- **Cross-format conversion**: Seamless data interchange between Arrow, Parquet, and native types.
- **Cap'n Proto serialization**: Zero-copy, schema-based serialization for efficient data interchange.

## Platform

[NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
algorithmic trading platform, providing quantitative traders with the ability to backtest
portfolios of automated trading strategies on historical data with an event-driven engine,
and also deploy those same strategies live, with no code changes.

NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation,
depending on the intended use case, i.e. whether to provide Python bindings
for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
or as part of a Rust only build.

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module (used with `python`).
- `high-precision`: Enables [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) to use 128-bit value types.
- `capnp`: Enables [Cap'n Proto](https://capnproto.org/) serialization support.

### Building with Cap'n Proto support

To build with Cap'n Proto serialization enabled:

```bash
cargo build -p nautilus-serialization --features capnp
```

The Cap'n Proto compiler can be installed from [capnproto.org](https://capnproto.org/install.html).

## Cap'n Proto schemas

When the `capnp` feature is enabled, this crate provides zero-copy serialization using Cap'n Proto schemas.

### Schema location

Cap'n Proto schemas are bundled with the crate in `schemas/capnp/`:

- `common/identifiers.capnp` - Identifier types (TraderId, InstrumentId, etc.)
- `common/types.capnp` - Value types (Price, Quantity, Money, etc.)
- `common/enums.capnp` - Trading enumerations
- `commands/trading.capnp` - Trading commands
- `commands/data.capnp` - Data subscription/request commands
- `events/order.capnp` - Order events
- `events/position.capnp` - Position events
- `events/account.capnp` - Account events
- `data/market.capnp` - Market data types (quotes, trades, bars, order books)

### Generated modules

During build, schemas are compiled to Rust code and made available as:

- `nautilus_serialization::identifiers_capnp`
- `nautilus_serialization::types_capnp`
- `nautilus_serialization::enums_capnp`
- `nautilus_serialization::trading_capnp`
- `nautilus_serialization::data_capnp`
- `nautilus_serialization::order_capnp`
- `nautilus_serialization::position_capnp`
- `nautilus_serialization::account_capnp`
- `nautilus_serialization::market_capnp`

### Usage example

```rust
use nautilus_model::types::Price;
use nautilus_serialization::capnp::{ToCapnp, FromCapnp};

// Serialize a Price
let price = Price::from("123.45");
let bytes = nautilus_serialization::capnp::conversions::serialize_price(&price).unwrap();

// Deserialize back
let decoded = nautilus_serialization::capnp::conversions::deserialize_price(&bytes).unwrap();
assert_eq!(price, decoded);
```

See the `conversions` module for trait-based serialization patterns:

```rust
use nautilus_model::identifiers::InstrumentId;
use nautilus_serialization::capnp::{ToCapnp, FromCapnp, identifiers_capnp};

let instrument_id = InstrumentId::from("AAPL.NASDAQ");

// Using traits
let mut message = capnp::message::Builder::new_default();
let builder = message.init_root::<identifiers_capnp::instrument_id::Builder>();
instrument_id.to_capnp(builder);

// Serialize to bytes
let mut bytes = Vec::new();
capnp::serialize::write_message(&mut bytes, &message).unwrap();

// Deserialize
let reader = capnp::serialize::read_message(
    &mut &bytes[..],
    capnp::message::ReaderOptions::new()
).unwrap();
let root = reader.get_root::<identifiers_capnp::instrument_id::Reader>().unwrap();
let decoded = InstrumentId::from_capnp(root).unwrap();
```

### Contributing schemas

When adding or modifying schemas:

1. Edit schema files in the appropriate subdirectory under `schemas/capnp/`.
2. Use lowerCamelCase for field names to match Cap'n Proto conventions.
3. Generate a unique schema ID using: `capnp id`.
4. Implement `ToCapnp` and `FromCapnp` traits in `src/capnp/conversions.rs`.
5. Add integration tests in `tests/` to verify roundtrip serialization.

The build script (`build.rs`) automatically discovers and compiles all `.capnp` files during build.

## Serialization format comparison

This crate supports three serialization formats for market data types. Choose the format based on your use case:

| Format       | Serialize | Deserialize | Size      | Use case                                    |
|--------------|-----------|-------------|-----------|---------------------------------------------|
| Cap'n Proto  | ~267ns    | ~530ns      | 264 bytes | High-frequency data streams, IPC, caching.  |
| JSON         | ~332ns    | ~779ns      | 174 bytes | Human-readable output, debugging, APIs.     |
| MsgPack      | ~375ns    | ~634ns      | 134 bytes | Compact storage, network transmission.      |
| Arrow        | TBD       | TBD         | Columnar  | Batch processing, Parquet, IPC, analytics.  |

Performance numbers shown for `QuoteTick` serialization (measured on AMD Ryzen 9 7950X). Cap'n Proto provides the
fastest serialization and deserialization, while MsgPack offers the smallest size. Arrow is optimized for batch
processing rather than individual messages.

**Note:** Cap'n Proto performance can be further optimized through zero-copy techniques and direct buffer manipulation
for specialized use cases.

### Usage examples

#### JSON serialization

```rust
use nautilus_core::serialization::Serializable;
use nautilus_model::data::QuoteTick;

let quote = QuoteTick { /* ... */ };

// Serialize to JSON
let json_bytes = quote.to_json_bytes()?;

// Deserialize from JSON
let decoded = QuoteTick::from_json_bytes(&json_bytes)?;
```

#### MsgPack serialization

```rust
use nautilus_core::serialization::{ToMsgPack, FromMsgPack};
use nautilus_model::data::QuoteTick;

let quote = QuoteTick { /* ... */ };

// Serialize to MsgPack
let msgpack_bytes = quote.to_msgpack_bytes()?;

// Deserialize from MsgPack
let decoded = QuoteTick::from_msgpack_bytes(&msgpack_bytes)?;
```

#### Cap'n Proto serialization

```rust
use nautilus_model::data::QuoteTick;
use nautilus_serialization::capnp::{ToCapnp, FromCapnp, market_capnp};

let quote = QuoteTick { /* ... */ };

// Serialize to Cap'n Proto
let mut message = capnp::message::Builder::new_default();
let builder = message.init_root::<market_capnp::quote_tick::Builder>();
quote.to_capnp(builder);

let mut bytes = Vec::new();
capnp::serialize::write_message(&mut bytes, &message)?;

// Deserialize from Cap'n Proto
let reader = capnp::serialize::read_message(
    &mut &bytes[..],
    capnp::message::ReaderOptions::new()
)?;
let root = reader.get_root::<market_capnp::quote_tick::Reader>()?;
let decoded = QuoteTick::from_capnp(root)?;
```

## Benchmarking

Run benchmarks to compare serialization performance across formats:

```bash
# Compare all formats for QuoteTick
cargo bench -p nautilus-serialization --features capnp --bench serialization_comparison -- QuoteTick

# Compare all formats for TradeTick
cargo bench -p nautilus-serialization --features capnp --bench serialization_comparison -- TradeTick

# Compare all formats for Bar
cargo bench -p nautilus-serialization --features capnp --bench serialization_comparison -- Bar

# Run all Cap'n Proto benchmarks (including OrderBookDeltas with varying sizes)
cargo bench -p nautilus-serialization --features capnp --bench capnp_serialization

# Run all comparison benchmarks
cargo bench -p nautilus-serialization --features capnp --bench serialization_comparison
```

Benchmark results include serialization and deserialization times for each format.

## Documentation

See [the docs](https://docs.rs/nautilus-serialization) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

© 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
