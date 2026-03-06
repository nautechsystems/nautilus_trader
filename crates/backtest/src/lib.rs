//! Backtest engine for [NautilusTrader](http://nautilustrader.io).
//!
//! The `nautilus-backtest` crate provides a comprehensive event-driven backtesting framework that allows
//! quantitative traders to test and validate trading strategies on historical data with high
//! fidelity market simulation. The system replicates real market conditions including:
//!
//! - Event-driven backtesting engine with simulated exchanges.
//! - Market data replay with configurable latency and fill models.
//! - Order matching engines with realistic execution simulation.
//! - Multi-venue and multi-asset backtesting capabilities.
//! - Comprehensive configuration and state management.
//! - Integration with live trading systems for seamless deployment.
//!
//! # Platform
//!
//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
//! highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.
//!
//! # Feature Flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `examples`: Enables example strategies and the EMA crossover backtest example.
//! - `streaming`: Enables `persistence` dependency for streaming configuration.
//! - `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds the crate as a Python extension module.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod accumulator;
pub mod config;
pub mod data_client;
pub mod data_iterator;
pub mod engine;
pub mod exchange;
pub mod execution_client;
pub mod modules;

#[cfg(feature = "streaming")]
pub mod node;

#[cfg(feature = "python")]
pub mod python;

#[cfg(feature = "ffi")]
pub mod ffi;
