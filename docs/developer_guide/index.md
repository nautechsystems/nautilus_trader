# Developer Guide

Welcome to the developer guide for NautilusTrader!

Here you'll find guidance on developing and extending NautilusTrader to meet your trading needs or to contribute improvements back to the project.

:::info
This guide is structured so that automated tooling can consume it alongside human readers.
:::

We believe in using the right tool for the job. The overall design philosophy is to fully utilize
the high level power of Python, with its rich eco-system of frameworks and libraries, whilst
overcoming some of its inherent shortcomings in performance and lack of built-in type safety
(with it being an interpreted dynamic language).

One of the advantages of Cython is that allocation and freeing of memory is handled by the C code
generator during the ‘cythonization’ step of the build (unless you’re specifically utilizing some of
its lower level features).

This approach combines Python’s simplicity with near-native C performance via compiled extensions.

The main development and runtime environment we are working in is Python. With the
introduction of Cython throughout the production codebase in `.pyx` and `.pxd` files, it's
important to be aware of how the CPython implementation of Python interacts with the underlying
CPython API, and the NautilusTrader C extension modules which Cython produces.

We recommend a thorough review of the [Cython docs](https://cython.readthedocs.io/en/latest/) to familiarize yourself with some of its core
concepts, and where C typing is being used.

It's not necessary to become a C language expert, however it's helpful to understand how Cython C
syntax is used in function and method definitions, in local code blocks, and the common primitive C
types and how these map to their corresponding `PyObject` types.

## Contents

- [Environment Setup](environment_setup.md)
- [Coding Standards](coding_standards.md)
- [Cython](cython.md)
- [Rust](rust.md)
- [Testing](testing.md)
- [Docs Style Guide](docs.md)
- [Release Notes Guide](releases.md)
- [Adapters](adapters.md)
- [Benchmarking](benchmarking.md)
- [Packaged Data](packaged_data.md)
- [FFI Memory Contract](ffi.md)
