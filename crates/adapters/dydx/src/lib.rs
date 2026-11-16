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

//! [NautilusTrader](https://nautilustrader.io) adapter for the [dYdX](https://dydx.trade/) decentralized derivatives exchange.
//!
//! The `nautilus-dydx` crate provides client bindings (HTTP, WebSocket & gRPC) and data
//! models for the official **dYdX v4 API**.
//!
//! The official dYdX API reference can be found at <https://docs.dydx.xyz/>.
//!
//! # NautilusTrader
//!
//! [NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
//! with a deterministic, event-driven architecture for multi-asset, multi-venue systems.
//!
//! The system spans research, simulation, and live execution, providing research-to-live semantic parity.
//!
//! # Feature Flags
//!
//! Feature flags control code inclusion for Rust builds and Python bindings:
//!
//! - `examples`: Enables the crate's example binaries.
//! - `extension-module`: Builds as a Python extension module.
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//!
//! # Integrator Attribution
//!
//! Orders built by the gRPC `OrderBuilder` set `order_router_address` to NautilusTrader's address.
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/dydx/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/dydx/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/dydx)

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]
// pyo3's `from_py_object` generates `.clone()` on `Copy` fields that clippy flags from the
// macro expansion; an item-level `allow` cannot reach the expansion
#![allow(clippy::clone_on_copy)]

pub mod common;
pub mod config;
pub mod data;
pub mod error;
pub mod execution;
pub mod factories;
pub mod grpc;
pub mod http;
pub mod proto;
pub mod types;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;

pub use crate::{
    common::{
        enums::{
            DydxCandleResolution, DydxMarketStatus, DydxOrderSide, DydxOrderStatus, DydxOrderType,
            DydxTickerType, DydxTimeInForce,
        },
        models::DydxAccount,
    },
    data::DydxDataClient,
    error::DydxError,
    factories::{DydxDataClientFactory, DydxExecutionClientFactory},
    http::{
        client::DydxHttpClient,
        error::DydxHttpError,
        models::{MarketsResponse, PerpetualMarket},
    },
    types::DydxOraclePrice,
    websocket::{
        client::DydxWebSocketClient,
        enums::{DydxWsChannel, DydxWsOperation},
        error::DydxWsError,
    },
};
