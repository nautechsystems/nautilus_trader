// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! [NautilusTrader](http://nautilustrader.io) adapter for [Architect](https://architect.co/)
//! perpetual futures exchange and multi-asset brokerage (equities, futures, and options).
//!
//! [Architect](https://architect.co) provides institutional financial technology for 21st-century
//! capital markets. This crate provides connectivity to two Architect products:
//!
//! - **AX Exchange** (`AX`): A regulated perpetual futures exchange for traditional asset classes.
//! - **Architect Brokerage** (`ARCHITECT`): A US-regulated multi-asset brokerage for equities,
//!   futures, and options.
//!
//! The `nautilus-architect` crate provides client bindings (HTTP & WebSocket), data models, and
//! helper utilities that wrap the official Architect API.
//!
//! # AX Exchange
//!
//! [AX Exchange](https://architect.exchange) is the world's first centralized and regulated
//! exchange for perpetual futures on traditional underlying asset classes (FX, rates, metals,
//! energy, stock indexes). Designed for institutional and professional traders, it combines
//! innovations from digital asset perpetual exchanges with the safety and risk management of
//! traditional futures exchanges. Licensed under the Bermuda Monetary Authority.
//!
//! # Architect Brokerage
//!
//! [Architect](https://architect.co) operates a US-regulated institutional multi-asset brokerage offering
//! equities, futures, and options trading with full-featured APIs designed for professional
//! traders and trading firms. Architect Securities LLC is an SEC-registered broker-dealer
//! (FINRA/SIPC member), and Architect Financial Derivatives LLC is an NFA-registered
//! introducing broker.
//!
//! The brokerage integration is planned for future development. The current implementation
//! focuses on AX Exchange perpetual futures.
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
//! - API reference: <https://docs.sandbox.x.architect.co/api-reference/>
//! - Crate docs: <https://docs.rs/nautilus-architect>

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
pub mod error;
pub mod http;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;
