# Design Principles

These principles describe the architectural guarantees and trade-offs that users and developers
can rely on across NautilusTrader.

## Message immutability

Messages (requests, responses, events, and commands) are immutable after creation. Their fields
remain unchanged for the rest of the message lifetime. See
[Message Bus: message integrity](../concepts/message_bus.md#message-integrity) for the ownership
rules that follow from this invariant.

The invariant protects several properties the system depends on:

- **Determinism**: Every consumer sees the same input. Behavior is easier to reason about, replay,
  and test.
- **Temporal integrity**: A message preserves what was true when the system emitted it. Events and
  commands remain factual records instead of containers of drifting state.
- **Safer concurrency**: Readers do not need coordination to protect message payloads from later
  rewrites. This removes a common source of races around shared state.
- **Easier debugging**: Logs, traces, replay tools, and dead-letter inspection remain useful
  because the message still reflects the original payload.
- **Reliable replay and simulation**: Replaying a sequence yields the same logical inputs as the
  original run. This supports backtesting, incident reconstruction, and regression testing.
- **Clear ownership boundaries**: Components treat incoming messages as input. If a component needs
  a different representation, it derives new local state or a new message explicitly.
- **Better auditability**: The system can answer what it knew, when it knew it, and what it did
  from that information.
- **More robust distribution**: Serialized messages already cross process and service boundaries as
  copies. The same ownership rule keeps the in-memory model aligned with that reality.

## Interned identifier storage

### How string interning works

String interning stores one shared copy of each distinct string in a central cache. Repeated values
refer to the same cached bytes instead of allocating another copy. Small handles make the values
cheap to copy and compare, while a cached hash avoids reading the full string again during hashing.

NautilusTrader uses `Ustr` for its interned identifier components. Each `Ustr` is a pointer-sized
`Copy` handle with a precomputed hash and stable direct string access. Composite types such as
`InstrumentId` preserve the same cheap copy semantics by storing these handles.

### Reclamation boundary

The string cache retains every unique value for the process lifetime. This retention keeps copied
handles and returned string slices valid without reference counting, access guards, or explicit
lifetime parameters on identifier types. Process teardown is the normal reclamation boundary.

These guarantees rule out safe reclamation of individual entries. Rust can copy a `Copy` value
without executing code, so an atomic reference count cannot observe every copy. Designs that add
reclamation change the identifier contract:

- Reference counting requires `Clone` and `Drop`, which removes `Copy` from identifiers and types
  that contain them.
- Borrowed or epoch-protected storage requires lifetimes or access guards at string access points.
- Generational handles permit reclamation but make lookup fallible and invalidate stale handles.
- A global cache reset is safe only at a proven quiescent point after all handles, references, and
  foreign pointers have been destroyed and no task or thread can retain one.

### Storage boundaries

Interning is best suited to identifiers drawn from a bounded process-scoped universe and values that
repeat enough to benefit from deduplication. Identifiers whose distinct values can grow with every
order, trade, or message increase the cache for the process lifetime.

Fixed-capacity inline storage retains `Copy` when the external protocol supplies a suitable maximum.
`TradeId`, for example, uses a 36-character `StackStr`. Owned or reference-counted storage provides
dynamic capacity when reclamation matters more than `Copy`.

The domain model also contains compatibility exceptions. `ClientOrderId`, `VenueOrderId`,
`PositionId`, and `OrderListId` remain `Ustr`-backed and therefore retain every distinct value.
Identifier storage participates in the supported by-value C ABI, so a broader redesign depends on
conversion-based bindings replacing raw layout sharing.

The storage boundary includes an up-front estimate of every unique value and all intermediate
strings interned during parsing. The cache is shared by every `Ustr` use in the process, so its
memory cost is the aggregate set rather than a separate budget for each identifier type.

### Polymarket scale example

A Polymarket instrument symbol combines a 66-byte condition ID with a 77- or 78-byte token ID, for
a 144- or 145-byte interned symbol. With the 64-bit `ustr` 1.1.0 layout, 600,000 unique
`InstrumentId` values require roughly 150 MiB for the retained identifier values, cache lookup
table, and reserved string storage.

The Polymarket parsing path also interns each raw token ID and each condition ID. For 600,000
instruments from about 300,000 markets, these entries raise the estimate to roughly 300 MiB before
instrument objects, descriptions, maps, and other metadata. The estimate assumes unique instrument
and token IDs and includes capacity reserved by the cache's geometric allocator, so it is not an
exact resident-set measurement.

NautilusTrader accepts this bounded cost to preserve `Copy`, stable direct access, and global
deduplication across the instrument universe. Unbounded streams of unique external IDs remain
outside this storage model.

## Behavioral model architecture

### Model family structure

Behavioral model families use a common representation across simulation and execution:

- A Rust `<Family>Model` trait defines the behavioral contract.
- Concrete Rust types implement the built-in models.
- A `<Family>ModelAny` enum lists the core built-ins and any language bridges that require enum
  storage, then implements the trait through explicit enum dispatch.
- A `<Family>ModelHandle` stores a shared trait object where runtime components accept linked Rust
  implementations beyond the enum variants.

Supported concrete built-ins are exposed as PyO3 classes. Their
[type stub annotations](rust.md#type-stub-annotations) feed the
[generated Python artifacts](rust.md#generated-python-artifacts). Backtest configuration accepts
these concrete model objects directly rather than using separate model configuration and factory
wrappers.

Adapter-specific models live in their adapter crate when a core enum variant would create a reverse
dependency. Low-level Rust code passes these models through the corresponding handle. Python
exposure uses an explicit bridge for the model family, either as an enum variant or as a trait
implementation passed through the handle, depending on the storage boundary. Latency and margin
configuration accept built-in models only from Python.

### Dispatch boundary

| Form                     | Accepted implementations               | Dispatch     | Role                                          |
| ------------------------ | -------------------------------------- | ------------ | --------------------------------------------- |
| Concrete type or generic | One concrete implementation            | Static       | Model internals and specialized callers.      |
| `<Family>ModelAny`       | Declared built-ins and bridge variants | Enum match   | Built-in and bridge configuration or storage. |
| `<Family>ModelHandle`    | Any accepted Rust trait implementation | Trait object | Shared runtime storage and custom types.      |

`<Family>ModelAny` uses enum dispatch. `<Family>ModelHandle` uses dynamic dispatch through a trait
object, so a built-in converted from the enum into a handle crosses a vtable before its enum match.
Built-in-only storage remains typed as `<Family>ModelAny` where avoiding trait-object dispatch
matters. The handle has no separate built-in fast path; the simpler single representation remains
because no measured performance case justifies the additional variant and dispatch complexity.

### Native extension boundary

The open-source [plug-in crate](plugins.md) defines an artifact ABI, but model registration and the
loading host are not part of this repository. The open-source distribution does not provide runtime
native model plugins. Native models are composed at compile time and passed through the
corresponding enum or handle.

### Simulation modules

Simulation modules use the enum and handle forms at different configuration boundaries:

| Boundary                                       | Stored form              | Accepted implementations              |
| ---------------------------------------------- | ------------------------ | ------------------------------------- |
| Declarative `BacktestVenueConfig`              | `SimulationModuleAny`    | Built-ins and language bridges.       |
| `SimulatedVenueConfig` and `SimulatedExchange` | `SimulationModuleHandle` | Any linked Rust trait implementation. |

`SimulationModuleHandle` owns an `Rc<dyn SimulationModule>`, so cloning a handle shares the module
and its state. Cloning a built-in enum value copies its state, while cloning a Python bridge retains
the same Python object. Venues or runs that require isolated state therefore use distinct module
instances, including distinct Python objects.

#### Lifecycle

The exchange runs each module through this lifecycle:

1. `pre_process` runs before the exchange processes each supported market data item.
1. `process` runs for each module in order against the same read-only exchange snapshot after
   commands have settled for the timestamp. Processing stops at the first failure, and the exchange
   applies no adjustments from that timestamp.
1. For each completed result in order, the exchange applies its batch as ordered `Money`
   adjustments, then calls that module's `acknowledge` exactly once with the corresponding outcomes,
   including for an empty batch.

#### Failure handling

- Failures from `pre_process`, `process`, `acknowledge`, or `reset` leave the exchange in an error
  state until every module resets successfully. This prevents a failed acknowledgement from
  replaying adjustments that the account may already contain.
- Diagnostic failures return to the engine with the module index and hook name without changing the
  exchange error state.

#### Python modules

The `process` hook for a Python `SimulationModule` subclass receives an owned
`SimulationModuleContext` snapshot containing:

- The venue.
- The optional base currency.
- The instruments.
- The order books.
- The open positions.

The bridge does not expose mutable cache or matching-engine state. Python exceptions retain the hook
name as they propagate through the exchange and `BacktestEngine.run`.

#### Linked native types

Linked native PyO3 types can register an extractor for their Python class. The extractor resolves an
object for imperative `BacktestEngine.add_venue` configuration. Python configuration resolves
modules as follows:

| Configuration path         | Accepted objects                                      | Stored form              | Native extractor behavior  |
| -------------------------- | ----------------------------------------------------- | ------------------------ | -------------------------- |
| `BacktestEngine.add_venue` | Built-ins, linked native types, and Python subclasses | `SimulationModuleHandle` | Matches the exact type.    |
| `BacktestVenueConfig`      | Built-ins and Python subclasses                       | `SimulationModuleAny`    | Does not consult registry. |

An unrelated class with the same name does not select a registered extractor. Extractor
registration does not create a runtime ABI for trait objects across a `cdylib` boundary.

#### Built-in modules

The built-in FX rollover and CFD swap modules use the completed-batch acknowledgement flow. CFD swap
rates are per-instrument signed daily `Decimal` fractions of settlement notional, with separate long
and short values, a configurable UTC rollover time, and a configurable triple-roll weekday. For a
single-currency account, the module converts the adjustment to the account base currency at the
cached mid exchange rate.

The CFD swap module defers the whole batch when any of these inputs is missing:

- A matching engine.
- A settlement price.
- An exchange rate.

The module logs one warning per booking date, instrument, and failure kind before quieter retries.

Perpetual funding remains part of `SimulatedExchange` and is not a simulation module.
