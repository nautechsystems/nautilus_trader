# Rust API

The core of NautilusTrader is written in Rust, and one day it will be possible to run systems
entirely programmed and compiled from Rust. 

The API reference provides detailed technical documentation for the core NautilusTrader crates,
the docs are generated from source code using `cargo doc`.

```{note}
Note the docs are generated using the _nightly_ toolchain (to be able to compile docs for the entire workspace).
However, we target the _stable_ toolchain for all releases.
```

Use the following links to explore the Rust docs API references for two different versions of the codebase:

## [Latest Rust docs](https://docs.nautilustrader.io/core)
This API reference is built from the HEAD of the `master` branch and represents the latest stable release.

## [Develop Rust docs](https://docs.nautilustrader.io/develop/core)
This API reference is built from the HEAD of the `develop` branch and represents bleeding edge and experimental changes/features currently in development.

## What is Rust?
[Rust](https://www.rust-lang.org/) is a multi-paradigm programming language designed for performance and safety, especially safe
concurrency. Rust is blazingly fast and memory-efficient (comparable to C and C++) with no runtime or
garbage collector. It can power mission-critical systems, run on embedded devices, and easily
integrates with other languages.

Rust’s rich type system and ownership model guarantees memory-safety and thread-safety deterministically —
eliminating many classes of bugs at compile-time.

The project increasingly utilizes Rust for core performance-critical components. Python language binding is handled through
Cython, with static libraries linked at compile-time before the wheel binaries are packaged, so a user
does not need to have Rust installed to run NautilusTrader. In the future as more Rust code is introduced,
[PyO3](https://pyo3.rs/latest) will be leveraged for easier Python bindings.

This project makes the [Soundness Pledge](https://raphlinus.github.io/rust/2020/01/18/soundness-pledge.html):

> “The intent of this project is to be free of soundness bugs.
> The developers will do their best to avoid them, and welcome help in analyzing and fixing them.”
