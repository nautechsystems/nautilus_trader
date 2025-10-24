# Rust Style Guide

The [Rust](https://www.rust-lang.org/learn) programming language is an ideal fit for implementing the mission-critical core of the platform and systems.
Its strong type system, ownership model, and compile-time checks eliminate memory errors and data races by construction,
while zero-cost abstractions and the absence of a garbage collector deliver C-like performance—critical for high-frequency trading workloads.

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

## Feature flag conventions

- Prefer additive feature flags—enabling a feature must not break existing functionality.
- Use descriptive flag names that explain what capability is enabled.
- Document every feature in the crate-level documentation so consumers know what they toggle.
- Common patterns:
  - `high-precision`: switches the value-type backing (64-bit or 128-bit integers) to support domains that require extra precision.
  - `default = []`: keep defaults minimal.
  - `python`: enables Python bindings.
  - `extension-module`: builds a Python extension module (always include `python`).
  - `ffi`: enables C FFI bindings.
  - `stubs`: exposes testing stubs.

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
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

### Logging

- Fully qualify logging macros so the backend is explicit:
  - Use `log::…` (`log::info!`, `log::warn!`, etc.) inside synchronous core crates.
  - Use `tracing::…` (`tracing::debug!`, `tracing::info!`, etc.) for async runtimes, adapters, and peripheral components.
- Start messages with a capitalised word, prefer complete sentences, and omit terminal periods (e.g. `"Processing batch"`, not `"Processing batch."`).

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

### Async patterns

Use consistent async/await patterns:

1. **Async function naming**: No special suffix is required; prefer natural names.
2. **Tokio usage**: Use `tokio::spawn` for fire-and-forget work, and document when that background task is expected to finish.
3. **Error handling**: Return `anyhow::Result` from async functions to match the synchronous conventions.
4. **Cancellation safety**: Call out whether the function is cancellation-safe and what invariants still hold when it is cancelled.
5. **Stream handling**: Use `tokio_stream` (or `futures::Stream`) for async iterators to make back-pressure explicit.
6. **Timeout patterns**: Wrap network or long-running awaits with timeouts (`tokio::time::timeout`) and propagate or handle the timeout error.

### Attribute patterns

Consistent attribute usage and ordering:

```rust
#[repr(C)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model")
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
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.model")
)]
pub enum AccountType {
    /// An account with unleveraged cash assets only.
    Cash = 1,
    /// An account which facilitates trading on margin, using account assets as collateral.
    Margin = 2,
}
```

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
pub fn new_checked<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
    // Implementation
}

/// Creates a new [`Symbol`] instance.
///
/// # Panics
///
/// Panics if `value` is not a valid string.
pub fn new<T: AsRef<str>>(value: T) -> Self {
    Self::new_checked(value).expect(FAILED)
}
```

Always use the `FAILED` constant for `.expect()` messages related to correctness checks:

```rust
use nautilus_core::correctness::FAILED;
```

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

Prefer `AHashMap` and `AHashSet` from the `ahash` crate over the standard library's `HashMap` and `HashSet`:

```rust
use ahash::{AHashMap, AHashSet};

// Preferred - using AHashMap/AHashSet
let mut symbols: AHashSet<Symbol> = AHashSet::new();
let mut prices: AHashMap<InstrumentId, Price> = AHashMap::new();

// Instead of - standard library HashMap/HashSet
use std::collections::{HashMap, HashSet};
let mut symbols: HashSet<Symbol> = HashSet::new();
let mut prices: HashMap<InstrumentId, Price> = HashMap::new();
```

**Why use `ahash`?**

- **Superior performance**: AHash uses AES-NI hardware instructions when available, providing 2-3x faster hashing compared to the default SipHash.
- **Low collision rates**: Despite being non-cryptographic, AHash provides excellent distribution and low collision rates for typical data.
- **Drop-in replacement**: Fully compatible API with standard library collections.

**When to use standard `HashMap`/`HashSet`:**

- **Cryptographic security required**: Use standard `HashMap` when hash flooding attacks are a concern (e.g., handling untrusted user input in network protocols).
- **Network clients**: Currently prefer standard `HashMap` for network-facing components where security considerations outweigh performance benefits.

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

Python bindings are provided via Cython and [PyO3](https://pyo3.rs), allowing users to import NautilusTrader crates directly in Python without a Rust toolchain.

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

### Testing conventions

- Use `mod tests` as the standard test module name unless you need to specifically compartmentalize.
- Use `#[rstest]` attributes consistently, this standardization reduces cognitive overhead.
- Do *not* use Arrange, Act, Assert separator comments in Rust tests.

#### Test organization

Use consistent test module structure with section separators:

```rust
////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use super::*;
    use crate::identifiers::{Symbol, stubs::*};

    #[rstest]
    fn test_string_reprs(symbol_eth_perp: Symbol) {
        assert_eq!(symbol_eth_perp.as_str(), "ETH-PERP");
        assert_eq!(format!("{symbol_eth_perp}"), "ETH-PERP");
    }
}
```

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

#### Test naming

Use descriptive test names that explain the scenario:

```rust
fn test_sma_with_no_inputs()
fn test_sma_with_single_input()
fn test_symbol_is_composite()
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

Great care will be taken with the use of Rusts `unsafe` facility - which just enables a small set of additional language features, thereby changing
the contract between the interface and caller, shifting some responsibility for guaranteeing correctness
from the Rust compiler, and onto us. The goal is to realize the advantages of the `unsafe` facility, whilst avoiding *any* undefined behavior.
The definition for what the Rust language designers consider undefined behavior can be found in the [language reference](https://doc.rust-lang.org/stable/reference/behavior-considered-undefined.html).

### Safety policy

To maintain correctness, any use of `unsafe` Rust must follow our policy:

- If a function is `unsafe` to call, there *must* be a `Safety` section in the documentation explaining why the function is `unsafe`.
and covering the invariants which the function expects the callers to uphold, and how to meet their obligations in that contract.
- Document why each function is `unsafe` in its doc comment's Safety section, and cover all `unsafe` blocks with unit tests.
- Always include a `SAFETY:` comment explaining why the unsafe operation is valid:

```rust
// SAFETY: Message bus is not meant to be passed between threads
#[allow(unsafe_code)]

unsafe impl Send for MessageBus {}
```

- **Crate-level lint** – every crate that exposes FFI symbols enables
  `#![deny(unsafe_op_in_unsafe_fn)]`. Even inside an `unsafe fn`, each pointer dereference or
  other dangerous operation must be wrapped in its own `unsafe { … }` block.

- **CVec contract** – for raw vectors that cross the FFI boundary read the
  [FFI Memory Contract](ffi.md). Foreign code becomes the owner of the allocation and **must**
  call the matching `vec_drop_*` function exactly once.

## Tooling configuration

The project uses several tools for code quality:

- **rustfmt**: Automatic code formatting (see `rustfmt.toml`).
- **clippy**: Linting and best practices (see `clippy.toml`).
- **cbindgen**: C header generation for FFI.

## Rust version management

The project pins to a specific Rust version via `rust-toolchain.toml`.

**Keep your toolchain synchronized with CI:**

```bash
rustup update       # Update to latest stable Rust
rustup show         # Verify correct toolchain is active
```

If pre-commit passes locally but fails in CI, clear the pre-commit cache and re-run:

```bash
pre-commit clean    # Clear cached environments
make pre-commit     # Re-run all checks
```

This ensures you're using the same Rust and clippy versions as CI.

## Resources

- [The Rustonomicon](https://doc.rust-lang.org/nomicon/) – The Dark Arts of Unsafe Rust.
- [The Rust Reference – Unsafety](https://doc.rust-lang.org/stable/reference/unsafety.html).
- [Safe Bindings in Rust – Russell Johnston](https://www.abubalay.com/blog/2020/08/22/safe-bindings-in-rust).
- [Google – Rust and C interoperability](https://www.chromium.org/Home/chromium-security/memory-safety/rust-and-c-interoperability/).
