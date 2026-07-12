# Benchmarks

Performance benchmarks across the workspace. All benchmarks use
[Criterion](https://docs.rs/criterion) unless noted as IAI (instruction-count).

Run all CI benchmarks:

```bash
make cargo-ci-benches
```

Run benchmarks for a specific crate:

```bash
cargo bench -p nautilus-core --benches
```

## Core crates

### nautilus-core

| File | What it measures |
|------|------------------|
| `concurrent_map.rs` | `AtomicMap` lookup strategies (get-clone vs load-once guard). |
| `correctness.rs` | Precondition check overhead. |
| `datetime.rs` | Datetime formatting for log lines. |
| `decimal_deserialization.rs` | Decimal deserialization approaches. |
| `hash_map.rs` | HashMap implementation comparison. |
| `hex.rs` | Hex encode/decode at various payload sizes (32–256 bytes). |
| `identifier_comparison.rs` | `StackStr` vs `Ustr` creation and lookup in 1000-element sets. |
| `stack_str.rs` | `StackStr` construction, equality, hashing, and display. |
| `stack_str_iai.rs` | `StackStr` operations (IAI instruction count). |
| `time.rs` | `duration_since_unix_epoch` and `nanos_since_unix_epoch`. |
| `to_snake_case.rs` | PascalCase/camelCase to snake_case conversion. |
| `urlencoding.rs` | URL encoding for various payloads (timestamps, JSON, UTF-8). |
| `uuid.rs` | `UUID4` creation, display, parsing, and serialization. |

### nautilus-model

| File | What it measures |
|------|------------------|
| `black_scholes_criterion.rs` | Implied volatility and Greeks computation (f32). |
| `book_iai.rs` | Order book operations (IAI instruction count). |
| `expressions_criterion.rs` | Expression evaluation (simple avg, weighted, conditional). |
| `f64_vs_decimal_to_price_quantity.rs` | f64 vs `Decimal` for Price/Quantity construction. |
| `fixed_precision_criterion.rs` | Fixed-precision arithmetic (Criterion). |
| `fixed_precision_iai.rs` | Fixed-precision arithmetic (IAI instruction count). |
| `greeks_criterion.rs` | Options Greeks calculation. |
| `money_criterion.rs` | `Money` type operations. |
| `price_criterion.rs` | `Price` type operations. |
| `quantity_criterion.rs` | `Quantity` type operations. |

### nautilus-common

| File | What it measures |
|------|------------------|
| `cache/orders.rs` | Order cache operations. |
| `cache/query_sets.rs` | Cache query set performance. |
| `client_order_id.rs` | `ClientOrderId` generation (same-second, cross-second, with/without hyphens). |
| `logging.rs` | Log line formatting (plain, colored, cached, with fields). |
| `matching.rs` | Topic pattern matching (Criterion). |
| `matching_iai.rs` | Topic pattern matching (IAI instruction count). |
| `msgbus.rs` | MessageBus dispatch (Any-based vs typed handlers and routers). |
| `mstr.rs` | `MStr` topic/pattern creation and lookup. |
| `order_list_id.rs` | `OrderListId` generation. |
| `position_id.rs` | `PositionId` generation (single vs rotating strategies). |
| `throttler.rs` | Throttler admit/reject/batch/timer overhead. |

### nautilus-data

| File | What it measures |
|------|------------------|
| `engine.rs` | DataEngine `process_data` throughput for trade ticks. |

### nautilus-execution

| File | What it measures |
|------|------------------|
| `matching_core.rs` | Matching engine core: limit match, stop match, touch trigger, fill check. |

### nautilus-live

| File | What it measures |
|------|------------------|
| `runner.rs` | Channel send/recv, runner setup, stop signal, and quote tick creation overhead. |

### nautilus-backtest

| File | What it measures |
|------|------------------|
| `engine.rs` | Backtest engine full run throughput. |

### nautilus-network

| File | What it measures |
|------|------------------|
| `http_response.rs` | HTTP response parsing. |
| `ratelimiter.rs` | Rate limiter check/admit overhead. |
| `test_client.rs` | HTTP client benchmark harness. |
| `test_server.rs` | Test server for network benchmarks. |
| `websocket_latency.rs` | WebSocket message round-trip latency. |
| `websocket_transport.rs` | WebSocket transport throughput. |

### nautilus-serialization

| File | What it measures |
|------|------------------|
| `capnp_serialization.rs` | Cap'n Proto serialization/deserialization. |
| `market_data_capnp_vs_sbe.rs` | Cap'n Proto vs SBE for market data encoding. |
| `sbe_decoding.rs` | SBE (Simple Binary Encoding) decoding. |
| `serialization_comparison.rs` | Cross-format serialization comparison. |

### nautilus-persistence

| File | What it measures |
|------|------------------|
| `persistence.rs` | Data persistence read/write throughput. |

### nautilus-portfolio

| File | What it measures |
|------|------------------|
| `portfolio.rs` | Portfolio update and query performance. |

### nautilus-event-store

| File | What it measures |
|------|------------------|
| `codec.rs` | Event store codec encode/decode. |
| `hash.rs` | Event hashing. |

## Adapter crates

Each adapter has a consistent benchmark structure measuring the hot path from
raw wire data to Nautilus domain types.

### Common adapter benchmark pattern

| File | What it measures |
|------|------------------|
| `micros.rs` | Raw JSON/binary → intermediate struct deserialization. |
| `data.rs` | Intermediate struct → Nautilus data types (trades, book deltas, depth10, quotes). |
| `exec.rs` | Order request construction (submit market/limit/stop, cancel). |
| `signing.rs` | Cryptographic signing overhead (L1 actions, order hashing). |
| `websocket.rs` | Full WS message pipeline: classify → deserialize → parse → emit. |

### Adapters with benchmarks

| Adapter | Benchmark files |
|---------|-----------------|
| Architect AX | `deserialization.rs`, `parsing.rs` |
| Binance | `encoder.rs` (SBE encode/decode) |
| Bybit | `websocket.rs` |
| Databento | `clients.rs`, `data.rs`, `micros.rs` |
| Deribit | `websocket.rs` |
| Derive | `data.rs`, `exec.rs`, `micros.rs`, `signing.rs` |
| Hyperliquid | `data.rs`, `exec.rs`, `micros.rs`, `signing.rs` |
| Kraken | `websocket.rs` |
| Lighter | `data.rs`, `exec.rs`, `micros.rs`, `signing_*.rs` (curve, field, poseidon2, sign/verify) |
| OKX | `data.rs`, `exec.rs`, `micros.rs`, `signing.rs` |
| Polymarket | `data.rs`, `exec.rs`, `micros.rs`, `signing.rs` |

### Lighter signing benchmarks

Lighter uses a custom zero-knowledge signing scheme, so it has detailed
cryptographic benchmarks:

| File | What it measures |
|------|------------------|
| `signing_field.rs` | Finite field arithmetic (mul, square, invert). |
| `signing_field_iai.rs` | Field arithmetic (IAI instruction count). |
| `signing_curve.rs` | Elliptic curve point operations (add, double, scalar mul). |
| `signing_poseidon2.rs` | Poseidon2 hash permutation and hashing. |
| `signing_sign_verify.rs` | End-to-end sign and verify. |

## CI performance workflow

The CI runs benchmarks for these crates on every push:

```makefile
CI_BENCH_CRATES := nautilus-core nautilus-model nautilus-common nautilus-live
```

Run locally with:

```bash
make cargo-ci-benches
```
