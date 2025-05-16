# Rust

data race conditions, being 'correct by construction' through its formal specification of types, ownership
The [Rust](https://www.rust-lang.org/learn) programming language is an ideal fit for implementing the mission-critical core of the platform and systems. Its strong type system, ownership model, and compile-time checks eliminate memory errors and data races by construction, while zero-cost abstractions and the absence of a garbage collector deliver C-like performance—critical for high-frequency trading workloads.

## Python Bindings

Python bindings are provided via Cython and [PyO3](https://pyo3.rs), allowing users to import NautilusTrader crates directly in Python without a Rust toolchain.

## Unsafe Rust

It will be necessary to write `unsafe` Rust code to be able to achieve the value
of interoperating between Cython and Rust. The ability to step outside the boundaries of safe Rust is what makes it possible to
implement many of the most fundamental features of the Rust language itself, just as C and C++ are used to implement
their own standard libraries.

Great care will be taken with the use of Rusts `unsafe` facility - which just enables a small set of additional language features, thereby changing
the contract between the interface and caller, shifting some responsibility for guaranteeing correctness
from the Rust compiler, and onto us. The goal is to realize the advantages of the `unsafe` facility, whilst avoiding *any* undefined behavior.
The definition for what the Rust language designers consider undefined behavior can be found in the [language reference](https://doc.rust-lang.org/stable/reference/behavior-considered-undefined.html).

## Safety Policy

to adhere to when implementing `unsafe` Rust.
To maintain correctness, any use of `unsafe` Rust must follow our policy:

- If a function is `unsafe` to call, there *must* be a `Safety` section in the documentation explaining why the function is `unsafe`
and covering the invariants which the function expects the callers to uphold, and how to meet their obligations in that contract.
- Document why each function is `unsafe` in its doc comment’s Safety section, and cover all `unsafe` blocks with unit tests.

## Resources

- [The Rustonomicon](https://doc.rust-lang.org/nomicon/) - The Dark Arts of Unsafe Rust
- [The Rust Reference - Unsafety](https://doc.rust-lang.org/stable/reference/unsafety.html)
- [Safe Bindings in Rust - Russell Johnston](https://www.abubalay.com/blog/2020/08/22/safe-bindings-in-rust)
- [Google - Rust and C interoperability](https://www.chromium.org/Home/chromium-security/memory-safety/rust-and-c-interoperability/)
