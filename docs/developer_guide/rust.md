# Rust Style Guide

The [Rust](https://www.rust-lang.org/learn) programming language is an ideal fit for implementing the mission-critical core of the platform and systems. Its strong type system, ownership model, and compile-time checks eliminate memory errors and data races by construction, while zero-cost abstractions and the absence of a garbage collector deliver C-like performance—critical for high-frequency trading workloads.

## Python Bindings

Python bindings are provided via Cython and [PyO3](https://pyo3.rs), allowing users to import NautilusTrader crates directly in Python without a Rust toolchain.

## Code Style and Conventions

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

### Documentation Standards

#### Module-Level Documentation

All modules should have comprehensive module-level documentation:

```rust
//! Functions for correctness checks similar to the *design by contract* philosophy.
//!
//! This module provides validation checking of function or method conditions.
//!
//! A condition is a predicate which must be true just prior to the execution of
//! some section of code - for correct behavior as per the design specification.
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

### Testing Conventions

#### Test Organization

```rust
#[cfg(test)]
mod tests {
    use rstest::rstest;
    use super::*;
    use crate::stubs::*;

    // Tests here
}
```

#### Parameterized Testing

Use the `rstest` attribute consistently, and for parameterized tests:

```rust
#[rstest]
#[case(1)]
#[case(3)]
#[case(5)]
fn test_with_different_periods(#[case] period: usize) {
    // Test implementation
}
```

#### Test Naming

Use descriptive test names that explain the scenario:

```rust
fn test_sma_with_no_inputs()
fn test_sma_with_single_input()
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

- If a function is `unsafe` to call, there *must* be a `Safety` section in the documentation explaining why the function is `unsafe`
and covering the invariants which the function expects the callers to uphold, and how to meet their obligations in that contract.
- Document why each function is `unsafe` in its doc comment's Safety section, and cover all `unsafe` blocks with unit tests.
- Always include a `SAFETY:` comment explaining why the unsafe operation is valid:

```rust
// SAFETY: Message bus is not meant to be passed between threads
#[allow(unsafe_code)]
unsafe impl Send for MessageBus {}
```

### File Headers

All Rust files should include the standard copyright header:

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
// -------------------------------------------------------------------------------------------------`
```

## Tooling Configuration

The project uses several tools for code quality:

- **rustfmt**: Automatic code formatting (see `rustfmt.toml`)
- **clippy**: Linting and best practices (see `clippy.toml`)
- **cbindgen**: C header generation for FFI

## Resources

- [The Rustonomicon](https://doc.rust-lang.org/nomicon/) - The Dark Arts of Unsafe Rust
- [The Rust Reference - Unsafety](https://doc.rust-lang.org/stable/reference/unsafety.html)
- [Safe Bindings in Rust - Russell Johnston](https://www.abubalay.com/blog/2020/08/22/safe-bindings-in-rust)
- [Google - Rust and C interoperability](https://www.chromium.org/Home/chromium-security/memory-safety/rust-and-c-interoperability/)
