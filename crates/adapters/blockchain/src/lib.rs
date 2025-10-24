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

//! Blockchain data adapter for [NautilusTrader](http://nautilustrader.io).
//!
//! The `nautilus-blockchain` crate provides a high-performance, universal, extensible adapter for ingesting
//! DeFi data from decentralized exchanges (DEXs), liquidity pools, and on-chain events.
//! It enables you to power analytics pipelines and trading strategies with real-time and historical
//! on-chain data.
//!
//! # Platform
//!
//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `hypersync`: Enables the [HyperSync](https://envio.dev/#hypersync) client integration.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds as a Python extension module (used with `python`).
//! - `turmoil`: Enables deterministic network simulation testing with [turmoil](https://github.com/tokio-rs/turmoil).

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod config;
pub mod contracts;
pub mod decode;
pub mod events;
pub mod math;
pub mod rpc;

#[cfg(feature = "hypersync")]
pub mod cache;

#[cfg(feature = "hypersync")]
pub mod data;

#[cfg(feature = "hypersync")]
pub mod exchanges;

#[cfg(feature = "hypersync")]
pub mod factories;

#[cfg(feature = "hypersync")]
pub mod hypersync;

#[cfg(feature = "python")]
pub mod python;
