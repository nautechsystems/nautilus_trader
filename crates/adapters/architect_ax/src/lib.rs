//! [NautilusTrader](http://nautilustrader.io) adapter for Ax's [AX Exchange](https://architect.exchange).
//!
//! [AX Exchange](https://architect.exchange) is the world's first centralized and regulated
//! exchange for perpetual futures on traditional underlying asset classes (FX, rates, metals,
//! energy, stock indexes). Designed for institutional and professional traders, it combines
//! innovations from digital asset perpetual exchanges with the safety and risk management of
//! traditional futures exchanges. Licensed under the [Bermuda Monetary Authority (BMA)](https://www.bma.bm/).
//!
//! The `nautilus-architect-ax` crate provides client bindings (HTTP & WebSocket), data models, and
//! helper utilities that wrap the official AX Exchange API.
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
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation:
//!
//! - `python`: Enables Python bindings via [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds as a Python extension module (used together with `python`).
//!
//! # Documentation
//!
//! - API reference: <https://docs.architect.exchange/api-reference/>
//! - Crate docs: <https://docs.rs/nautilus-architect-ax>

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod common;
pub mod config;
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;
