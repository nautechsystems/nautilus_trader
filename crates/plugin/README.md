# nautilus-plugin

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-plugin)](https://docs.rs/nautilus-plugin/latest/nautilus-plugin/)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Plug-in system for [NautilusTrader](https://nautilustrader.io).

The `nautilus-plugin` crate defines the C-ABI boundary between a Nautilus host (the live node)
and independently compiled Rust plug-in cdylibs. The host `dlopen`s a plug-in, calls a single
`nautilus_plugin_init` entry symbol, and registers every plug point the returned manifest
enumerates. The boundary is C ABI because Rust's `#[repr(Rust)]` layout is unstable across
compilations, so cross-cdylib `Box<dyn Trait>` and `async fn` are unsound. C ABI is the layer
of contract both halves can compile to without sharing a build.

Authors write normal Rust. The `nautilus_plugin!` macro emits the `extern "C"` symbol, the
`#[repr(C)]` manifest, and the per-type vtables; authors never type `extern "C"`,
`#[repr(C)]`, or `unsafe` themselves.

The plug-in system supports the following sync trait surfaces. Each lives in its
own module under `src/` and follows the same pattern: a `#[repr(C)]` vtable, a
matching author-facing trait, `extern "C"` thunks wired through `panic::guard`,
and a `Slice<'static, Registration>` field on `PluginManifest`. Adding a plug
point means adding one module and one `Slice` field.

| Plug point         | Status     | Module                       |
|--------------------|------------|------------------------------|
| Custom data type   | Shipped    | `surfaces::custom_data`      |
| Actor / DataActor  | Shipped    | `surfaces::actor`            |
| Strategy           | Shipped    | `surfaces::strategy`         |
| Execution algorithm| Not yet    | `surfaces::exec_algorithm`   |
| Indicator          | Not yet    | `surfaces::indicator`        |
| Fill model         | Not yet    | `surfaces::fill_model`       |
| Pricing / greeks   | Not yet    | `surfaces::pricing`          |

Out of scope: async client adapters (data and execution), catalog and cache
backends, pre-trade risk gating, event store backends, and hot reload of any
plug-in while the live node is running. Plug-ins load at process startup and
live for the process lifetime.

`OrderBookDeltas` callbacks are also out of scope for v1. The type owns a
`Vec<OrderBookDelta>` and cannot be `#[repr(C)]`, so passing a raw pointer
across the cdylib boundary has no stable layout guarantee. A future revision
will deliver book deltas through a boundary-owned representation.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `host`: Enables host-side plug-in loading via [`libloading`](https://crates.io/crates/libloading).
  The live node enables this feature; plug-in cdylibs leave it off so they do not link
  `libloading` themselves.

## Platform

Plug-in cdylibs use the platform-native shared-library format:

- Linux: `lib<name>.so`
- macOS: `lib<name>.dylib`
- Windows: `<name>.dll`

`libloading` handles the platform differences on the host side; the example test under
`tests/load_example_cdylib.rs` builds and loads a cdylib on all three.

Rust's ABI is unstable across compilations on every platform, so the plug-in build must be
pinned to the host's Rust toolchain version and Nautilus version. `NAUTILUS_PLUGIN_ABI_VERSION`
is bumped on every breaking change to any `#[repr(C)]` struct or vtable in this crate; the host
refuses to load a plug-in whose `PluginManifest::abi_version` does not match.

## Documentation

See [the docs](https://docs.rs/nautilus-plugin) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
