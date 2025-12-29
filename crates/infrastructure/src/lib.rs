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

//! Database and messaging infrastructure for [NautilusTrader](http://nautilustrader.io).
//!
//! The `nautilus-infrastructure` crate provides backend database implementations and message bus adapters
//! that enable NautilusTrader to scale from development to production deployments. This includes
//! enterprise-grade data persistence and messaging capabilities:
//!
//! - **Redis integration**: Cache database and message bus implementations using Redis.
//! - **PostgreSQL integration**: SQL-based cache database with comprehensive data models.
//! - **Connection management**: Robust connection handling with retry logic and health monitoring.
//! - **Serialization options**: Support for JSON and MessagePack encoding formats.
//! - **Python bindings**: PyO3 integration for seamless Python interoperability.
//!
//! The crate supports multiple database backends through feature flags, allowing users to choose
//! the appropriate infrastructure components for their specific deployment requirements and scale.
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
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `redis`: Enables the Redis cache database and message bus backing implementations.
//! - `postgres`: Enables the PostgreSQL SQLx models and cache database backend.
//! - `extension-module`: Builds the crate as a Python extension module.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

#[cfg(feature = "python")]
pub mod python;

#[cfg(feature = "redis")]
pub mod redis;

#[cfg(feature = "postgres")]
pub mod sql;
