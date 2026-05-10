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

//! [NautilusTrader](https://nautilustrader.io) adapter for Ax's [AX Exchange](https://architect.exchange).
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
//! # NautilusTrader
//!
//! [NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
//! engine for multi-asset, multi-venue trading systems.
//!
//! The system spans research, deterministic simulation, and live execution within a single
//! event-driven architecture, providing research-to-live semantic parity.
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
