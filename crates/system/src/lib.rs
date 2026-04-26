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

//! System-level components and orchestration for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-system` crate provides the core system architecture for orchestrating trading systems,
//! including the kernel that manages all engines, configuration management,
//! and system-level factories for creating components:
//!
//! - `NautilusKernel` - Core system orchestrator managing engines and components.
//! - `NautilusKernelConfig` - Configuration for kernel initialization.
//! - System builders and factories for component creation.
//!
//! # NautilusTrader
//!
//! [NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
//! engine for multi-asset, multi-venue trading systems.
//!
//! The system spans research, deterministic simulation, and live execution within a single
//! event-driven architecture, providing research-to-live semantic parity.
//!
//! # Feature Flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `streaming`: Enables `persistence` dependency for streaming configuration.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs) (auto-enables `streaming`).
//! - `defi`: Enables DeFi (Decentralized Finance) support.
//! - `live`: Enables live trading mode dependencies.
//! - `tracing-bridge`: Enables the `tracing` subscriber bridge for log integration.
//! - `extension-module`: Builds the crate as a Python extension module.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod builder;
pub mod config;
pub mod controller;
pub mod kernel;
pub mod messages;
pub mod trader;

#[cfg(feature = "python")]
pub mod python;

// Re-exports
pub use builder::NautilusKernelBuilder;
pub use config::{NautilusKernelConfig, RotationConfig, StreamingConfig};
pub use controller::Controller;
pub use kernel::NautilusKernel;
pub use messages::ControllerCommand;
#[cfg(feature = "python")]
pub use python::{FactoryRegistry, get_global_pyo3_registry};
