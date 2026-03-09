//! [NautilusTrader](http://nautilustrader.io) adapter for the
//! [Binance](https://www.binance.com/) cryptocurrency exchange.
//!
//! The `nautilus-binance` crate provides client bindings (HTTP & WebSocket), data
//! models, and helper utilities that wrap the official **Binance API**, covering:
//!
//! - Spot trading (api.binance.com)
//! - Spot margin trading
//! - USD-M Futures (fapi.binance.com)
//! - COIN-M Futures (dapi.binance.com)
//! - European Options (eapi.binance.com)
//!
//! The official Binance API reference can be found at <https://binance-docs.github.io/apidocs/>.
//!
//! # Platform
//!
//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! NautilusTrader's design, architecture, and implementation philosophy prioritizes software
//! correctness and safety at the highest level, with the aim of supporting mission-critical
//! trading system backtesting and live deployment workloads.
//!
//! # Feature Flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case (Rust-only builds vs. Python bindings through PyO3).
//!
//! - `python`: Enables Python bindings via [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds as a Python extension module (used together with `python`).
//!
//! [High-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) (128-bit value types) is enabled by default.
//!
//! # Documentation
//!
//! See <https://docs.rs/nautilus-binance> for the latest API documentation.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod common;
pub mod config;
pub mod factories;
pub mod futures;
pub mod spot;

#[cfg(feature = "python")]
pub mod python;
