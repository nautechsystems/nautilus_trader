# Rust

[Rust](https://www.rust-lang.org/learn) provides the type system, ownership model, and predictable
performance needed by the mission-critical core of the platform. Safe Rust prevents data races and
many memory errors at compile time. Code that uses `unsafe` must state and uphold the invariants that
the compiler cannot check.

## Cargo manifest conventions

- In `[dependencies]`, list internal crates (`nautilus-*`) first in alphabetical order. Add a blank
  line, list required external dependencies alphabetically, then add another blank line and list
  optional dependencies alphabetically. Preserve inline comments with their dependency.
- Add `"python"` to every `extension-module` feature list that builds a Python artifact. Keep it
  adjacent to `"pyo3/extension-module"` so the full Python stack is clear.
- When a manifest groups adapters separately, keep the `# Adapters` block directly below the
  internal crate list.
- Always include a blank line before `[dev-dependencies]` and `[build-dependencies]` sections.
- Apply the same layout across related manifests when feature or dependency sets change.
- Use snake_case filenames for `bin/` sources, for example `bin/ws_data.rs`, and use those paths in
  each `[[bin]]` section.
- Keep `[[bin]] name` entries in kebab-case, for example `name = "hyperliquid-ws-data"`.

## Versioning guidance

- Use workspace inheritance for shared dependencies (for example `serde = { workspace = true }`).
- Only pin versions directly for crate-specific dependencies that are not part of the workspace.
- Group workspace-provided dependencies before crate-only dependencies so the inheritance is easy to audit.
- Keep related dependencies aligned: `capnp`/`capnpc` (exact), `arrow`/`parquet` (major.minor),
  `datafusion`/`object_store`, and `dydx-proto`/`prost`/`tonic`. Pre-commit enforces this.
- Adapter-only dependencies belong in the "Adapter dependencies" section of the workspace
  `Cargo.toml`. Pre-commit prevents core crates from using them.

## Feature flag conventions

- Prefer additive feature flags. Enabling a feature must not break existing functionality.
- Use descriptive flag names that explain what capability is enabled.
- Document every feature in the crate-level documentation so consumers know what they toggle.
- Common patterns:
  - `high-precision`: switches fixed-point value types from 64-bit to 128-bit integer backing.
  - `default = []`: keep defaults minimal.
  - `python`: enables Python bindings.
  - `extension-module`: builds a Python extension module (always include `python`).
  - `ffi`: enables C FFI bindings.
  - `stubs`: exposes testing stubs.

## Build configurations

To avoid unnecessary rebuilds, align Cargo features, profiles, and flags across related targets.
Cargo keys build artifacts by features, profiles, and flags. A mismatch creates separate artifacts
and can cause substantial recompilation.

### Primary targets

The Makefile and changed-crate scripts are the source of truth for target scope and features. Do not
copy their feature lists into new documentation.

| Target                   | Scope                              | Feature source                    |
|--------------------------|------------------------------------|-----------------------------------|
| `make cargo-test`        | Workspace libraries and tests.     | `CARGO_FEATURES` in the Makefile. |
| Pre‑commit Clippy        | Changed crate libraries and tests. | `scripts/clippy-changed.sh`.      |
| `make check-all-targets` | Workspace, including examples.     | `CARGO_FEATURES` plus `examples`. |

These targets use the `nextest` Cargo profile by default. When adding a target for the same surface,
match its profile and features so Cargo can reuse compiled artifacts.

### Documentation builds

Documentation is built separately using `make docs-rust`, which runs:

```bash
cargo +nightly doc --all-features --no-deps --workspace
```

This target uses nightly and `--all-features`, so it does not share all build artifacts with the
test and lint targets.

### Separate target (Python extension building)

| Target        | Profile   | Feature source          |
|---------------|-----------|-------------------------|
| `build`       | `release` | `_set_feature_flags()`. |
| `build-debug` | `dev`     | `_set_feature_flags()`. |

Both targets run `build.py`. Python extension builds require `extension-module`, so they use a
different feature set and create separate Cargo artifacts.

### Rebuild triggers to avoid

Mismatches in any of these cause full rebuilds:

- Different feature combinations (e.g., `--features "a,b"` vs `--features "a,c"`).
- Different `--no-default-features` usage (enables/disables default features).
- Different profiles (e.g., `dev` vs `nextest` vs `release`).

When adding or changing a build target, match the test and lint group when the target covers the same code.

### Generated FFI bindings and precision mode

The `nautilus-model` build script regenerates `nautilus_trader/core/includes/model.h` and
`nautilus_trader/core/rust/model.pxd` when the `ffi` feature is enabled. Those files encode
whether the generated C/Cython bindings use high precision. The committed generated files use
high precision. Local cargo commands that compile `nautilus-model` with `ffi` should either
include the `high-precision` feature or avoid regenerating those files.

Make targets that use `BASE_FEATURES`, such as `make build-debug-v2`, already include
`high-precision`. The drift risk mainly comes from ad-hoc cargo commands that enable `ffi`
without the aligned feature set.

Use the Rust feature for narrow checks that do not include the full aligned feature set. Keep the
environment override in the command so a stale shell value cannot force standard-precision bindings:

```bash
env HIGH_PRECISION=true cargo check -p nautilus-model --features ffi,python,high-precision
```

Before committing FFI-related work, verify those generated files did not drift:

```bash
git diff -- nautilus_trader/core/includes/model.h nautilus_trader/core/rust/model.pxd
```

If they changed only because a command ran without high precision, rerun the cargo command
with `HIGH_PRECISION=true`. Do not hand-edit the generated files.

## Module organization

- Keep modules focused on a single responsibility.
- Use `mod.rs` as the module root when defining submodules.
- Prefer relatively flat hierarchies over deep nesting to keep paths manageable.
- Use the narrowest practical visibility. The workspace denies unreachable `pub` items.
- Re-export intentional public API from the crate root.
- Keep imports at the top of the file or module unless a narrow local import materially improves clarity.
- Place private functions and types below their callers. In adapter modules, keep the primary client
  structs and implementations near the top, followed by private route types and functions.

## Code style and conventions

### File header requirements

All hand-written Rust files must include the standardized copyright header. Generated files are
exempt and must retain their generator header.

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------
```

:::info[Automated enforcement]
The `check_copyright_year.sh` pre-commit hook verifies copyright headers include the current year.
:::

### Code formatting

`rustfmt` formats imports when running `make format`. It groups standard library, external crate,
and local imports and sorts each group alphabetically.

Follow these spacing rules:

- Leave one blank line between functions, including tests.
- Leave one blank line above each doc comment (`///` or `//!`).
- Leave one blank line above standalone `if`, `match`, `for`, `while`, and `loop` expressions.
- Leave one blank line above spawn calls.

The control-flow and spawn rules do not apply when the expression starts a block, continues the
previous line's operation, or has an attached comment or attribute. The `check-formatting-rs`
pre-commit hook enforces these cases.

#### String formatting

Prefer inline format strings over positional arguments:

```rust
anyhow::bail!("Failed to subtract {n} months from {datetime}");
```

Avoid positional arguments:

```rust
anyhow::bail!("Failed to subtract {} months from {}", n, datetime);
```

This makes messages more readable and self-documenting, especially when there are multiple variables.

### Type qualification

Follow these conventions for qualifying types in code:

- **anyhow**: Fully qualify its macros and result type, such as `anyhow::bail!` and `anyhow::Result<T>`.
- **Nautilus domain types**: Import types such as `Symbol`, `InstrumentId`, and `Price`, then use them
  without a crate prefix.
- **Tokio**: Fully qualify its types and functions, such as `tokio::spawn` and `tokio::time::timeout`.
- **std::fmt**: Import `Debug` and `Display`, but fully qualify `std::fmt::Formatter` and
  `std::fmt::Result`. Use `debug_struct(stringify!(TypeName))` in manual `Debug` implementations.
- **Nautilus macros**: Import `nautilus_actor!` and `nautilus_strategy!`, then call them without
  a crate prefix.

```rust
use nautilus_model::identifiers::Symbol;

pub fn process_symbol(symbol: Symbol) -> anyhow::Result<()> {
    if !symbol.is_valid() {
        anyhow::bail!("Invalid symbol: {symbol}");
    }

    tokio::spawn(async move {
        // Process symbol asynchronously
    });

    Ok(())
}
```

:::info[Automated enforcement]
The `check_anyhow_usage.sh` pre-commit hook enforces these anyhow conventions automatically.
:::

### Logging

- Fully qualify `log` macros so the backend is explicit, for example `log::debug!` and `log::info!`.
- Start messages with a capitalized word and omit terminal periods.
- Keep connection lifecycle, client lifecycle, reconnection, reconciliation, and mass-status
  summaries at `INFO`.
- Keep subscription details, per-order confirmations, instrument counts, authentication, and
  WebSocket internals at `DEBUG`.
- Leave a blank line above a log call unless it is the first line of the function.
- Do not write directly to stdout or stderr or call `std::process::exit` from production library
  code. Binaries, examples, benches, tests, adapters, the CLI, and testkit are exempt where direct
  process control is part of their role.

:::info[Automated enforcement]
The `check_logging_conventions.sh` hook enforces macro qualification, terminal periods, direct
output, and process exits.
:::

### Error handling

Choose the error type at the API boundary:

| Boundary                              | Return type                         |
|---------------------------------------|-------------------------------------|
| Reusable library or domain API.       | A typed `Result<T, E>`.             |
| Application or adapter orchestration. | `anyhow::Result<T>`.                |
| Public input validation.              | `CorrectnessResult<T>` when suited. |

- Define typed errors with `thiserror` when callers can inspect or recover from the failure.
- Use `?` for error propagation.
- Bind error patterns and closures as `e`, not `err` or `error`.
- Prefer `anyhow::bail!` for early returns from functions that return `anyhow::Result`:

  ```rust
  pub fn process_value(value: i32) -> anyhow::Result<i32> {
      if value < 0 {
          anyhow::bail!("Value cannot be negative: {value}");
      }

      Ok(value * 2)
  }
  ```

- Use `anyhow::anyhow!` where an error value is required, such as `ok_or_else`.
- Do not use `", got"` in errors or assertions. Use `", was"`, `", received"`, or `", found"`
  according to the context.
- Start `.context()` messages with lowercase text so chained errors read naturally, except when the
  message starts with a proper noun or acronym:

  ```rust
  parse_timestamp(value).context("failed to parse timestamp")?;
  connect().context("BitMEX websocket did not become active")?;
  ```

:::info[Automated enforcement]
The `check_error_conventions.sh` hook enforces error variable names. The `check_anyhow_usage.sh`
hook enforces qualified imports and `anyhow::bail!` for early returns.
:::

### Async patterns

Use consistent async patterns:

- Use natural function names without an `async` suffix.
- Fully qualify Tokio types and functions. Use `std::time::Duration` rather than its Tokio re-export.
- Keep Tokio out of synchronous core crate dependencies. The `common` crate declares Tokio as
  an optional dependency.
- Document cancellation safety when cancellation can leave partial work or alter an invariant.
- Use `tokio_stream` or `futures::Stream` when back pressure matters.
- Apply timeouts at network and long-running operation boundaries. Avoid stacking redundant timeouts.
- Follow the [DST determinism contract](../concepts/dst.md#determinism-contract): route clocks, random
  values, task spawning, and network access through the project seams, and use `biased;` in
  `tokio::select!` blocks on the DST path.

### Adapter runtime patterns

Adapter crates under `crates/adapters/` use the shared runtime so calls from Python threads do not
depend on a thread-local Tokio context:

- **Spawn tasks**: Use `get_runtime().spawn()` instead of `tokio::spawn()` in production adapter code.

  ```rust
  use nautilus_common::live::get_runtime;

  get_runtime().spawn(async move {
      run_client().await;
  });
  ```

- **Import the re-export**: Use `live::get_runtime`, not `live::runtime::get_runtime`.

- **Bridge synchronous code**: Use `get_runtime().block_on()` when synchronous adapter code calls
  an async function:

  ```rust
  fn sync_method(&self) -> anyhow::Result<()> {
      get_runtime().block_on(self.async_implementation())
  }
  ```

- **Install custom runtimes before first use**: Rust-native binaries that own `main()` may call
  `set_runtime()` before `LiveNode::build()` or any adapter/client usage. Build custom runtimes
  with `tokio::runtime::Builder::new_multi_thread().enable_all()`; current-thread runtimes and
  runtimes without I/O or timer drivers do not satisfy adapter assumptions. If the `python` feature
  is enabled, prepare Python before building the runtime or keep the default initializer.

- **Use test runtimes in tests**: Code under `#[tokio::test]` has its own runtime context, so
  `tokio::spawn()` works correctly. The enforcement hook skips test files and test modules.

:::info[Automated enforcement]
The `check_tokio_usage.sh` hook enforces shared runtime and import restrictions in adapters.
:::

### Attribute patterns

Match the derive order used by nearby types. Keep related PyO3 and stub attributes adjacent, with
the runtime PyO3 path separate from the public stub path.

```rust
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct Symbol(Ustr);
```

For enums with extensive derive attributes:

```rust
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum AccountType {
    /// An account with unleveraged cash assets only.
    Cash = 1,
    /// An account which facilitates trading on margin, using account assets as collateral.
    Margin = 2,
}
```

### Type stub annotations

Python type stubs (`.pyi` files) are generated from Rust source using
[pyo3-stub-gen](https://github.com/Jij-Inc/pyo3-stub-gen). Every type and function
exposed to Python needs a matching stub annotation so the generated stubs stay in sync
with the bindings.

**Annotation types:**

| PyO3 construct    | Stub annotation                                  |
|-------------------|--------------------------------------------------|
| `#[pyclass]`      | `pyo3_stub_gen::derive::gen_stub_pyclass`        |
| enum `#[pyclass]` | `pyo3_stub_gen::derive::gen_stub_pyclass_enum`   |
| `#[pymethods]`    | `pyo3_stub_gen::derive::gen_stub_pymethods`      |
| `#[pyfunction]`   | `pyo3_stub_gen::derive::gen_stub_pyfunction`     |

**Placement rules:**

- On structs and enums, use `#[cfg_attr(feature = "python", ...)]` and place the stub
  annotation directly below the `pyo3::pyclass` attribute.
- On `#[pymethods]` impl blocks, place `#[pyo3_stub_gen::derive::gen_stub_pymethods]`
  directly below `#[pymethods]`.
- On functions, place the stub annotation directly above `#[pyfunction]`, after any doc
  comments. Fully qualify the path rather than importing it.

```rust
/// Converts a list of `Bar` into Arrow IPC bytes.
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[pyfunction(name = "bars_to_arrow")]
pub fn py_bars_to_arrow(data: Vec<Bar>) -> PyResult<Py<PyBytes>> {
    // ...
}
```

```rust
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl AccountState {
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(values: &Bound<'_, PyDict>) -> PyResult<Self> {
        // ...
    }
}
```

**Module parameter:** set `module = "nautilus_trader.<package>"` to match the package where Python
imports the type. Use `nautilus_trader.model` for model types and `nautilus_trader.serialization`
for serialization functions.

**Cargo.toml:** add `pyo3-stub-gen` as an optional dependency and include it in the
`python` feature list:

```toml
[features]
python = ["pyo3", "pyo3-stub-gen"]

[dependencies]
pyo3-stub-gen = { workspace = true, optional = true }
```

### Generated Python artifacts

The v2 Python surface commits two generated artifact types:

- Python type stubs under `python/nautilus_trader/**/*.pyi`.
- PyO3 wrapper doc comments under `crates/**/src/python/**/*.rs`.

Run the generator with one command:

```bash
make py-stubs-v2
```

Run it after changing any Python-exposed Rust surface: `#[pyclass]`, `#[pymethods]`,
`#[pyfunction]`, stub annotations, doc comments on wrapped core items, or adapter feature wiring.
Commit every generated `.pyi` file and wrapper doc comment changed by the target with the source
change. CI fails when committed output does not match regeneration. `make build-debug-v2` also
regenerates these artifacts, but use `make py-stubs-v2` when you only need stubs and docstrings.

Wrapper `///` docs under `crates/**/src/python/**` are generated by
`python/generate_docstrings.py` from the core Rust item docs. Do not hand-edit them. Edit the core
docs and run `make py-stubs-v2`. The sync applies these transforms:

- `# Errors` and `# Safety` sections are copied as-is.
- `# Panics` sections are dropped before they reach the Python API.
- Intra-doc links are stripped.
- Rust paths written with `::` become Python-style `.` paths.

The v2 target uses the uv version pinned by `required-version = "==0.11.29"` in
`python/pyproject.toml`. If your local `uv` differs, `make sync-v2`, `make py-stubs-v2`, and
`make build-debug-v2` fail before sync with the required version and update command. Run the
`uv self update --version ...` command printed by the preflight, or prepend a matching `uv`
binary to `PATH`.

Stub generation must compile the same optional Python surface that wheel builds expose.
`python/generate_stubs.py` strips `extension-module` before running cargo, so features enabled only
by `extension-module` in wheel builds must be appended explicitly in that script. Interactive
Brokers uses this rule by appending `nautilus-interactive-brokers/gateway`, which keeps
`DockerizedIBGateway` and `ContainerStatus` in the generated stubs.

The post-processor handles `py_` prefix stripping, `@property`/`@staticmethod`/`@classmethod`
decoration, keyword escaping, deduplication, and ruff formatting.

### Constructor patterns

Use the `new()` vs `new_checked()` convention consistently:

```rust
/// Creates a new [`Symbol`] instance with correctness checking.
///
/// # Errors
///
/// Returns an error if `value` is not a valid string.
///
/// # Notes
///
/// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
pub fn new_checked<T: AsRef<str>>(value: T) -> CorrectnessResult<Self> {
    // Implementation
}

/// Creates a new [`Symbol`] instance.
///
/// # Panics
///
/// Panics if `value` is not a valid string.
pub fn new<T: AsRef<str>>(value: T) -> Self {
    Self::new_checked(value).expect_display(FAILED)
}
```

Always use the `FAILED` constant for `.expect_display()` messages on
`CorrectnessResult`, and import the trait that provides it:

```rust
use nautilus_core::correctness::{CorrectnessResult, CorrectnessResultExt, FAILED};
```

#### Fluent builders for many-optional constructors

Types with large constructors dominated by optional fields (the `instruments`
domain types) also expose a fluent `bon` builder, so callers set only the fields
they need instead of passing a long run of `None`. Put `#[bon::bon]` on the
inherent impl and add a builder method that delegates to `new_checked`, which
keeps a single validated construction path:

```rust
#[bon::bon]
impl CryptoPerpetual {
    // new_checked / new as above

    /// Returns a fluent builder for a [`CryptoPerpetual`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if any input validation fails (see [`CryptoPerpetual::new_checked`]).
    #[builder(start_fn = builder, finish_fn = build)]
    pub fn build_checked(/* same parameters as new_checked */) -> CorrectnessResult<Self> {
        Self::new_checked(/* forward verbatim */)
    }
}
```

Callers write `CryptoPerpetual::builder().instrument_id(..)..build()?`. Required
(non-`Option`) parameters are enforced at compile time by bon's typestate;
`Option` parameters are omittable and default exactly as `new_checked` applies
them. `build()` returns the same `CorrectnessResult` as `new_checked`, so every
correctness check still runs. Unlike the test-only event specs, this builder lives
on the production type, ships in production builds, and returns a `Result` rather
than the value. Keep `new()` and `new_checked()` in place; the builder is additive.

### Type conversion patterns

Use `FromStr` for string parsing and `TryFrom` when a conversion can fail. Implement `From` only
for conversions that cannot fail.

```rust
impl FromStr for Symbol {
    type Err = SymbolParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // parsing logic
    }
}
```

Some existing domain types have generic `From<T: AsRef<str>>` implementations that panic on invalid
input. Treat these as compatibility surfaces and do not copy the pattern into new APIs. Use
`FromStr`, `.parse()`, or `TryFrom` when the input is not already validated.

### Domain numeric types

Use decimal values for discrete financial quantities:

| Value                                    | Type and construction                                      |
|------------------------------------------|------------------------------------------------------------|
| Price or quantity.                       | `Price::from_decimal_dp` or `Quantity::from_decimal_dp`.   |
| Money, fee, margin, or balance.          | `Decimal`, then `Money::from_decimal` or `Money::zero`.    |
| Continuous signal ratio or timing curve. | `f64` when decimal precision has no domain meaning.        |

In tests, compare `.as_decimal()` with `dec!(value)`. Do not convert financial values to `f64` for
assertions. Legacy float constructors remain in the codebase, but use decimal paths in new code.

### Constants and naming conventions

Use SCREAMING_SNAKE_CASE for constants with descriptive names:

```rust
/// Number of nanoseconds in one second.
pub const NANOSECONDS_IN_SECOND: u64 = 1_000_000_000;

/// Bar specification for 1-minute last price bars.
pub const BAR_SPEC_1_MINUTE_LAST: BarSpecification = BarSpecification {
    step: NonZero::new(1).unwrap(),
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
```

### Hash collections

Choose a hash collection by determinism, trust boundary, performance, and access pattern:

| Requirement                                      | Collection                     |
|--------------------------------------------------|--------------------------------|
| Observable insertion‑order iteration.            | `IndexMap` or `IndexSet`.      |
| Hot lookup with no observable iteration order.   | `AHashMap` or `AHashSet`.      |
| Untrusted keys or a network‑facing boundary.     | `HashMap` or `HashSet`.        |
| External API requires a standard collection.     | `HashMap` or `HashSet`.        |
| Simple, non‑critical storage.                    | `HashMap` or `HashSet`.        |

#### Iteration-order determinism

`AHash` randomizes its hasher per process, so its iteration order varies between runs. Use
`IndexMap` or `IndexSet` when iteration feeds observable state, including emitted events, returned
sequences, random number consumption, or downstream effects.

```rust
use indexmap::{IndexMap, IndexSet};

let mut commissions: IndexMap<Currency, Money> = IndexMap::new();
let mut subscribed: IndexSet<InstrumentId> = IndexSet::new();
```

The `check-dst-conventions` hook enforces this rule on audited DST paths. Review other sites against the
[DST determinism contract](../concepts/dst.md#determinism-contract).

#### Performance

For lookup-heavy hot paths where iteration order is not observable, use `AHashMap` or `AHashSet`:

```rust
use ahash::{AHashMap, AHashSet};

let mut symbols: AHashSet<Symbol> = AHashSet::new();
let mut prices: AHashMap<InstrumentId, Price> = AHashMap::new();
```

`AHashMap` is non-cryptographic. Do not use it where untrusted keys make hash-flooding resistance
part of the security boundary.

Benchmarks live in `crates/core/benches/hash_map.rs`. Re-run them before making claims that depend on
hardware or toolchain details.

For removal from `IndexMap`:

- Use `shift_remove` when insertion order must remain stable.
- Use `swap_remove` when order no longer matters.

### Thread-safe hash map patterns

`Arc<AHashMap<K, V>>` supports shared reads, not mutation. Safe Rust rejects mutation through an
`Arc` unless the value provides interior mutability.

| Access pattern                               | Collection                                                     |
|----------------------------------------------|----------------------------------------------------------------|
| Single‑threaded reads and writes.            | `AHashMap<K, V>`.                                              |
| Shared, immutable after construction.        | `Arc<AHashMap<K, V>>`.                                         |
| Shared reads and writes to independent keys. | `Arc<DashMap<K, V>>`.                                          |
| Shared state with cross‑key invariants.      | `Arc<RwLock<AHashMap<K, V>>>` or `Arc<Mutex<AHashMap<K, V>>>`. |

Choose `RwLock` or `Mutex` when an operation must update several entries atomically. Do not use
`DashMap` guards across `.await` points.

### Shared mutability storage

Code ported from Cython often cloned values out of a container before mutating them.
That pattern produces silent staleness: the local clone diverges from the canonical
entry the moment another code path applies an event to it.

Reach for `Rc<RefCell<T>>` (single-threaded) or `Arc<RwLock<T>>` (multi-threaded)
storage only when all three hold:

- The value is mutated after insertion.
- Multiple holders need to observe each other's writes.
- A handle must outlive the container's borrow scope.

Orders in `Cache` use this shape internally for per-key borrow tracking. Storage
is `AHashMap<ClientOrderId, SharedCell<OrderAny>>`; the smart-pointer leak stays
internal. Public accessors return scoped newtypes that hide it: `Cache::order`
returns `OrderRef<'_>` (read borrow), `Cache::order_mut` returns `OrderRefMut<'_>`
(exclusive write borrow, requires `&mut Cache`), and `Cache::order_owned` returns
an owned `OrderAny` snapshot when a value must cross a boundary. Use
`Cache::try_order` or `Cache::try_order_owned` when a missing order is an error;
they return `OrderLookupError` instead of forcing each caller to build an ad hoc
not-found error. Engines drop the borrow before dispatching events and re-read
the cache for post-event state, which keeps the dispatch a clean transaction boundary.

`Cache::order_mut` takes `&mut Cache`, which means strategies and adapters
receiving a `CacheView` (which only exposes immutable cache borrows) cannot reach
it. Order mutation is reserved for the data and execution engines that hold the
cache directly; the type system enforces that contract.

Otherwise prefer the simpler shape:

- Read-mostly and set once: `Rc<T>` or `Arc<T>` (no interior mutability).
- Owned snapshots suffice for callers: store `T`, clone on read.
- Single owner, no mutation: plain field.

Costs of `Rc<RefCell<T>>` worth weighing before adopting it:

- Every access pays a runtime borrow check.
- The smart-pointer type leaks at write boundaries.
- Misuse panics at runtime instead of failing to compile.
- `Rc<RefCell<T>>` is `!Send`; cross-thread storage needs `Arc<RwLock<T>>`
  (or `Arc<Mutex<T>>` when reads are rare).

**Decision tree:**

1. Mutable, multi-observer, and handle outlives container borrow?
   - Single-threaded: `Rc<RefCell<T>>`.
   - Multi-threaded: `Arc<RwLock<T>>` (or `Arc<Mutex<T>>` when reads are rare).
2. Read-mostly and set once: `Rc<T>` or `Arc<T>`.
3. Owned snapshots fine: store `T`, clone on read.
4. Single owner, no mutation: plain field.

### Re-export patterns

Organize re-exports alphabetically and place at the end of lib.rs files:

```rust
pub use crate::{
    nanos::UnixNanos,
    time::AtomicTime,
    uuid::UUID4,
};
```

### Documentation standards

Use the indicative mood for doc comments: "Returns the account ID", not "Return the account ID".

#### Section header casing

Rustdoc section headers use Title Case, matching the Rust standard library convention:

- `# Examples`
- `# Errors`
- `# Panics`
- `# Safety`
- `# Notes`
- `# Thread Safety`
- `# Feature Flags`

#### Module-level documentation

Add module-level documentation to public modules and modules with a non-obvious contract. Do not add
boilerplate documentation to private leaf modules.

```rust
//! Functions for correctness checks similar to the *design by contract* philosophy.
//!
//! This module provides validation checking of function or method conditions.
//!
//! A condition is a predicate which must be true just prior to the execution of
//! some section of code - for correct behavior as per the design specification.
```

For modules with feature flags, document them clearly:

```rust
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case:
//!
//! - `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds as a Python extension module (used with `python`).
//! - `stubs`: Enables type stubs for use in testing scenarios.
```

#### Field documentation

Document public fields when neighboring fields are documented. Use terminating periods and keep the
density consistent within the type. Do not add doc comments to private fields; put important context
in the type-level documentation instead.

```rust
pub struct Currency {
    /// The currency code as an alpha-3 string (e.g., "USD", "EUR").
    pub code: Ustr,
    /// The currency decimal precision.
    pub precision: u8,
    /// The ISO 4217 currency code.
    pub iso4217: u16,
    /// The full name of the currency.
    pub name: Ustr,
    /// The currency type, indicating its category (e.g. Fiat, Crypto).
    pub currency_type: CurrencyType,
}
```

#### Function documentation

Document all public functions with:

- Purpose and behavior.
- Input usage when it is not clear from the type and name.
- Error conditions when the function returns `Result`.
- Panic conditions when the function can panic.

```rust
/// Returns a reference to the `AccountBalance` for the specified currency, or `None` if absent.
///
/// # Panics
///
/// Panics if `currency` is `None` and `self.base_currency` is `None`.
pub fn base_balance(&self, currency: Option<Currency>) -> Option<&AccountBalance> {
    // Implementation
}
```

#### Errors and panics documentation format

For single-line error and panic documentation, use sentence case:

```rust
/// Returns a reference to the `AccountBalance` for the specified currency, or `None` if absent.
///
/// # Errors
///
/// Returns an error if the currency conversion fails.
///
/// # Panics
///
/// Panics if `currency` is `None` and `self.base_currency` is `None`.
pub fn base_balance(&self, currency: Option<Currency>) -> anyhow::Result<Option<&AccountBalance>> {
    // Implementation
}
```

For multi-line error and panic documentation, use bullets with terminating periods:

```rust
/// Calculates the unrealized profit and loss for the position.
///
/// # Errors
///
/// Returns an error if:
/// - The market price for the instrument cannot be found.
/// - The conversion rate calculation fails.
/// - Invalid position state is encountered.
///
/// # Panics
///
/// This function panics if:
/// - The instrument ID is invalid or uninitialized.
/// - Required market data is missing from the cache.
/// - Internal state consistency checks fail.
pub fn calculate_unrealized_pnl(&self, market_price: Price) -> anyhow::Result<Money> {
    // Implementation
}
```

#### Safety documentation format

Use a `# Safety` section to state the caller's obligations for an unsafe function. Put a `SAFETY:`
comment immediately above each unsafe operation and explain why the operation satisfies those obligations.

```rust
/// Creates a new instance from raw components without validation.
///
/// # Safety
///
/// The caller must ensure that all input parameters are valid and properly initialized.
pub unsafe fn from_raw_parts(ptr: *const u8, len: usize) -> Self {
    // SAFETY: The caller guarantees that `ptr` is valid for `len` bytes.
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    Self { data }
}
```

## Python bindings

Python bindings use [PyO3](https://pyo3.rs), allowing users to import NautilusTrader crates directly
in Python without a Rust toolchain.

### PyO3 naming conventions

When exposing Rust functions and types through PyO3:

- Prefix Rust function symbols with `py_`.
- Use `#[pyo3(name = "...")]` to publish the Python function name without the prefix.
- Name Python-facing wrapper types with a `Py` prefix and publish the Python type without it. Reserve
  a `PyTypeInner` name for backing state that the wrapper owns; do not expose it as a pyclass.
- Use `nautilus_trader.adapters.<adapter_name>` for public adapter stub metadata. Runtime module
  paths may use `nautilus_trader.core.nautilus_pyo3.<adapter_name>`.
- Convert standard Python exceptions with `to_pyvalue_err`, `to_pytype_err`, `to_pyruntime_err`,
  `to_pykey_err`, `to_pyexception`, and `to_pynotimplemented_err` from `nautilus_core::python`.

```rust
#[pyo3(name = "do_something")]
pub fn py_do_something() -> PyResult<()> {
    // ...
}
```

:::info[Automated enforcement]
The `check_pyo3_conventions.sh` pre-commit hook enforces the `py_` prefix for PyO3 functions.
:::

### PyO3 enum conventions

Enums exposed to Python should use the following `pyclass` attributes:

- `frozen`: enums are immutable value types.
- `eq, eq_int`: enables equality with other enum instances and integer discriminants.
- `rename_all = "SCREAMING_SNAKE_CASE"`: standardizes Python variant names.
- `from_py_object`: enables conversion from Python objects.

:::warning[Do not use the `hash` pyclass attribute with `eq_int` enums]
PyO3's auto-generated `__hash__` uses Rust's `DefaultHasher`, which produces different values
than Python's `hash()` on the equivalent integer. Since `eq_int` makes `MyEnum.VARIANT == 1`
true, the hash contract (`a == b` implies `hash(a) == hash(b)`) would be violated. Instead,
provide a manual `__hash__` returning the discriminant directly:
:::

```rust
#[pymethods]
impl MyEnum {
    const fn __hash__(&self) -> isize {
        *self as isize
    }
}
```

### Testing conventions

- Use `mod tests` as the standard test module name unless you need to specifically compartmentalize.
- Use `#[rstest]` instead of `#[test]`, including for non-parameterized tests.
- Use `#[tokio::test]` for non-parameterized async tests.
- Keep `#[cfg(test)]` on test modules and test-only files. Do not add test behavior to production code.
- Store JSON fixtures under the crate's `test_data/` directory and load them with `include_str!`.
- Compare prices, quantities, and money through `.as_decimal()` and `dec!(value)`.
- Do not use Arrange, Act, Assert separator comments.

:::info[Automated enforcement]
The `check_testing_conventions.sh` pre-commit hook enforces the use of `#[rstest]` over `#[test]`.
:::

#### Parameterized testing

Use the `rstest` attribute consistently, and for parameterized tests:

```rust
#[rstest]
#[case("AUDUSD", false)]
#[case("AUD/USD", false)]
#[case("CL.FUT", true)]
fn test_symbol_is_composite(#[case] input: &str, #[case] expected: bool) {
    let symbol = Symbol::new(input);
    assert_eq!(symbol.is_composite(), expected);
}
```

#### Test specs (bon builders)

For events with many constructor arguments, the canonical test builder is a
fluent spec defined alongside the event under `events/<event>/spec/<name>.rs`
(see `crates/model/src/events/order/spec/filled.rs` for the reference
implementation). Gate the spec module with
`#[cfg(any(test, feature = "stubs"))]` so it is available to in-crate tests
and to downstream crates that opt in with the `stubs` feature, but compiled
out of production builds. Specs must not be referenced from production code.

Why a custom spec instead of `derive_builder::Builder` with `builder(default)`:
the latter bypasses the production constructor, so invariants added later are
not exercised by tests. A spec funnels through the production constructor on every `build()`.

Anatomy:

- Derive `bon::Builder` with `finish_fn = into_spec` so the generated finish
  method does not collide with the custom `build()`.
- Mark every required field `#[builder(default = ...)]` with a literal or a
  `TestDefault::test_default()` call. Leave optional fields as `Option<T>`
  without a default so callers either set them or accept `None`.
- Default event ID fields to `test_uuid()` from `crate::stubs`. This yields
  distinct, reproducible UUIDs without callers managing state.
- Implement `build()` on the generated builder so it calls `into_spec()` and
  forwards through the production constructor (e.g. `OrderFilled::new`). The
  return type is the event itself, not a `Result`, because spec defaults are
  valid by construction.

Caller usage:

```rust
let fill = OrderFilledSpec::builder()
    .last_qty(Quantity::from(50_000))
    .trade_id(TradeId::from("TRADE-1"))
    .build();
```

Override only the fields the test cares about; the rest take spec defaults.
Do not write `.unwrap()` after `build()`.

Determinism: under `cargo nextest` each test runs in a fresh process, so the
per-thread UUID sequence resets automatically. Under plain `cargo test`, call
`reset_test_uuid_rng()` from `crate::stubs` at the start of any test that
compares UUID sequences across draws.

Pin spec defaults with a single test in the spec module so accidental drift
in any field surfaces there rather than as silent behavior change in downstream tests.

#### Property-based testing

Use the `proptest` crate for property-based tests. Place these in a separate
`property_tests` module (not inside `mod tests`) to keep deterministic unit
tests separate from randomized property tests:

```rust
#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    fn my_strategy() -> impl Strategy<Value = MyType> {
        prop_oneof![
            Just(MyType::VariantA),
            Just(MyType::VariantB),
        ]
    }

    fn value_strategy() -> impl Strategy<Value = f64> {
        prop_oneof![
            -1000.0..1000.0,
            Just(0.0),
        ]
    }

    proptest! {
        #[rstest]
        fn prop_construction_roundtrip(
            value in value_strategy(),
            variant in my_strategy()
        ) {
            // The constructed value must preserve `value` and `variant`.
        }
    }
}
```

Conventions:

- Name the module `property_tests`, separate from `mod tests`.
- Import `proptest::prelude::*` and `rstest::rstest`.
- Define strategy functions returning `impl Strategy<Value = T>`.
- Combine value ranges with edge cases using `prop_oneof!`.
- Filter invalid combinations with `prop_filter_map`.
- Prefix test names with `prop_`.
- Mark each test inside `proptest!` with `#[rstest]`.

#### Test naming

Use descriptive test names that explain the scenario:

```rust
fn test_sma_with_no_inputs()
fn test_sma_with_single_input()
fn test_symbol_is_composite()
```

### Box-style banner comments

Do not use box-style banner or separator comments. If code requires visual
separation, consider splitting it into separate modules or files. Instead use:

- Clear function names that convey purpose.
- Module structure for logical groupings (`mod tests { mod fixtures { } }`).
- Impl blocks to group related methods.
- Doc comments (`///`) for semantic documentation.
- IDE navigation and code folding.

Patterns to avoid:

```rust
// ============================================================================
// Some Section
// ============================================================================

// ========== Test Fixtures ==========
```

## Rust-Python memory management

`Py<T>` owns a reference to a Python object. `Py::clone_ref` and
`nautilus_core::python::clone_py_object` increment the Python reference count while attached to the
interpreter. They provide safe cloning, but they do not break reference cycles.

An additional `Arc<Py<T>>` is usually unnecessary because `Py<T>` already provides shared ownership.
Removing the `Arc` simplifies ownership, but a cycle still exists if the Python object refers back to
the Rust owner.

Use the ownership shape that matches the relationship:

| Relationship                                      | Pattern                                          |
|---------------------------------------------------|--------------------------------------------------|
| Rust owns a Python object with no back‑reference. | Store `Py<T>` and clone with `clone_py_object`.  |
| A pyclass owns other Python objects.              | Implement `__traverse__` and `__clear__`.        |
| The reference must not keep its target alive.     | Use a Python weak reference.                     |
| Ownership crosses threads.                        | Acquire the interpreter before Python API calls. |

For pyclasses, `__traverse__` must visit each owned Python reference and `__clear__` must clear
mutable references that can participate in a cycle. Do not attach to the interpreter from
`__traverse__`; PyO3 prohibits it while the garbage collector is traversing objects. See
[PyO3 garbage collector integration](https://pyo3.rs/v0.29.0/class/protocols.html#garbage-collector-integration).

## Design by contract

Design by contract states the obligations between a function and its callers:

- **Preconditions**: what the function requires from callers.
- **Postconditions**: what the function guarantees in return.
- **Invariants**: what properties its type maintains across calls.

Prefer the type system first. Ownership, lifetimes, `Send`/`Sync`, `Result`/`Option`,
exhaustive matching, newtypes, and visibility encode most contracts at compile time
and cost nothing at runtime. Use runtime checks only where the type system cannot.

For most preconditions, use the `nautilus_core::correctness` module: it is the
project's design-by-contract mechanism and should be the default. `check_*`
functions (`check_predicate_true`, `check_valid_string_ascii`,
`check_positive_u64`, `check_in_range_inclusive_f64`, `check_equal_usize`,
`check_key_in_map`, ...) return a typed `CorrectnessResult<()>` whose
`CorrectnessError` variants name each kind of violation. Pair `new_checked()` (fallible, returns
`CorrectnessResult`) with a `new()` wrapper that panics via
`.expect_display(FAILED)` for validated types; this is the
[Constructor patterns](#constructor-patterns) convention and produces panic
messages prefixed with `Condition failed: ...`.

Use `debug_assert!` (and `debug_assert_eq!`/`_ne!`) for *internal* invariants the
correctness module does not model: field relationships, monotonic sequences, CAS
postconditions, encode/decode round-trips, provably in-range indices, and
preconditions on internal helpers that trusted upstream validation. Release builds
strip the check, so never use `debug_assert!` for public API input. For `unsafe`
code, use always-on `assert!` for soundness-critical preconditions (null,
alignment, provenance) and reserve `debug_assert!` for hot-path preconditions
upheld by design.

Choosing a mechanism:

| Situation                                      | Use                                               |
|------------------------------------------------|---------------------------------------------------|
| Public API precondition.                       | `check_*` from `nautilus_core::correctness`.      |
| Validated constructor.                         | `new_checked()` and `new()`.                      |
| Recoverable parse, I/O, or network error.      | `Result<T, DomainError>`.                         |
| Internal invariant the compiler cannot prove.  | `debug_assert!`.                                  |
| Always‑on internal invariant.                  | `assert!`.                                        |
| Soundness‑critical unsafe precondition.        | `assert!` (always on).                            |
| Hot‑path unsafe precondition upheld by design. | `debug_assert!` and a documented `Safety` clause. |

Style:

- Prefix `debug_assert!` messages with `Invariant:` and state the positive rule,
  not the failure: `debug_assert!(next > last, "Invariant: time is strictly monotonic across CAS")`.
- `Condition failed: ...` (from the `FAILED` constant) marks a caller-supplied
  input violation; `Invariant: ...` marks an internal contract bug.
- Place assertions where the invariant is first assumed. When an invariant holds
  across a hot loop, assert once at the boundary rather than inside the loop.

## Common anti-patterns

- Avoid `.clone()` in hot paths; favor borrowing or shared ownership through `Arc`.
- Avoid `.unwrap()` in production code. Propagate or map recoverable errors. Unwrapping lock
  poisoning is acceptable when it represents an unrecoverable program state.
- Avoid `String` when `&str` suffices, especially on hot paths.
- Avoid exposing interior mutability. Hide locks and `RefCell` behind safe APIs.
- Avoid large error variants. Box large payloads when they materially increase `Result<T, E>` size.

## Unsafe Rust

NautilusTrader uses `unsafe` where FFI and low-level storage require contracts that safe Rust cannot
express. Each unsafe operation shifts a specific proof obligation from the compiler to the code and
its reviewers. See the Rust Reference for
[behavior considered undefined](https://doc.rust-lang.org/stable/reference/behavior-considered-undefined.html).

### Safety policy

Any use of unsafe Rust must follow this policy:

- Give each unsafe function a `# Safety` section that states the caller's complete obligations.
- Put a `SAFETY:` comment above each unsafe operation and explain why its preconditions hold.
- Add targeted tests for observable behavior around unsafe code. Tests support, but do not establish,
  the soundness proof.
- Every crate that exposes FFI symbols enables
  `#![deny(unsafe_op_in_unsafe_fn)]`. Even inside an `unsafe fn`, each pointer dereference or
  other unsafe operation must be wrapped in its own `unsafe { ... }` block.
- For raw vectors that cross the FFI boundary, follow the
  [FFI memory contract](ffi.md). Foreign code becomes the owner of the allocation and must
  call the matching `vec_drop_*` function exactly once.

### Categories of unsafe code

The codebase uses unsafe Rust for these purposes:

- FFI boundaries that operate on raw pointers. See [FFI](ffi.md).
- `UnsafeCell` storage with enforced aliasing and lifetime invariants.
- Unsafe `Send` or `Sync` implementations whose full reachable state satisfies the trait contract.

### Unsafe Send/Sync requirements

`Send` and `Sync` have different obligations:

| Trait  | Safe code may                                              |
|--------|------------------------------------------------------------|
| `Send` | Move ownership of the value to another thread.             |
| `Sync` | Share `&T` between threads.                                |

An unsafe implementation must make every permitted safe use sound. The proof covers all reachable
state, aliases, callbacks, generic parameters, safe methods, cloning, and destruction. Documentation
cannot transfer this obligation to safe callers.

Do not implement `Send` or `Sync` for a thread-affine type if moving or sharing it can cause undefined
behavior. Keep the type non-thread-safe, replace its state with thread-safe primitives, or expose a
separate command handle backed by channels or atomics.

### Defense in depth

Where unsafe code relies on invariants, add defense mechanisms:

- Verify types before casting, for example with `TypeId`.
- Use RAII guards for cleanup on return and panic paths.
- Use always-on checks when failure would violate soundness.
- Use debug assertions only as diagnostics for invariants already upheld by design. They cannot
  enforce soundness because release builds remove them.

### Runtime invariants

Several core subsystems rely on runtime invariants rather than compile-time
guarantees. Tests verify the first three contracts below. The guard usage
rules are enforced by convention. Any PR that touches `UnsafeCell`,
registries, `unsendable`, or live-node threading should confirm the
invariant tests still pass.

#### Thread-local registries

The actor registry, component registry, and message bus each use
`thread_local!` storage. An object registered on one thread is never visible
from another. The live node event loop runs on a single thread, and all
registry and message bus access happens on that thread.

`LiveNodeHandle` is the only intended cross-thread control surface. It uses
`Arc<AtomicBool>` for stop signaling and `Arc<AtomicU8>` for state, both with `Ordering::Relaxed`.

#### Actor registry vs component registry

Both registries store `Rc<UnsafeCell<dyn Trait>>` in thread-local maps but
differ in how they handle aliased access:

| Property          | Actor registry                     | Component registry                 |
|-------------------|------------------------------------|------------------------------------|
| Aliasing          | Allowed (multiple guards).         | Prevented (`BorrowGuard` + set).   |
| Re‑entrant access | Yes, required for callbacks.       | No, lifecycle ops are sequential.  |
| Error handling    | Panic or `None` on lookup failure. | Returns `anyhow::Result` on error. |
| Guard type        | `ActorRef<T>` (Rc‑backed).         | Stack‑local `BorrowGuard`.         |

The actor registry chooses re-entrant access over aliasing prevention because
message handlers frequently call back into the registry to look up other
actors. The component registry can enforce strict aliasing because lifecycle
operations (start, stop, reset, dispose) are non-re-entrant.

#### `ActorRef` usage rules

`ActorRef` guards must be:

- Obtained and dropped within a single synchronous scope.
- Never stored in a struct field.
- Never held across an `.await` point.
- Never sent to another thread.

The canonical pattern captures an actor's `Ustr` ID in a closure and looks
up the actor each time the callback fires:

```rust
let actor_id = actor.actor_id().inner();
let handler = TypedHandler::from(move |quote: &QuoteTick| {
    if let Some(mut actor) = try_get_actor_unchecked::<MyActor>(&actor_id) {
        actor.handle_quote(quote);
    }
});
```

## Tooling configuration

The repository combines standard Rust tools with project-specific pre-commit checks:

| Area                       | Source of truth                                                 |
|----------------------------|-----------------------------------------------------------------|
| Formatting and imports.    | `rustfmt.toml`.                                                 |
| Workspace lints.           | `Cargo.toml` and `clippy.toml`.                                 |
| Rust layout conventions.   | `.pre-commit-hooks/check_formatting_rs.sh`.                     |
| Nautilus type conventions. | `.pre-commit-hooks/check_nautilus_conventions.sh`.              |
| Tokio and DST usage.       | `check_tokio_usage.sh` and `check_dst_conventions.sh` hooks.    |
| PyO3 bindings.             | `.pre-commit-hooks/check_pyo3_conventions.sh`.                  |

Every workspace crate inherits the workspace lints through `[lints] workspace = true`. When
suppressing `missing_panics_doc` or `missing_errors_doc`, include a `reason` that explains why the
lint does not apply:

```rust
#[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
```

Use `cbindgen` to generate C headers for FFI. Do not edit generated headers directly.

## Rust version management

The project pins to a specific Rust version via `rust-toolchain.toml`.

Install the pinned toolchain and verify the active override:

```bash
rustup toolchain install "$(bash scripts/rust-toolchain.sh)"
rustup show active-toolchain
```

If pre-commit passes locally but fails in CI, clear the prek cache and re-run:

```bash
prek clean
make pre-commit
```

These commands restore the Rust and Clippy versions used by CI.

## Cap'n Proto serialization

The `nautilus-serialization` crate provides optional Cap'n Proto serialization. The feature remains
opt-in so standard builds do not require the compiler.

### Installing Cap'n Proto

Install the Cap'n Proto compiler before working with schemas. The required version is
specified in `tools.toml` in the repository root.

See [Environment setup](environment_setup.md#capn-proto) for platform-specific instructions.

:::warning
Ubuntu's default `capnproto` package is too old. Linux users must install from source.
:::

Verify installation:

```bash
capnp --version
```

The version must match `tools.toml`.

### Schema development workflow

Schema files live in `crates/serialization/schemas/capnp/`:

- `common/`: base types, identifiers, and enums.
- `commands/`: trading commands.
- `events/`: order and position events.
- `data/`: market data types.

When modifying schemas:

1. Edit the `.capnp` schema file in the appropriate subdirectory.
2. Regenerate Rust bindings:

   ```bash
   make regen-capnp
   ```

3. Review changes:

   ```bash
   git diff crates/serialization/generated/capnp
   ```

4. Update conversions in `crates/serialization/src/capnp/conversions.rs` if needed.
5. Run tests:

   ```bash
   make cargo-test EXTRA_FEATURES="capnp"
   ```

### Generated code

Generated Rust files are checked into `crates/serialization/generated/capnp/` for docs.rs and drift
review. The docs.rs build uses these files because its environment lacks the Cap'n Proto compiler.

Normal builds with the `capnp` feature still require the pinned compiler. `build.rs` compiles the
schemas into `OUT_DIR`; `make regen-capnp` copies that output into the checked-in directory.

### Verifying schema consistency

Before committing schema changes, ensure generated files are up-to-date:

```bash
make check-capnp-schemas
```

This target:

1. Skips with a warning if `capnp` is not installed, which is acceptable for local development.
2. Fails if regeneration errors occur, such as a version mismatch.
3. Regenerates schemas and fails if generated files differ from committed versions.

CI runs this check automatically to catch drift (capnp is always installed in CI).

### Testing with capnp feature

- Run workspace tests: `make cargo-test EXTRA_FEATURES="capnp"`.
- Run serialization crate tests: `make cargo-test-crate-nautilus-serialization`.

### Schema evolution guidelines

When evolving schemas:

- **Additive changes only**: Add new fields at the end.
- **Never remove fields**: Mark deprecated fields in comments.
- **Never reuse field numbers**: Even after deprecation.
- **Test roundtrip compatibility**: Ensure old and new versions interoperate.

Cap'n Proto's evolution rules allow schema changes without breaking binary compatibility, but
you must follow these constraints to maintain forward/backward compatibility.

## Resources

- [The Rustonomicon](https://doc.rust-lang.org/nomicon/).
- [Rust Reference: Unsafety](https://doc.rust-lang.org/stable/reference/unsafety.html).
- [Safe bindings in Rust](https://www.abubalay.com/blog/2020/08/22/safe-bindings-in-rust).
- [Rust and C interoperability](https://www.chromium.org/Home/chromium-security/memory-safety/rust-and-c-interoperability/).
