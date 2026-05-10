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
//! The `nautilus-dydx` crate provides client bindings (HTTP, WebSocket & gRPC), data
//! models, and helper utilities that wrap the official **dYdX v4 API**.
//!
//! # Official Documentation
//!
//! | Resource                             | Reference                                              |
//! |--------------------------------------|--------------------------------------------------------|
//! | Main documentation                   | <https://docs.dydx.xyz>                                |
//! | Indexer HTTP API                     | <https://docs.dydx.xyz/api_integration-indexer/indexer_api> |
//! | Indexer WebSocket API                | <https://docs.dydx.xyz/api_integration-indexer/indexer_websocket> |
//! | Order types                          | <https://docs.dydx.xyz/concepts/trading/orders>        |
//! | Permissioned keys                    | <https://docs.dydx.xyz/concepts/trading/authenticators> |
//! | Validator client (gRPC)              | <https://docs.dydx.xyz/api_integration-clients/validator_client> |
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
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds as a Python extension module.
//!
//! [High-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) (128-bit value types) is enabled by default.

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
