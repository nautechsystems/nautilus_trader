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

//! [NautilusTrader](http://nautilustrader.io) adapter for the [dYdX](https://dydx.trade/) decentralized derivatives exchange.
//!
//! The `nautilus-dydx` crate provides client bindings (HTTP, WebSocket & gRPC), data
//! models, and helper utilities that wrap the official **dYdX v4 API**.
//!
//! # Official Documentation
//!
//! | Resource                             | Reference                                              |
//! |--------------------------------------|--------------------------------------------------------|
//! | Main documentation                   | <https://docs.dydx.xyz>                                |
//! | Indexer HTTP API                     | <https://docs.dydx.exchange/api_integration-indexer/indexer_api> |
//! | Indexer WebSocket API                | <https://docs.dydx.trade/developers/indexer/websockets> |
//! | Order types                          | <https://docs.dydx.xyz/concepts/trading/orders>        |
//! | Permissioned keys                    | <https://docs.dydx.xyz/concepts/trading/authenticators> |
//! | Validator client (gRPC)              | <https://docs.dydx.exchange/api_integration-clients/validator_client> |
//!
//! # Platform
//!
//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! NautilusTrader's design, architecture, and implementation philosophy prioritizes software
//! correctness and safety at the highest level, with the aim of supporting mission-critical trading
//! system backtesting and live deployment workloads.
//!
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds as a Python extension module (used with `python`).

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
