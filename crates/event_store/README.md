# nautilus-event-store

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Embedded event store and authoritative log of state-affecting messages for
[NautilusTrader](https://nautilustrader.io).

> [!WARNING]
> **Early alpha**. The API is not stable and may change between versions. Event-store capture,
> replay, and verification workflows are the current focus.

The `nautilus-event-store` crate captures commands, generated events, raw venue reports,
reconciliation outputs, and request/response traffic at the message bus boundary. It persists
those messages as a durable per-run log and exposes read-only verification and replay surfaces.

For the design model, capture boundaries, replay modes, and operational examples, see the
[Event Sourcing concept guide](../../docs/concepts/event_sourcing.md).

## Design contract

The event store is the durable boundary for deterministic engine history.

- The synchronous core is deterministic.
- The cache is a write-through projection, not the source of truth.
- Replay derives state from captured history and named deterministic replay rules.
- The event store records ordered inputs and generated state-affecting messages.
- Anything not recorded is non-state-affecting or named as a deterministic replay rule.
- External I/O becomes replayable when captured as raw reports or commands.

## What this crate provides

`nautilus-event-store` is a single-node, embedded event store for one trading instance. It defines:

- `BusCaptureAdapter`: the seam between message bus dispatch and the writer.
- `EventStoreWriter`: the append path, batching, high-watermark advancement, and fail-stop signaling.
- `EventStoreReader`: the read-only range scan, point lookup, and replay-facing surface.
- `RedbBackend`: the default on-disk backend, with one `redb` file per run.
- `Verifier`: the library surface for integrity checks over a single run.
- `verify`: the standalone binary for process-isolated verification of sealed run files.
- `plan_redb_retention`: a non-destructive planner for sealed run-file reclaim candidates.

The crate does not replace the data catalog, provide OLAP queries, or aggregate multiple trader
instances into a consensus log.

## Operational use today

Verify a sealed run file:

```fish
cargo run -p nautilus-event-store --bin verify -- /path/to/run.redb
```

Verifier exit codes:

- `0`: the run is clean.
- `1`: the run has corrupt findings, or the verifier worker aborted or timed out.
- `2`: the verifier could not open or run against the requested file.

The verifier opens the run through a read-only `redb` handle and reports
`quarantine=not-performed`. A supervisor or operator process owns quarantine policy.

Read a sealed run from Rust:

```rust
use nautilus_event_store::{EventStoreReader, RedbBackend, ScanDirection};

fn inspect_run() -> Result<(), Box<dyn std::error::Error>> {
    let backend =
        RedbBackend::open_sealed_file("./event_store/trader-001/1700000000-cafe0001.redb")?;
    let reader = EventStoreReader::new(backend);
    let high_watermark = reader.high_watermark()?;

    for entry in reader.scan_range(1, high_watermark, ScanDirection::Forward) {
        let entry = entry?;
        println!("{} {}", entry.seq, entry.topic);
    }

    Ok(())
}
```

## Storage model

The default backend is [`redb`](https://docs.rs/redb/latest/redb/), a pure-Rust ACID key-value
store. The backend uses one file per run:

```text
<base>/<instance_id>/<run_id>.redb
```

Each file stores:

- Entries keyed by monotonic `seq`.
- Secondary indices for `client_order_id` and `venue_order_id`.
- A manifest written at run start and sealed at run end.
- An optional snapshot anchor for cache restore.

Every commit uses `Durability::Immediate`. The high-watermark advances only after the backend
acknowledges durability.

## Failure and verification model

The verifier is intentionally process-isolated. Some corrupted `redb` files can panic on open or
first read, and the release profile builds with `panic = "abort"`. The `verify` binary delegates
the scan to a worker subprocess so a bad run file aborts the worker, not the caller.

Integrity checks include:

- Recomputing every entry hash.
- Detecting gaps inside the high-watermark.
- Checking table-key and embedded-`seq` agreement.
- Cross-checking secondary indices against entries.
- Validating manifest status and high-watermark fields.

## Documentation

- [Event Sourcing concept guide](../../docs/concepts/event_sourcing.md): design, replay, recovery,
  and operational examples.
- [docs.rs](https://docs.rs/nautilus-event-store): generated Rust API reference.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## License

The source code for NautilusTrader is available on GitHub under the
[GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
