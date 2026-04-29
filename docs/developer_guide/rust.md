# Rust

The [Rust](https://www.rust-lang.org/learn) programming language is an ideal fit for implementing the mission-critical core of the platform and systems.
Its strong type system, ownership model, and compile-time checks eliminate memory errors and data races by construction,
while zero-cost abstractions and the absence of a garbage collector deliver C-like performance, important for high-frequency trading workloads.

## Cargo manifest conventions

- In `[dependencies]`, list internal crates (`nautilus-*`) first in alphabetical order, insert a blank line, then external required dependencies alphabetically, followed by another blank line and the optional dependencies (those with `optional = true`) in alphabetical order. Preserve inline comments with their dependency.
- Add `"python"` to every `extension-module` feature list that builds a Python artefact, keeping it adjacent to `"pyo3/extension-module"` so the full Python stack is obvious.
- When a manifest groups adapters separately (for example `crates/pyo3`), keep the `# Adapters` block immediately below the internal crate list so downstream consumers can scan adapter coverage quickly.
- Always include a blank line before `[dev-dependencies]` and `[build-dependencies]` sections.
- Apply the same layout across related manifests when the feature or dependency sets change to avoid drift between crates.
- Use snake_case filenames for `bin/` sources (for example `bin/ws_data.rs`) and reflect those paths in each `[[bin]]` section.
- Keep `[[bin]] name` entries in kebab-case (for example `name = "hyperliquid-ws-data"`) so the compiled binaries retain their intended CLI names.

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
  - `high-precision`: switches the value-type backing (64-bit or 128-bit integers) to support domains that require extra precision.
  - `default = []`: keep defaults minimal.
  - `python`: enables Python bindings.
  - `extension-module`: builds a Python extension module (always include `python`).
  - `ffi`: enables C FFI bindings.
  - `stubs`: exposes testing stubs.

## Build configurations

To avoid unnecessary rebuilds during development, align cargo features, profiles, and flags across different build targets.
Cargo's build cache is keyed by the exact combination of features, profiles, and flags. Any mismatch triggers a full rebuild.

### Aligned targets (testing and linting)

| Target                      | Features                         | Profile   | `--all-targets` | `--no-deps` | Purpose        |
|-----------------------------|----------------------------------|-----------|-----------------|-------------|----------------|
| `cargo-test`                | `ffi,python,high-precision,defi` | `nextest` | ✓ (implicit)    | n/a         | Run tests.     |
| `cargo-clippy` (pre‑commit) | `ffi,python,high-precision,defi` | `nextest` | ✓               | n/a         | Lint all code. |

These targets share the same feature set and profile, allowing cargo to reuse compiled artifacts between linting and testing without rebuilds.
The `nextest` profile is used to align with the workflow of the majority of core maintainers who use cargo-nextest for running tests.

### Documentation builds

Documentation is built separately using `make docs-rust`, which runs:

```bash
cargo +nightly doc --all-features --no-deps --workspace
```

This uses the nightly toolchain and `--all-features` rather than the aligned feature set above, so it does not share build artifacts with testing/linting.

### Separate target (Python extension building)

| Target        | Features                             | Profile   | Notes |
|---------------|--------------------------------------|-----------|-------|
| `build`       | Includes `extension-module` + subset | `release` | Requires different features for PyO3 extension module. |
| `build-debug` | Includes `extension-module` + subset | `dev`     | Requires different features for PyO3 extension module. |

Python extension building intentionally uses different features (`extension-module` is required) and will trigger rebuilds. This is expected and unavoidable.

### Rebuild triggers to avoid

Mismatches in any of these cause full rebuilds:

- Different feature combinations (e.g., `--features "a,b"` vs `--features "a,c"`).
- Different `--no-default-features` usage (enables/disables default features).
- Different profiles (e.g., `dev` vs `nextest` vs `release`).

When adding new build targets or modifying existing ones, maintain alignment with the testing/linting group to preserve fast incremental builds.

## Module organization

- Keep modules focused on a single responsibility.
- Use `mod.rs` as the module root when defining submodules.
- Prefer relatively flat hierarchies over deep nesting to keep paths manageable.
- Re-export commonly used items from the crate root for convenience.

## Code style and conventions

### File header requirements

All Rust files must include the standardized copyright header:

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

Import formatting is automatically handled by rustfmt when running `make format`.
The tool organizes imports into groups (standard library, external crates, local imports) and sorts them alphabetically within each group.

Within this section, follow these spacing rules:

- Leave **one blank line between functions** (including tests) – this improves readability and
mirrors the default behavior of `rustfmt`.
- Leave **one blank line above every doc comment** (`///` or `//!`) so that the comment is clearly
  detached from the previous code block.

#### String formatting

Prefer inline format strings over positional arguments:

```rust
// Preferred - inline format with variable names
anyhow::bail!("Failed to subtract {n} months from {datetime}");

// Instead of - positional arguments
anyhow::bail!("Failed to subtract {} months from {}", n, datetime);
```

This makes messages more readable and self-documenting, especially when there are multiple variables.

### Type qualification

Follow these conventions for qualifying types in code:

- **anyhow**: Always fully qualify `anyhow` macros (`anyhow::bail!`, `anyhow::anyhow!`) and the Result type (`anyhow::Result<T>`).
- **Nautilus domain types**: Do not fully qualify Nautilus domain types. Use them directly after importing (e.g., `Symbol`, `InstrumentId`, `Price`).
- **tokio**: Generally fully qualify `tokio` types as they can have equivalents in std library and other crates (e.g., `tokio::spawn`, `tokio::time::timeout`).

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

- Fully qualify logging macros so the backend is explicit:
  - Use `log::…` (`log::debug!`, `log::info!`, `log::warn!`, etc.) for all Rust components.
- Start messages with a capitalised word, prefer complete sentences, and omit terminal periods (e.g. `"Processing batch"`, not `"Processing batch."`).

:::info[Automated enforcement]
The `check_logging_macro_usage.sh` pre-commit hook enforces fully qualified logging macros.
:::

### Error handling

Use structured error handling patterns consistently:

1. **Primary Pattern**: Use `anyhow::Result<T>` for fallible functions:

   ```rust
   pub fn calculate_balance(&mut self) -> anyhow::Result<Money> {
       // Implementation
   }
   ```

2. **Custom Error Types**: Use `thiserror` for domain-specific errors:

   ```rust
   #[derive(Error, Debug)]
   pub enum NetworkError {
       #[error("Connection failed: {0}")]
       ConnectionFailed(String),
       #[error("Timeout occurred")]
       Timeout,
   }
   ```

3. **Error Propagation**: Use the `?` operator for clean error propagation.

4. **Error Creation**: Prefer `anyhow::bail!` for early returns with errors:

   ```rust
   // Preferred - using bail! for early returns
   pub fn process_value(value: i32) -> anyhow::Result<i32> {
       if value < 0 {
           anyhow::bail!("Value cannot be negative: {value}");
       }
       Ok(value * 2)
   }

   // Instead of - verbose return statement
   if value < 0 {
       return Err(anyhow::anyhow!("Value cannot be negative: {value}"));
   }
   ```

   **Note**: Use `anyhow::bail!` for early returns, but `anyhow::anyhow!` in closure contexts like `ok_or_else()` where early returns aren't possible.

5. **Error Context**: Use lowercase for `.context()` messages to support error chaining (except proper nouns/acronyms):

   ```rust
   // Good - lowercase chains naturally
   parse_timestamp(value).context("failed to parse timestamp")?;

   // Exception - proper nouns stay capitalized
   connect().context("BitMEX websocket did not become active")?;
   ```

:::info[Automated enforcement]
The `check_error_conventions.sh` and `check_anyhow_usage.sh` pre-commit hooks enforce these error handling patterns.
:::

### Async patterns

Use consistent async/await patterns:

1. **Async function naming**: No special suffix is required; prefer natural names.
2. **Tokio usage**: Fully qualify tokio types (e.g., `tokio::time::timeout`). See [Adapter runtime patterns](#adapter-runtime-patterns) for spawn rules.
3. **Error handling**: Return `anyhow::Result` from async functions to match the synchronous conventions.
4. **Cancellation safety**: Call out whether the function is cancellation-safe and what invariants still hold when it is cancelled.
5. **Stream handling**: Use `tokio_stream` (or `futures::Stream`) for async iterators to make back-pressure explicit.
6. **Timeout patterns**: Wrap network or long-running awaits with timeouts (`tokio::time::timeout`) and propagate or handle the timeout error.

### Adapter runtime patterns

Adapter crates (under `crates/adapters/`) require special handling for spawning async tasks due to Python FFI compatibility:

1. **Use `get_runtime().spawn()` instead of `tokio::spawn()`**: When called from Python threads (which have no Tokio context), `tokio::spawn()` panics because it relies on thread-local storage. The global runtime pattern provides an explicit reference accessible from any thread.

   ```rust
   use nautilus_common::live::get_runtime;

   // Correct - works from Python threads
   get_runtime().spawn(async move {
       // async work
   });

   // Incorrect - panics from Python threads
   tokio::spawn(async move {
       // async work
   });
   ```

2. **Use the shorter import path**: Import `get_runtime` from the `live` module re-export, not the full path:

   ```rust
   // Preferred - shorter path via re-export
   use nautilus_common::live::get_runtime;

   // Avoid - unnecessarily verbose
   use nautilus_common::live::runtime::get_runtime;
   ```

3. **Use `get_runtime().block_on()` for sync-to-async bridges**: When synchronous code needs to call async functions in adapters:

   ```rust
   fn sync_method(&self) -> anyhow::Result<()> {
       get_runtime().block_on(self.async_implementation())
   }
   ```

4. **Install custom runtimes before first use**: Rust-native binaries that own `main()` may call
   `set_runtime()` before `LiveNode::build()` or any adapter/client usage. Build custom runtimes
   with `tokio::runtime::Builder::new_multi_thread().enable_all()`; current-thread runtimes and
   runtimes without I/O or timer drivers do not satisfy adapter assumptions. If the `python` feature
   is enabled, prepare Python before building the runtime or keep the default initializer.

5. **Tests are exempt**: Test code using `#[tokio::test]` creates its own runtime context, so
   `tokio::spawn()` works correctly. The enforcement hook skips test files and test modules.

:::info[Automated enforcement]
The `check_tokio_usage.sh` pre-commit hook enforces these adapter runtime patterns automatically.
:::

### Attribute patterns

Consistent attribute usage and ordering:

```rust
#[repr(C)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model")
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
        module = "nautilus_trader.model",
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
| ----------------- | ------------------------------------------------ |
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

**Module parameter:** set `module = "nautilus_trader.<package>"` to match the Python
package where the type is imported. For example, model types use
`nautilus_trader.model` and serialization functions use
`nautilus_trader.serialization`.

**Cargo.toml:** add `pyo3-stub-gen` as an optional dependency and include it in the
`python` feature list:

```toml
[features]
python = ["pyo3", "pyo3-stub-gen"]

[dependencies]
pyo3-stub-gen = { workspace = true, optional = true }
```

**Regenerating stubs:** run `make py-stubs-v2` (or `python python/generate_stubs.py`)
after changing annotations. The post-processor handles `py_` prefix stripping,
`@property`/`@staticmethod`/`@classmethod` decoration, keyword escaping, deduplication,
and ruff formatting.

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

### Type conversion patterns

For types that parse from strings, provide both fallible and infallible conversions:

1. **`FromStr`**: Fallible parsing via `.parse()` or `from_str()`. Returns `Result`.

2. **`From<T: AsRef<str>>`**: Ergonomic infallible conversion that accepts `&str`, `String`, `Cow<str>`, etc. directly without requiring `.as_str()`.

```rust
impl FromStr for Symbol {
    type Err = SymbolParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // parsing logic
    }
}

impl<T: AsRef<str>> From<T> for Symbol {
    fn from(value: T) -> Self {
        Self::from_str(value.as_ref()).expect(FAILED)
    }
}
```

**Design note**: The `From` impl may panic on invalid input. This is intentional for API ergonomics. Use `FromStr` / `.parse()` when error handling is needed. The `From` impl provides convenience for cases where the input is known to be valid.

**Constraint**: This pattern cannot be used for types that implement `AsRef<str>` themselves (e.g., string wrapper types), as it would conflict with the blanket `impl<T> From<T> for T`. For such types, provide separate `From<&str>` and `From<String>` impls instead.

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

Three concerns drive the choice of hash collection:

- **Iteration-order determinism** (the primary filter)
- **Performance**
- **Thread safety**

Answer the determinism question first, then pick from the remaining options on performance grounds.

#### Iteration-order determinism

`AHash` randomizes its hasher per process, so `AHashMap` / `AHashSet`
iteration order varies between runs. When the iteration order of a
collection feeds observable state on the deterministic simulation testing
(DST) path (events emitted on the message bus, ordered `Vec`s returned
from public methods, the sequence in which a seeded RNG is consumed, the
order in which downstream effects fire), use `IndexMap` / `IndexSet` from
the `indexmap` crate instead. They preserve insertion order and are a
drop-in replacement for the `AHash*` collections.

```rust
use indexmap::{IndexMap, IndexSet};

// Insertion-order iteration; deterministic across runs
let mut commissions: IndexMap<Currency, Money> = IndexMap::new();
let mut subscribed: IndexSet<InstrumentId> = IndexSet::new();
```

The pre-commit hook `check-dst-conventions` enforces `IndexMap` / `IndexSet`
in `crates/live/src/manager.rs` and
`crates/execution/src/matching_engine/engine.rs` because both files were
audited as load-bearing for fill ordering and reconciliation. Other call
sites are reviewed individually; the closed sites and remaining allowed
patterns are listed under "Implementation notes" in
[../concepts/dst.md](../concepts/dst.md).

When the collection is **lookup-only** (no `.iter()`, `.values()`,
`.keys()`, `.into_iter()`, `.drain()`, or `for x in map { ... }`),
iteration order is irrelevant and `AHashMap` / `AHashSet` is the right
choice on performance grounds. Borderline cases (e.g. a public getter
that clones the map and lets callers iterate) should be reviewed against
the inventory's classification rules.

#### Performance

For lookup-heavy hot paths where iteration order does not feed observable
state, prefer `AHashMap` / `AHashSet` over the standard library:

```rust
use ahash::{AHashMap, AHashSet};

let mut symbols: AHashSet<Symbol> = AHashSet::new();
let mut prices: AHashMap<InstrumentId, Price> = AHashMap::new();
```

For non-performance-critical, non-iteration-sensitive cases (factory
registries, configuration maps, test fixtures), standard
`HashMap` / `HashSet` is acceptable and often preferred for simplicity:

```rust
use std::collections::{HashMap, HashSet};

let mut symbols: HashSet<Symbol> = HashSet::new();
let mut prices: HashMap<InstrumentId, Price> = HashMap::new();
```

**Why use `ahash`?**

- **Superior performance**: AHash uses AES-NI hardware instructions when available, providing 2-3x faster hashing compared to the default SipHash.
- **Low collision rates**: Despite being non-cryptographic, AHash provides excellent distribution and low collision rates for typical data.
- **Drop-in replacement**: Fully compatible API with standard library collections.

**When to use standard `HashMap`/`HashSet`:**

- **Non-performance-critical code**: For simple cases where performance is not critical (e.g., factory registries, configuration maps, test fixtures), standard `HashMap`/`HashSet` are acceptable and even preferred for simplicity.
- **Cryptographic security required**: Use standard `HashMap` when hash flooding attacks are a concern (e.g., handling untrusted user input in network protocols).
- **Network clients**: Prefer standard `HashMap` for network-facing components where security considerations outweigh performance benefits.
- **External library boundaries**: Use standard `HashMap` when interfacing with external libraries that expect it (e.g., Arrow serialization metadata).

#### AHashMap vs IndexMap microbenchmarks

The numbers below come from `crates/core/benches/hash_map.rs` (release
profile). Times are per operation; ratio is `IndexMap` relative to
`AHashMap` (values below 1.0 favour `IndexMap`).

| Pattern               | Size | AHashMap | IndexMap | Ratio |
|-----------------------|-----:|---------:|---------:|------:|
| Insert (build map)    |    4 |  40.8 ns |  49.8 ns | 1.22x |
| Insert (build map)    |   32 | 192.4 ns | 348.2 ns | 1.81x |
| Insert (build map)    |  256 |  1.01 us |  2.74 us | 2.72x |
| Lookup (random get)   |    4 |  2.56 ns |  9.36 ns | 3.66x |
| Lookup (random get)   |   32 |  2.49 ns |  7.95 ns | 3.19x |
| Lookup (random get)   |  256 |  3.00 ns |  9.48 ns | 3.16x |
| `.values().collect()` |    4 |  8.08 ns |  6.61 ns | 0.82x |
| `.values().collect()` |   32 |  22.8 ns |  14.8 ns | 0.65x |
| `.values().collect()` |  256 |   145 ns |   109 ns | 0.75x |
| `.keys().collect()`   |    4 |  7.90 ns |  6.24 ns | 0.79x |
| `.keys().collect()`   |   32 |  23.0 ns |  12.6 ns | 0.55x |
| `.keys().collect()`   |  256 |   145 ns |   101 ns | 0.70x |
| Clone                 |    4 |  8.48 ns |  17.8 ns | 2.10x |
| Clone                 |   32 |  25.3 ns |  62.5 ns | 2.47x |
| Clone                 |  256 |  71.0 ns |   247 ns | 3.48x |
| Entry accumulate      |    4 |   122 ns |   159 ns | 1.30x |
| Entry accumulate      |   32 |   439 ns |  1.10 us | 2.51x |
| Entry accumulate      |  256 |  2.21 us |  7.83 us | 3.54x |

For one-key removal, `IndexMap` exposes two methods: `shift_remove`
preserves insertion order at `O(n)` cost; `swap_remove` is `O(1)` but
swaps the last entry into the removed slot, breaking iteration order.

| Pattern    | Size | AHashMap.remove | IndexMap.shift_remove | IndexMap.swap_remove |
|------------|-----:|----------------:|----------------------:|---------------------:|
| Remove one |    4 |         9.89 ns |               37.8 ns |              37.1 ns |
| Remove one |   32 |         62.0 ns |                117 ns |              53.4 ns |
| Remove one |  256 |         70.3 ns |                355 ns |               269 ns |

How to read the table:

- `AHashMap` is roughly 3x faster on pure lookup. Keep `AHashMap` on hot
  lookup paths where iteration order does not flow into observable state.
- `IndexMap` is 25 to 45 percent faster on `.values().collect()` and
  `.keys().collect()`. Where iteration drives observable state, the flip
  to `IndexMap` is a small performance win as well as a determinism win.
- `IndexMap` is 1.3 to 3.5x slower on insert, clone, and entry-modify-or-insert.
  Keep `AHashMap` on construction-heavy or per-fill accumulation paths.
- Prefer `swap_remove` over `shift_remove` when iteration order does not
  matter after the removal; it stays competitive with `AHashMap` removal.

### Thread-safe hash map patterns

`AHashMap` is not thread-safe. Wrapping it in `Arc` only enables sharing the pointer across threads but does not coordinate mutation. Use `Arc<AHashMap>` only when the map is immutable after construction, otherwise add proper synchronization.

```rust
// Avoid: Data races when multiple threads mutate
let cache = Arc::new(AHashMap::new());
let cache_clone = Arc::clone(&cache);
tokio::spawn(async move {
    cache_clone.insert(key, value);  // Data race
});
cache.insert(other_key, other_value);  // Data race
```

**Patterns:**

1. **Immutable after construction** – Build the map once, then share it read-only:

   ```rust
   let mut map = AHashMap::new();
   map.insert(key1, value1);
   map.insert(key2, value2);
   let shared_map = Arc::new(map);  // Now immutable

   // Multiple threads can safely read
   let map_clone = Arc::clone(&shared_map);
   tokio::spawn(async move {
       if let Some(value) = map_clone.get(&key1) {
           // Safe read-only access
       }
   });
   ```

2. **Concurrent reads and writes** – Use `DashMap`:

   ```rust
   use dashmap::DashMap;

   let cache: Arc<DashMap<K, V>> = Arc::new(DashMap::new());

   // Multiple threads can safely read and write concurrently
   cache.insert(key, value);
   if let Some(entry) = cache.get(&key) {
       // Safe concurrent access
   }
   ```

   `DashMap` internally uses sharding and fine-grained locking for efficient concurrent access.

3. **Single-threaded hot paths** – Use plain `AHashMap` in single-threaded contexts:

   ```rust
   struct Handler {
       instruments: AHashMap<Ustr, InstrumentAny>,
   }

   impl Handler {
       async fn next(&mut self) -> Option<()> {
           // Handler runs on a single task, no concurrent access
           self.instruments.insert(key, value);
           Ok(())
       }
   }
   ```

**Decision tree:**

1. Iteration order observable on the DST path? Use `IndexMap<K, V>` / `IndexSet<T>`
2. Otherwise, by access pattern:
   - Immutable after construction: use `Arc<AHashMap<K, V>>`
   - Concurrent access needed: use `Arc<DashMap<K, V>>`
   - Single-threaded access: use plain `AHashMap<K, V>`

### Re-export patterns

Organize re-exports alphabetically and place at the end of lib.rs files:

```rust
// Re-exports
pub use crate::{
    nanos::UnixNanos,
    time::AtomicTime,
    uuid::UUID4,
};

// Module-level re-exports
pub use crate::identifiers::{
    account_id::AccountId,
    actor_id::ActorId,
    client_id::ClientId,
};
```

### Documentation standards

Use third-person declarative voice for all doc comments (e.g., "Returns the account ID" not "Return the account ID").

#### Section header casing

Rustdoc section headers use Title Case, matching the Rust standard library convention:

- `# Examples`
- `# Errors`
- `# Panics`
- `# Safety`
- `# Notes`
- `# Thread Safety`
- `# Feature Flags`

#### Module-Level documentation

All modules must have module-level documentation starting with a brief description:

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

All struct and enum fields must have documentation with terminating periods:

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

- Purpose and behavior
- Explanation of input argument usage
- Error conditions (if applicable)
- Panic conditions (if applicable)

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

For single line errors and panics documentation, use sentence case with the following convention:

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

For multi-line errors and panics documentation, use sentence case with bullets and terminating periods:

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

For Safety documentation, use the `SAFETY:` prefix followed by a short description explaining why the unsafe operation is valid:

```rust
/// Creates a new instance from raw components without validation.
///
/// # Safety
///
/// The caller must ensure that all input parameters are valid and properly initialized.
pub unsafe fn from_raw_parts(ptr: *const u8, len: usize) -> Self {
    // SAFETY: Caller guarantees ptr is valid and len is correct
    Self {
        data: std::slice::from_raw_parts(ptr, len),
    }
}
```

For inline unsafe blocks, use the `SAFETY:` comment directly above the unsafe code:

```rust
impl Send for MessageBus {
    fn send(&self) {
        // SAFETY: Message bus is not meant to be passed between threads
        unsafe {
            // unsafe operation here
        }
    }
}
```

## Python bindings

Python bindings are provided via [PyO3](https://pyo3.rs), allowing users to import NautilusTrader crates directly in Python without a Rust toolchain.

### PyO3 naming conventions

When exposing Rust functions to Python **via PyO3**:

1. The Rust symbol **must** be prefixed with `py_*` to make its purpose explicit inside the Rust
   codebase.
2. Use the `#[pyo3(name = "…")]` attribute to publish the *Python* name **without** the `py_`
   prefix so the Python API remains clean.

```rust
#[pyo3(name = "do_something")]
pub fn py_do_something() -> PyResult<()> {
    // …
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
- Use `#[rstest]` attributes consistently, this standardization reduces cognitive overhead.
- Do *not* use Arrange, Act, Assert separator comments in Rust tests.

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
not exercised by tests. A spec funnels through the production constructor on
every `build()`.

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
in any field surfaces there rather than as silent behavior change in
downstream tests.

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

    // Define strategies for generating test inputs
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

    // Group all property tests inside the proptest! macro
    proptest! {
        #[rstest]
        fn prop_construction_roundtrip(
            value in value_strategy(),
            variant in my_strategy()
        ) {
            // Test invariants that should hold for all generated inputs
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

When working with PyO3 bindings, it's critical to understand and avoid reference cycles between Rust's `Arc` reference counting and Python's garbage collector.
This section documents best practices for handling Python objects in Rust callback-holding structures.

### The reference cycle problem

**Problem**: Using `Arc<PyObject>` in callback-holding structs creates circular references:

1. **Rust `Arc` holds Python objects** → increases Python reference count.
2. **Python objects might reference Rust objects** → creates cycles.
3. **Neither side can be garbage collected** → memory leak.

**Example of problematic pattern**:

```rust
// AVOID: This creates reference cycles
struct CallbackHolder {
    handler: Option<Arc<PyObject>>,  // ❌ Arc wrapper causes cycles
}
```

### The solution: GIL-based cloning

**Solution**: Use plain `PyObject` with proper GIL-based cloning via `clone_py_object()`:

```rust
use nautilus_core::python::clone_py_object;

// CORRECT: Use plain PyObject without Arc wrapper
struct CallbackHolder {
    handler: Option<PyObject>,  // ✅ No Arc wrapper
}

// Manual Clone implementation using clone_py_object
impl Clone for CallbackHolder {
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.as_ref().map(clone_py_object),
        }
    }
}
```

### Best practices

#### 1. Use `clone_py_object()` for Python object cloning

```rust
// When cloning Python callbacks
let cloned_callback = clone_py_object(&original_callback);

// In manual Clone implementations
self.py_handler.as_ref().map(clone_py_object)
```

#### 2. Remove `#[derive(Clone)]` from callback-holding structs

```rust
// BEFORE: Automatic derive causes issues with PyObject
#[derive(Clone)]  // ❌ Remove this
struct Config {
    handler: Option<PyObject>,
}

// AFTER: Manual implementation with proper cloning
struct Config {
    handler: Option<PyObject>,
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Self {
            // Clone regular fields normally
            url: self.url.clone(),
            // Use clone_py_object for Python objects
            handler: self.handler.as_ref().map(clone_py_object),
        }
    }
}
```

#### 3. Update function signatures to accept `PyObject`

```rust
// BEFORE: Arc wrapper in function signatures
fn spawn_task(handler: Arc<PyObject>) { ... }  // ❌

// AFTER: Plain PyObject
fn spawn_task(handler: PyObject) { ... }  // ✅
```

#### 4. Avoid `Arc::new()` when creating Python callbacks

```rust
// BEFORE: Wrapping in Arc
let callback = Arc::new(py_function);  // ❌

// AFTER: Use directly
let callback = py_function;  // ✅
```

### Why this works

The `clone_py_object()` function:

- **Acquires the Python GIL** before performing clone operations.
- **Uses Python's native reference counting** via `clone_ref()`.
- **Avoids Rust Arc wrappers** that interfere with Python GC.
- **Maintains thread safety** through proper GIL management.

This approach allows both Rust and Python garbage collectors to work correctly, eliminating memory leaks from reference cycles.

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

| Situation                                                          | Use                                               |
|--------------------------------------------------------------------|---------------------------------------------------|
| Public API input against named preconditions                       | `check_*` from `nautilus_core::correctness`       |
| Validated constructors (fallible + panic pair)                     | `new_checked()` / `new()`                         |
| Recoverable non‑validation errors (I/O, parse, network)            | `Result<T, DomainError>`                          |
| Internal invariant the compiler cannot prove                       | `debug_assert!`                                   |
| Always‑on internal invariant without a matching `CorrectnessError` | `assert!`                                         |
| Soundness‑critical `unsafe` precondition                           | `assert!` (always on)                             |
| Hot‑path `unsafe` precondition upheld by design                    | `debug_assert!` plus a documented `Safety` clause |

Style:

- Prefix `debug_assert!` messages with `Invariant:` and state the positive rule,
  not the failure: `debug_assert!(next > last, "Invariant: time is strictly monotonic across CAS")`.
- `Condition failed: ...` (from the `FAILED` constant) marks a caller-supplied
  input violation; `Invariant: ...` marks an internal contract bug.
- Place assertions where the invariant is first assumed. When an invariant holds
  across a hot loop, assert once at the boundary rather than inside the loop.

## Common anti-patterns

1. **Avoid `.clone()` in hot paths** – favour borrowing or shared ownership via `Arc`.
2. **Avoid `.unwrap()` in production code** – generally propagate errors with `?` or map them into domain errors, but unwrapping lock poisoning is acceptable because it signals a severe program state that should abort fast.
3. **Avoid `String` when `&str` suffices** – minimise allocations on tight loops.
4. **Avoid exposing interior mutability** – hide mutexes/`RefCell` behind safe APIs.
5. **Avoid large structs in `Result<T, E>`** – box large error payloads (`Box<dyn Error + Send + Sync>`).

## Unsafe Rust

It will be necessary to write `unsafe` Rust code to be able to achieve the value
of interoperating between Cython and Rust. The ability to step outside the boundaries of safe Rust is what makes it possible to
implement many of the most fundamental features of the Rust language itself, just as C and C++ are used to implement
their own standard libraries.

Great care will be taken with the use of Rusts `unsafe` facility - which enables a small set of additional language features, thereby changing
the contract between the interface and caller, shifting some responsibility for guaranteeing correctness
from the Rust compiler, and onto us. The goal is to realize the advantages of the `unsafe` facility, whilst avoiding *any* undefined behavior.
The definition for what the Rust language designers consider undefined behavior can be found in the [language reference](https://doc.rust-lang.org/stable/reference/behavior-considered-undefined.html).

### Safety policy

To maintain correctness, any use of `unsafe` Rust must follow our policy:

- If a function is `unsafe` to call, there *must* be a `Safety` section in the documentation explaining why the function is `unsafe`,
  covering the invariants which the function expects the callers to uphold, and how to meet their obligations in that contract.
- Document why each function is `unsafe` in its doc comment's Safety section, and cover all `unsafe` blocks with unit tests.
- Always include a `SAFETY:` comment explaining why the unsafe operation is valid.
- **Crate-level lint** – every crate that exposes FFI symbols enables
  `#![deny(unsafe_op_in_unsafe_fn)]`. Even inside an `unsafe fn`, each pointer dereference or
  other dangerous operation must be wrapped in its own `unsafe { … }` block.
- **CVec contract** – for raw vectors that cross the FFI boundary read the
  [FFI Memory Contract](ffi.md). Foreign code becomes the owner of the allocation and **must**
  call the matching `vec_drop_*` function exactly once.

### Categories of unsafe code

The codebase uses unsafe Rust in these categories:

1. **FFI boundaries** – Raw pointer operations for C interop. See [FFI documentation](ffi.md).
2. **Interior mutability** – `UnsafeCell` for thread-local registries with controlled access patterns.
3. **Unsafe Send/Sync** – Types that are not inherently thread-safe but satisfy trait bounds
   through runtime invariants (e.g., single-threaded access guaranteed by architecture).

### Unsafe Send/Sync requirements

When implementing `Send` or `Sync` unsafely:

1. Document exactly which fields violate the trait requirements.
2. Explain the runtime mechanism that ensures safety (e.g., single-threaded event loop).
3. Include a `WARNING` stating that violating the invariant is undefined behavior.
4. Prefer runtime enforcement (assertions, `Result` returns) over documentation-only guarantees.

```rust
// SAFETY: Contains Rc<RefCell<...>> which is not thread-safe.
// Single-threaded access guaranteed by the backtest engine architecture.
// WARNING: Actually sending across threads is undefined behavior.
#[allow(unsafe_code)]
unsafe impl Send for BacktestDataClient {}
```

### Defense in depth

Where unsafe code relies on invariants, add defense mechanisms:

- **Type verification**: Check types at runtime before casting (e.g., `TypeId` comparison).
- **Debug assertions**: Catch memory corruption early in debug builds.
- **RAII guards**: Ensure cleanup on both normal return and panic paths.
- **Runtime checks**: Fail fast when invariants are violated rather than proceeding unsafely.

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
`Arc<AtomicBool>` for stop signaling and `Arc<AtomicU8>` for state, both
with `Ordering::Relaxed`.

#### Actor registry vs component registry

Both registries store `Rc<UnsafeCell<dyn Trait>>` in thread-local maps but
differ in how they handle aliased access:

| Property          | Actor registry                     | Component registry                 |
|-------------------|------------------------------------|------------------------------------|
| Aliasing          | Allowed (multiple guards)          | Prevented (`BorrowGuard` + set)    |
| Re‑entrant access | Yes, required for callbacks        | No, lifecycle ops are sequential   |
| Error handling    | Panic or `None` on lookup failure  | Returns `anyhow::Result` on error  |
| Guard type        | `ActorRef<T>` (Rc‑backed)          | Stack‑local `BorrowGuard`          |

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

The project uses several tools for code quality:

- **rustfmt**: Automatic code formatting (see `rustfmt.toml`).
- **clippy**: Linting and best practices (see `clippy.toml`).
  When suppressing `missing_panics_doc` or `missing_errors_doc`, include a `reason`
  explaining why the lint does not apply:

  ```rust
  #[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
  ```

- **cbindgen**: C header generation for FFI.

## Rust version management

The project pins to a specific Rust version via `rust-toolchain.toml`.

**Keep your toolchain synchronized with CI:**

```bash
rustup update       # Update to latest stable Rust
rustup show         # Verify correct toolchain is active
```

If pre-commit passes locally but fails in CI, clear the prek cache and re-run:

```bash
prek clean    # Clear cached environments
make pre-commit     # Re-run all checks
```

This ensures you're using the same Rust and clippy versions as CI.

## Resources

- [The Rustonomicon](https://doc.rust-lang.org/nomicon/) – The Dark Arts of Unsafe Rust.
- [The Rust Reference – Unsafety](https://doc.rust-lang.org/stable/reference/unsafety.html).
- [Safe Bindings in Rust – Russell Johnston](https://www.abubalay.com/blog/2020/08/22/safe-bindings-in-rust).
- [Google – Rust and C interoperability](https://www.chromium.org/Home/chromium-security/memory-safety/rust-and-c-interoperability/).

## Cap'n Proto serialization

The `nautilus-serialization` crate provides optional Cap'n Proto serialization support for efficient data interchange.
This feature is opt-in to avoid requiring the Cap'n Proto compiler for standard builds.

### Installing Cap'n Proto

Install the Cap'n Proto compiler before working with schemas. The required version is
specified in `tools.toml` in the repository root.

See the [Environment Setup](environment_setup.md#capn-proto) guide for detailed installation
instructions for each platform.

:::warning
Ubuntu's default `capnproto` package is too old. Linux users must install from source.
:::

Verify installation:

```bash
capnp --version  # Should match the version in tools.toml
```

### Schema development workflow

Schema files live in `crates/serialization/schemas/capnp/`:

- `common/` - Base types, identifiers, enums.
- `commands/` - Trading commands.
- `events/` - Order and position events.
- `data/` - Market data types.

When modifying schemas:

1. Edit the `.capnp` schema file in the appropriate subdirectory.
2. Regenerate Rust bindings:

   ```bash
   make regen-capnp
   # or
   ./scripts/regen_capnp.sh
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

Generated Rust files are checked into `crates/serialization/generated/capnp/` for these reasons:

- **docs.rs compatibility**: The documentation build environment lacks the Cap'n Proto
  compiler.
- **Contributor convenience**: Most developers don't need to install capnp for standard
  development.
- **Build reproducibility**: Ensures consistent code generation across environments.

The generated files are automatically created during builds via `build.rs` when the `capnp`
feature is enabled, but we commit them to the repository to support builds without the
compiler installed.

### Verifying schema consistency

Before committing schema changes, ensure generated files are up-to-date:

```bash
make check-capnp-schemas
```

This target:

1. Skips with a warning if `capnp` is not installed (acceptable for local development).
2. Fails if regeneration errors occur (e.g., version mismatch).
3. Regenerates schemas and fails if generated files differ from committed versions.

CI runs this check automatically to catch drift (capnp is always installed in CI).

### Testing with capnp feature

```bash
# Run workspace tests with capnp
make cargo-test EXTRA_FEATURES="capnp"

# Run specific crate tests with capnp
make cargo-test-crate-nautilus-serialization FEATURES="capnp"

# Run specific test
cargo test -p nautilus-serialization --features capnp test_price_roundtrip
```

### Schema evolution guidelines

When evolving schemas:

- **Additive changes only**: Add new fields at the end.
- **Never remove fields**: Mark deprecated fields in comments.
- **Never reuse field numbers**: Even after deprecation.
- **Test roundtrip compatibility**: Ensure old and new versions interoperate.

Cap'n Proto's evolution rules allow schema changes without breaking binary compatibility, but
you must follow these constraints to maintain forward/backward compatibility.
