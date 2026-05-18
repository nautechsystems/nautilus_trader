# nautilus-event-store

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Embedded event store and authoritative log of state-affecting messages for
[NautilusTrader](https://nautilustrader.io).

> [!WARNING]
> **Early alpha**. The API is not stable and may change between versions. Event-store capture,
> replay, and verification workflows are the current focus.

The `nautilus-event-store` crate is designed to capture commands, generated events, raw venue
reports, reconciliation outputs, and request/response traffic at the message bus's publish and
send entry points. It persists those messages as a durable per-run log and exposes a typed replay
path. Combined with cache snapshots anchored to a durable log high-watermark, it provides the
state-affecting history needed for audit, deterministic replay, end-to-end correlation of agent
decisions, and counterfactual research.

## Design contract

The event store is the authoritative durable boundary for the deterministic engine's state-affecting history:

- The synchronous core is deterministic.
- Replay derives cache state from captured history and named replay rules, not from the cache as an independent authority.
- The cache is a write-through projection, not the source of truth.
- The event store records the ordered inputs and generated state-affecting messages that make replay possible.
- Anything not recorded is either non-state-affecting or explicitly named as a deterministic replay rule.
- External I/O remains outside the bit-identical core unless it is captured as raw reports or commands.

## Scope

`nautilus-event-store` is a single-node, embedded event store for one trading instance. It
captures state-affecting bus traffic at the publish and send entry points, persists it as a
per-run log, and exposes replay paths for the kernel, agents, and deterministic simulation
testing (DST).

It defines:

- The capture surface: which bus traffic is recorded, and where capture is instrumented.
- The on-disk representation: run organization, schema, and the high-watermark contract.
- The replay modes: forensics, decision, and full incident.
- The implementation roles: writer, reader, backend, and bus capture adapter.

It does not define:

- Replay-from-zero as the default recovery path. The cache stays write-through; replay-from-zero
  is an optional optimization tracked separately.
- Cross-instance aggregation. This crate is single-node, single event store.
- Analytics queries (the event store is not an OLAP engine).

## Properties

Two load-bearing guarantees:

- **Authoritative audit and replay.** Every state-affecting message captured at the message bus
  dispatch boundaries is durably committed before its consumers see it. A run identified by
  `(seed, binary_hash, config_hash, schema_version, log)` reproduces observable behavior
  bit-identically across the crates covered by deterministic simulation testing when paired with
  captured raw venue inputs and the canonical replay order. Outside that scope, such as adapter
  network I/O, replay is causally close: same logical sequence, not bit-exact byte order.
- **Snapshot-anchored recovery.** Cache snapshots are stamped with the durable log high-watermark
  at capture time. A run can be recovered to that high-watermark, plus its tail entries, into a
  deterministic engine. Live restart adopts this path once every cache mutation site is covered
  by a captured entry or a named deterministic replay rule, and the verification suite proves the
  coverage. Until then, live restart keeps snapshot-plus-reconcile and the event store serves as
  the authoritative audit and replay substrate.

The store also enables:

- Forensics: "show me everything that touched this order" as an event-store scan.
- Counterfactual research: mutate the recorded stream and rerun under deterministic simulation.
- End-to-end correlation of agent decisions: envelope plus log slice keyed by `intent_id`.
- Eval reproducibility: A/B agent policies against the same captured world stream.

## Non-goals

- Replacing the data catalog. Market-data observations stay in the Feather streaming catalog; the
  event store carries state-affecting messages only.
- Bit-exact replay across adapter-originated message reordering without deterministic simulation
  seams. Inside that scope, replay is bit-identical; outside it, replay is causally close.
- Multi-instance authoritative consensus. The store is embedded and per-instance.
- Sensitive-payload handling. Redaction, encryption-at-rest, and tamper evidence are deferred
  and are not part of the early alpha API contract.

## Terminology

- **Run.** A kernel session: from start to graceful stop or crash. One instance, one binary, one
  config. The unit of organization on disk. A run never spans process recovery; recovery starts
  a new run that records its `parent_run_id` in the manifest.
- **Entry.** One row in the event store: a captured bus message plus metadata.
- **Seq.** The per-run monotonic sequence assigned by the writer at commit time. It is the
  replay-order authority.
- **High-watermark.** The largest `seq` whose entry has been durably acknowledged by the backend.
  Snapshots are anchored to this value.
- **Snapshot anchor.** The high-watermark recorded atomically with a cache snapshot. Replay from
  a snapshot resumes at `seq > anchor`.
- **Headers.** First-class metadata propagated end-to-end: `intent_id`, `correlation_id`,
  `caused_by`. Header propagation spans command construction, endpoint sends, generated events,
  and reconciliation reports.

## Capture surface

Captured at the message bus dispatch boundaries:

- Commands (`SubmitOrder`, `CancelOrder`, `ModifyOrder`, `Subscribe*`, `Unsubscribe*`, etc.) at
  the send and publish entry points, before any handler runs.
- Generated state-affecting events (order, position, account, fill events) at the publish entry
  point. For cache-first order lifecycle paths, the event is published only after the cache
  mutation succeeds; capture records the successful lifecycle event before downstream handlers see
  it.
- Raw venue reports (`OrderStatusReport`, `FillReport`, `PositionStatusReport`) on a dedicated
  topic before reconciliation synthesizes any derived event from them.
- Reconciliation outputs (the synthesized events the reconciliation manager publishes) at the
  publish entry point. This includes the synthesized `OrderInitialized` produced for external
  orders during reconciliation.
- Request/response traffic that crosses the bus and affects state.
- Subscription lifecycle changes (`Subscribe*` and `Unsubscribe*` activations and removals) so
  replay reconstructs the strategy or agent's observation window.
- Account updates and timer firings that drive state-affecting handlers.
- Run-lifecycle entries: `RunStarted` (durably committed before any other entry of the run) and
  `RunEnded` on graceful shutdown. The `RunStarted` payload includes the registered component
  manifest so replay binds actors, strategies, algorithms, subscriptions, and command endpoints
  without consulting external config.

Capturing raw reports before synthesis is load-bearing: replay must be able to re-run
reconciliation against the same input the live engine saw, not only against the events it
produced.

## Cache replay contract

- Order creation, local pending update, and local pending cancel paths mutate the cache first and
  publish second. Subscribers in the synchronous core may read cache immediately, and failed cache
  mutations must not emit phantom lifecycle events.
- External-order materialization publishes the synthesized `OrderInitialized` after the cache
  insert succeeds. The raw venue report is captured before reconciliation, so replay can inspect
  both the input and the derived order origin.
- Contingent and spawned order `position_id` propagation is engine-derived from `OrderFilled`.
  Replay runs that propagation rule, and verification asserts that replay reproduces those links.
- Load-time cache fixups and structural registration are outside the captured steady-state run.
  Snapshot-plus-tail live recovery depends on the snapshot contract, the `RunStarted` component
  manifest, and verification coverage.

## Architecture

Capture sites are instrumented at the bus's publish and send entry points. Each captured message
is forwarded to a dedicated writer thread. The default store backend is
[`redb`](https://docs.rs/redb/latest/redb/), an embedded ACID key-value store.

```text
+-----------------+   publish/send   +---------------------+
| MessageBus tap  | ---------------> | Bus capture adapter |
+-----------------+                  +----------+----------+
                                                | sync_channel
                                                v
                                     +--------------------+
                                     | Event store writer | dedicated thread, batched commits
                                     +----------+---------+
                                                v
                                     +-----------------+
                                     | Store backend   | redb (default)
                                     +-----------------+
```

Bus-level capture frees adapters and engines from per-call instrumentation. Capture sites pass
the entry to the writer through a bounded `std::sync::mpsc::sync_channel`, mirroring the dedicated
writer-thread shape used by the existing logging subsystem. The event store uses a *bounded*
channel where logging uses an unbounded one; the audit contract requires capture, so the
backpressure policy is no-drop.

The writer batches up to ~100 entries or ~5 ms (whichever first), commits them in a single
backend transaction, and amortizes the fsync cost across the batch. This policy comes from the
storage-backend benchmark: with `redb` 4.1.0, 256 B payloads, `Durability::Immediate`, and an
NVMe/ext4 host, a batch size of 100 measured 5.16 ms p50 commit latency and 18,356 entries/sec.
That is two-to-three orders of magnitude above the expected captured-traffic rate; larger batches
cut tail latency only marginally and trade against burst responsiveness. The
high-watermark advances only when the backend acknowledges the commit. Backpressure: the channel
send blocks until the writer accepts. If a stall exceeds a configurable threshold, the kernel
halts rather than dropping or proceeding unaudited. Readers compose with the writer through the
backend's MVCC.

Persistence ordering has two boundaries. Inbound bus messages and raw reports are durably
committed before handler dispatch. Generated lifecycle events are durably committed before bus
fanout to downstream handlers. Some synchronous-core paths mutate cache before publishing the
generated lifecycle event; for those paths, the event store records the successful mutation at
the publish boundary and does not claim durable-before-cache-write ordering. State-affecting
handlers reached through the bus run after the writer has acknowledged the captured entry's
batch.

The tap must fire before fanout, never after. Three guarantees depend on it:

- **Replay fidelity.** If a handler observes a message and mutates state, the message must be in
  the log for replay to reconstruct that state. Tap-after-fanout would let handlers commit state
  changes whose originating message never reached the log, producing inconsistent replays.
- **Fail-stop coupling.** When the writer halts or fails its submit, the halt callback fires
  inside the tap dispatch. Tap-after-fanout would let the in-flight message reach handlers and
  mutate state before the kernel responds to the halt.
- **Causal ordering on the log.** Forensics scans and replay rely on the cause (command, raw
  report) preceding its effects (generated events). Tap-after-fanout would invert this on the
  hot path: handler-emitted events would commit before the message that produced them.

## API shape

**The public API is intentionally not frozen yet.** The implementation should expose four roles:

- A backend abstraction for opening a run, appending entries, scanning by `seq`, and looking up
  secondary indices.
- A single writer for each run, responsible for batching, durability, high-watermark advancement,
  and fail-stop signaling.
- A reader for range scans, entity lookups, `intent_id` lookups, and run iteration.
- A message bus capture adapter that converts captured bus traffic into event-store entries and
  hands them to the writer.

Entry metadata must include a per-run `seq` replay-order authority, domain timestamps such as
`ts_init` and optional bus-accepted time, topic or endpoint identity, payload type, payload bytes,
and correlation headers. Exact Rust type names, field names, and serialization choices are early
alpha implementation details.

## On-disk layout

The on-disk realization is backend-specific. The crate exposes only the logical layout. Every
backend stores per-run entries keyed by `seq`, with sidecar indices for `intent_id` and message
id lookups, plus a manifest and an optional snapshot anchor.

Logical layout, per-run:

- A monotonic ordered sequence of event entries keyed by `seq`.
- Indices: `intent_id -> seq`, `client_order_id -> seq`, `venue_order_id -> seq`.
- Manifest (durably committed at run start, sealed at end).
- Snapshot anchor (high-watermark plus snapshot blob reference) recorded atomically with each
  cache snapshot.

The [`redb`](https://docs.rs/redb/latest/redb/) backend stores entries and indices in named tables
inside one `redb` file per run; a custom-WAL backend stores entries as length-prefixed records
with frame-level CRC32 in segment files plus sidecar index files. Both expose the same logical
contract; the crate does not promise a particular file format.

Run manifest:

```text
run_id                <start_ts_init>-<short_uuid>; sortable by start time, unique by uuid
parent_run_id         optional; set when this run resumes a crashed predecessor
instance_id           from kernel config
binary_hash           hash of the trader binary
schema_version        bumps when entry payload schema changes
crate_versions        hash of Cargo.lock or equivalent crate version manifest
feature_flags         active Cargo features
adapter_versions      per-adapter version stamp
config_hash           hash of the kernel config
registered_components actor/strategy/algorithm ids, config hashes, endpoint bindings
seed                  if running under a seeded mode
start_ts_init         first ts_init in the run
end_ts_init           last ts_init in the run, or null if crashed
high_watermark        largest seq durably acknowledged at end of run
status                "running" | "ended" | "crashed-recovered" | "quarantined"
```

`RunStarted` is the first entry of every run and must be durably committed before any
state-affecting entry. `RunEnded` is the last entry on graceful shutdown. On boot, the kernel
scans `<instance_id>/` for any run whose status is `running` and that lacks a `RunEnded` entry,
seals it as `crashed-recovered` (or quarantines it for inspection), and starts a new run that
records the crashed predecessor's `run_id` as its `parent_run_id`.

## Replay modes

- **Forensics replay** (event store only). Range scan by `seq`, or lookup by `intent_id` /
  `client_order_id` / `venue_order_id`. Loads in seconds for a day's run; no data catalog needed.
- **Decision replay** (event store plus selected data catalog topics). Stream-table join: the
  catalog snapshot at run start plus subscription deltas during the run. Joined by `ts_init`,
  filtered by the strategy or agent's `Subscribe*` activations from the captured stream.
- **Full incident replay** (event store plus all relevant data catalog slices). Stream-stream
  join joined by `ts_init` plus instrument identity. Tolerates clock skew within deterministic
  simulation scope; outside that scope, ordering is causally close but not bit-exact for
  adapter-originated traffic.

A boot kernel flag `--replay-from <run-id>` skips venue reconciliation against the live venue,
restores the snapshot anchored to the run's high-watermark, replays the tail in `seq` order, and
exits or freezes for inspection. Reconciliation behavior during replay is reconstructed from the
captured raw venue reports; replay never queries the live venue. Live restart, when it adopts
event-store recovery, deduplicates by entry id when catching up past the run tail against the
live venue.

## Recovery

Crash recovery composes four primitives, addressed at four crash classes:

- **Before enqueue.** The captured message never reached the writer's channel. The producer's
  retry policy applies; the event store has no record. No durability claim covers this class.
- **After enqueue, before commit.** The entry is in the channel but not yet durably acknowledged.
  On crash, the in-flight batch is lost; on graceful shutdown, the writer drains the channel
  before exiting. The high-watermark does not advance until the backend acknowledges.
- **After commit, before snapshot anchor.** The entry is durable but a snapshot has not been
  recorded since. Recovery loads the prior snapshot and replays the tail since that anchor.
- **After snapshot anchor.** Recovery loads the latest snapshot and replays the tail since its
  anchor. This is the steady-state recovery path.

Live catch-up past the run tail (when the new run resumes trading) deduplicates against the
captured stream by entry id and venue identifiers.

Backend recovery time is asymmetric in the storage-backend benchmark. After graceful shutdown,
the `redb` backend reopened in single-digit milliseconds for files up to one-month scope;
copy-on-write with header validation skips log replay entirely. After a crash with an in-flight
transaction, `redb` walked the file at roughly NVMe-bandwidth (around 700 MB/s on the benchmark
host) to discard aborted writes and rebuild allocator state. Per-run files at one-month scope
(~18 GiB) reopened in tens of seconds. Per-run-file rotation is therefore the scaling unit:
long-lived runs that grow indefinitely would breach the restart target and break the audit
posture.

## Determinism contract

The store's determinism guarantee depends on what is paired with it:

- **Replay against captured inputs**: deterministic in `seq` order. Forensics, audit, and
  decision-level replay are exact replays of the captured stream.
- **Replay paired with deterministic simulation seams** for
  `(seed, binary_hash, config_hash, schema_version, log)`: bit-identical across the in-scope
  crates. Adapter network I/O is out of scope for bit-identicality; replay there is causally
  close, same logical sequence.
- **Bit-exact across adapter-originated reordering** without deterministic simulation: not
  promised. `ts_publish` bounds drift but is not the ordering authority; `seq` is.

`seq` is the replay-order authority, assigned by the writer at commit time. `ts_init` is a domain
timestamp, strictly monotonic and unique system-wide via `AtomicTime`'s `AcqRel`
compare-and-exchange. `ts_publish`, when populated, records the bus-accepted time; the writer
stamps it on receive, and the bus stamps it before fanout when cross-subscriber ordering matters.
Neither is used to order replay.

Idempotency under replay composes four primitives:

- **Immutable-value addressing.** Entries are addressed by `seq`; `seq` never points at a
  different entry once committed.
- **Expected-version on write.** The writer rejects out-of-order commits; consumers can rely on
  monotonic `seq` advance.
- **Gap detection on read.** A reader processing `seq=N+1` without having seen `seq=N` detects a
  silent skip rather than missing it.
- **Entity-keyed dedup on catch-up.** When live restart catches up past the run tail against
  the live venue, the engine dedups against captured `client_order_id` and `venue_order_id`
  before applying.

The four primitives compose to make double-apply and silent-skip unrepresentable, not merely
unlikely.

## Snapshot contract

Cache snapshots are owned by the cache, not by the event store. The event store stores only an
anchor: the high-watermark `seq` at the moment the snapshot was captured, plus a content-
addressed reference to the snapshot blob. The cache writes the blob to its own backing store;
the event store records the anchor inside the same transaction that advances the
high-watermark, so the anchor is durable iff the high-watermark is durable.

This is the blob-outside-store pattern. The alternative (snapshot-as-event) would couple
snapshot retention to log retention and require the event store to understand snapshot blobs;
we keep them separate.

Restore reads the anchor, fetches the blob from cache storage, validates its content hash
against the anchor, and replays log entries with `seq > anchor`. A blob whose hash mismatches
the anchor fails the restore and quarantines the run for inspection.

## Retention

Entries with `seq <= snapshot_anchor` are reclaimable. Reclamation rotates segments (or
backend files) as a unit, never by individual entry. The retention contract trades audit and
forensics depth against storage cost; the operator picks the trade per deployment:

- **Full retention**: every entry of every run is preserved indefinitely. Required for
  long-retention audit needs and unlimited counterfactual research.
- **Bounded retention**: runs older than N days are reclaimed. Forensics scope is bounded by
  N; counterfactual research over older runs requires re-fetching from cold storage.
- **Snapshot-anchored retention**: reclaim entries up to the most recent snapshot anchor of
  each sealed run. Restart and forensics still work for that run's tail; counterfactual replay
  before the anchor does not.

The default is full retention; bounded modes are configured per deployment. Reclamation never
touches a run whose status is `running`. Bounded retention must keep at least one
known-good prior sealed run as a complete restore point: its manifest, its snapshot anchor,
the external snapshot blob the anchor references, and the entry tail since that anchor.
A bare `redb` file alone is not a restore point. The supervisor falls back to this restore
point when the latest run quarantines on corruption, rather than entering a restart loop.

## Storage backend

The default backend is [`redb`](https://docs.rs/redb/latest/redb/): a pure-Rust ACID kv store with a
single-file B-tree and MVCC.
`redb`'s single-writer many-reader model aligns with the dedicated writer thread. The
`EventStore` trait encapsulates the backend so backend swaps do not touch consumers, and the
crate does not expose `redb`-specific types in its public API. Durability is set explicitly:
every commit uses `Durability::Immediate`. The `redb` file is one per run, sized for retention
rotation as a unit.

Backend failure model. The storage-backend benchmark characterized `redb`'s response to disk
pressure and physical corruption. Disk pressure (ENOSPC, RLIMIT_FSIZE) returns a typed
`Io(FileTooLarge)` error from `Table::insert` or `WriteTransaction::commit`; the aborted
batch's entries are not visible after reopen, prior commits are intact, and the high-
watermark has not advanced. The writer surfaces the error to the kernel halt path and
fail-stops. Header-region corruption is detected on open as a typed `Storage(Corrupted)`
error. Data-page corruption (truncation, mid-tree bit-flips) is not framewise-checksummed
by `redb` 4.x and surfaces as an assertion failure or unreachable-code panic, on open or on
first read. The release profile builds with `panic = "abort"`, so in-process `catch_unwind`
does not contain these panics; the trader process aborts.

Two consequences:

1. **Process-level supervision, not in-process panic catching.** Recovery is owned by an
   external supervisor that observes the trader's exit, marks the failing run as quarantined
   in its manifest, and either falls back to the prior sealed run or refuses to start.
   Untrusted run files (third-party imports, restored backups) are scanned in an isolated
   verifier process before the trader opens them, so a bad file aborts the verifier, not
   trading. Wrapping only open is insufficient: a `zero-tail` corruption opens cleanly and
   panics on first read, so the verifier exercises a full integrity scan, not just the
   file header.
2. **Canonical entry hash on every captured entry.** Every captured entry carries a
   single canonical hash over its full content (`seq`, `ts_init`, `ts_publish`, `topic`,
   `payload_type`, `payload`, `headers`) computed at capture time and stored alongside the
   entry. Readers, replay, export, and the verifier process recompute and check it; a
   mismatch quarantines the run. Sidecar indices (`intent_id -> seq`,
   `client_order_id -> seq`, `venue_order_id -> seq`) are rebuildable projections from the
   `seq -> entry` table, not authoritative storage; the verifier rebuilds and cross-checks
   them. Corruption that `redb` misses is caught by hash mismatch on the next read of the
   affected entry, and the run never proceeds unaudited.

Sealed runs may be exported to opt-in downstream sinks. An outage of any downstream sink
never blocks trading.

## Nautilus Agents composition

[Nautilus Agents](https://github.com/nautechsystems/nautilus_agents) records each agent decision
cycle as a `DecisionEnvelope`. The event store records the engine-side history that surrounds that
decision: the ordered world inputs, the command stream, and the generated state-affecting events
keyed by `intent_id`.

Together, the decision envelope and the event-store slice form one auditable transaction:

- The envelope records what the agent observed, decided, and passed through capability and
  guardrail checks.
- The event store records what the deterministic engine accepted, rejected, applied, filled, or
  otherwise changed as a result.
- Replay can compare policy or guardrail changes against the same captured world stream.

Agent-private records remain the agent framework's capture surface. The event store owns the
trader-side durable history that makes those decisions reproducible against engine state.

## DST composition

The event store and DST close different halves of replay. The event store supplies the durable
input history; DST supplies the execution environment that proves the same history produces the
same engine behavior.

Together, a captured run and the DST seams form the replay contract:

- The event store records the ordered commands, raw reports, generated events, and correlation
  headers that define the run.
- DST controls scheduling, time, seeded randomness, and other in-scope nondeterminism while the
  engine reprocesses that stream.
- Verification covers capture coverage, no-drop behavior, and named replay rules such as derived
  `position_id` propagation.

Under simulation (`cfg(madsim)`), a synchronous in-memory event store replaces the writer thread,
so tests assert against an authoritative in-process log without disk I/O or thread scheduling.
Production-shape integration tests outside `cfg(madsim)` exercise the on-disk writer.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Documentation

See [the docs](https://docs.rs/nautilus-event-store) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).
