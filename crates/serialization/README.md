# nautilus-serialization

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-serialization)](https://docs.rs/nautilus-serialization/latest/nautilus_serialization/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-serialization.svg)](https://crates.io/crates/nautilus-serialization)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Data serialization and format conversion for [NautilusTrader](https://nautilustrader.io).

The `nautilus-serialization` crate provides data serialization capabilities for converting
trading data between different formats including Apache Arrow, Parquet, JSON, MessagePack, and SBE.
This enables efficient data storage, retrieval, and interoperability across different systems:

- **Apache Arrow integration**: Schema definitions and encoding/decoding for market data types.
- **Parquet file operations**: High-performance columnar storage for historical data analysis.
- **Record batch processing**: Efficient batch operations for time-series data.
- **Schema management**: Type-safe schema definitions with metadata preservation.
- **Cross-format conversion**: Data interchange between Arrow, Parquet, and native types.
- **SBE decode utilities**: Zero-copy cursor, shared decode errors, and generic var/group decoders for SBE parsers.

> [!WARNING]
>
> SBE schemas are not yet stable and may break between releases.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `arrow`: Enables Apache Arrow schema definitions and RecordBatch encoding/decoding.
- `arrow-display`: Enables display-friendly Arrow encoders for market data and requires `arrow`.
- `extension-module`: Builds as a Python extension module.
- `high-precision`: Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `sbe`: Enables generic SBE (Simple Binary Encoding) decode utilities.

## SBE depth layouts

SBE uses the legacy ten-level layout for ten bids and ten asks with canonical sides and zero order IDs.
Other snapshots, including padded levels, use a variable-length layout with up to 10,000 levels per side.
Its decoder applies native level validation and removes absent levels together with their counts.
Readers that support only the legacy layout cannot decode the variable-length template. SBE schemas are
version-unstable as noted above.

## Serialization format comparison

The supported serialization formats serve different market data use cases. Choose the format based on your use case:

Latencies are the median of three full sessions' mean estimates, each with 100 Criterion samples,
a 3-second warmup, and a 5-second measurement period.

| Format  | Serialize | Deserialize | Size      | Use case                                   |
| ------- | --------- | ----------- | --------- | ------------------------------------------ |
| JSON    | 344 ns    | 356 ns      | 185 bytes | Human-readable output, debugging, APIs.    |
| MsgPack | 413 ns    | 221 ns      | 135 bytes | Compact storage, network transmission.     |
| Arrow   | TBD       | TBD         | Columnar  | Batch processing, Parquet, IPC, analytics. |

Measured 2026-10-19 with the existing `serialization_comparison` QuoteTick fixture
on AMD Ryzen Threadripper 9980X, Linux 7.0.0-34-generic, and rustc 1.99.0
(b940084d7 2026-09-28). The `bench-lto` build uses standard precision and the `sbe` feature.
Processes run with the `performance` governor and per-process ASLR disabled;
SMT and CPU boost remain enabled. No builds, full test suites, or profiling run concurrently.

The largest `(maximum - minimum) / median` spread across these four cases is 2.3%.
JSON and MessagePack round trips preserve every fixture field.
Sizes are encoded byte lengths for that fixture. Reproduce the publication controls described in the
[benchmarking guide](../../docs/developer_guide/benchmarking.md#measure-criterion-for-publication).

Arrow is optimized for batch processing rather than individual messages.

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

## Benchmarking

`serialization_comparison` compares JSON and MessagePack for quotes, trades, and bars. With the `sbe`
feature, it also compares decoding batches of 100 and 1,000 trades using JSON, MessagePack, and SBE.
The SBE batches use benchmark-only count and length framing around the existing per-trade messages.

```bash
CARGO_BUILD_JOBS=8 cargo bench --profile bench-lto -p nautilus-serialization \
  --features sbe --bench serialization_comparison
```

Run the SBE cursor decode microbenchmarks with:

```bash
CARGO_BUILD_JOBS=8 cargo bench --profile bench-lto -p nautilus-serialization \
  --features sbe --bench sbe_decoding
```

For complete native messages and local MessageBus delivery, see the
[native codec benchmark](../common/benches/codecs/README.md).

## Documentation

See [the docs](https://docs.rs/nautilus-serialization) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
