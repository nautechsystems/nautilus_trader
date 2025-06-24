# Rust Style Guide

The [Rust](https://www.rust-lang.org/learn) programming language is an ideal fit for implementing the mission-critical core of the platform and systems. Its strong type system, ownership model, and compile-time checks eliminate memory errors and data races by construction, while zero-cost abstractions and the absence of a garbage collector deliver C-like performance—critical for high-frequency trading workloads.

## Python Bindings

Python bindings are provided via Cython and [PyO3](https://pyo3.rs), allowing users to import NautilusTrader crates directly in Python without a Rust toolchain.

## Code Style and Conventions

### File Header Requirements

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

### Code Formatting

Import formatting is automatically handled by rustfmt when running `make format`.
The tool organizes imports into groups (standard library, external crates, local imports) and sorts them alphabetically within each group.

#### Function spacing

- Leave **one blank line between functions** (including tests) – this improves readability and
mirrors the default behavior of `rustfmt`.
- Leave **one blank line above every doc comment** (`///` or `//!`) so that the comment is clearly
  detached from the previous code block.

#### PyO3 naming convention

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

### Error Handling

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

5. **Error Message Formatting**: Prefer inline format strings over positional arguments:

   ```rust
   // Preferred - inline format with variable names
   anyhow::bail!("Failed to subtract {n} months from {datetime}");

   // Instead of - positional arguments
   anyhow::bail!("Failed to subtract {} months from {}", n, datetime);
   ```

   This makes error messages more readable and self-documenting, especially when there are multiple variables.

### Attribute Patterns

Consistent attribute usage and ordering:

```rust
#[repr(C)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
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
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
pub enum AccountType {
    /// An account with unleveraged cash assets only.
    Cash = 1,
    /// An account which facilitates trading on margin, using account assets as collateral.
    Margin = 2,
}
```

### Constructor Patterns

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

### Constants and Naming Conventions

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

### Re-export Patterns

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

### Documentation Standards

#### Module-Level Documentation

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
//! - `stubs`: Enables type stubs for use in testing scenarios.
```

#### Field Documentation

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

#### Function Documentation

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

#### Errors and Panics Documentation Format

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
/// This function will return an error if:
/// - The market price for the instrument cannot be found.
/// - The conversion rate calculation fails.
/// - Invalid position state is encountered.
///
/// # Panics
///
/// This function will panic if:
/// - The instrument ID is invalid or uninitialized.
/// - Required market data is missing from the cache.
/// - Internal state consistency checks fail.
pub fn calculate_unrealized_pnl(&self, market_price: Price) -> anyhow::Result<Money> {
    // Implementation
}
```

#### Safety Documentation Format

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

### Testing Conventions

#### Test Organization

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

#### Parameterized Testing

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

#### Test Naming

Use descriptive test names that explain the scenario:

```rust
fn test_sma_with_no_inputs()
fn test_sma_with_single_input()
fn test_symbol_is_composite()
```

## Unsafe Rust

It will be necessary to write `unsafe` Rust code to be able to achieve the value
of interoperating between Cython and Rust. The ability to step outside the boundaries of safe Rust is what makes it possible to
implement many of the most fundamental features of the Rust language itself, just as C and C++ are used to implement
their own standard libraries.

Great care will be taken with the use of Rusts `unsafe` facility - which just enables a small set of additional language features, thereby changing
the contract between the interface and caller, shifting some responsibility for guaranteeing correctness
from the Rust compiler, and onto us. The goal is to realize the advantages of the `unsafe` facility, whilst avoiding *any* undefined behavior.
The definition for what the Rust language designers consider undefined behavior can be found in the [language reference](https://doc.rust-lang.org/stable/reference/behavior-considered-undefined.html).

### Safety Policy

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

## Tooling Configuration

The project uses several tools for code quality:

- **rustfmt**: Automatic code formatting (see `rustfmt.toml`).
- **clippy**: Linting and best practices (see `clippy.toml`).
- **cbindgen**: C header generation for FFI.

## Resources

- [The Rustonomicon](https://doc.rust-lang.org/nomicon/) – The Dark Arts of Unsafe Rust.
- [The Rust Reference – Unsafety](https://doc.rust-lang.org/stable/reference/unsafety.html).
- [Safe Bindings in Rust – Russell Johnston](https://www.abubalay.com/blog/2020/08/22/safe-bindings-in-rust).
- [Google – Rust and C interoperability](https://www.chromium.org/Home/chromium-security/memory-safety/rust-and-c-interoperability/).
