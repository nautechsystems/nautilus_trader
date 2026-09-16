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

//! Kalshi integration adapter for the Nautilus trading engine.
//!
//! Kalshi is a regulated event-contract exchange. Every market is a binary (or scalar) contract
//! whose YES side pays the market's notional value on a win and nothing on a loss, and whose NO
//! side pays the complement. Two facts shape this adapter:
//!
//! - **One market is one instrument.** The YES and NO sides are the same
//!   `BinaryOption` instrument, quoted from the YES
//!   side. The instrument identifier is the market ticker, suffixed with the venue.
//! - **An event is the outcome group.** Markets inside an event carry the mutually exclusive
//!   outcomes of one real-world occurrence, so an event maps onto an
//!   `OutcomeGroup` whose legs are the event's markets
//!   and whose settlement is declared by the exchange per market.
//!
//! Prices and contract counts arrive as fixed-point strings (`*_dollars` and `*_fp`) and are
//! parsed into exact `Price` and
//! `Quantity` values; no value passes through a float.
//!
//! # Feature Flags
//!
//! - `extension-module`: Builds as a Python extension module.
//! - `high-precision`: Uses 128-bit integer backing for high-precision mode.
//! - `python`: Enables Python bindings from PyO3.

#![warn(rustc::all)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    reason = "documented at the module level and in the calling adapter"
)]
#![allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "PyO3 methods and trait method signatures require pass-by-reference parity"
)]
#![allow(
    clippy::unsafe_derive_deserialize,
    reason = "config and message types deserialize plain field values; unsafe in unrelated impls is sound"
)]
#![allow(
    clippy::unused_self,
    reason = "PyO3 methods and client trait operations take &self for interface parity"
)]

pub mod common;
pub mod config;
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod providers;

#[cfg(feature = "python")]
pub mod python;

pub use crate::{
    common::{credential::KalshiCredential, enums::KalshiEnvironment},
    config::{KalshiDataClientConfig, KalshiExecClientConfig},
    data::client::KalshiDataClient,
    execution::{client::KalshiExecutionClient, parse as execution_parse},
    factories::{KalshiDataClientFactory, KalshiExecutionClientFactory},
    http::client::KalshiHttpClient,
    providers::KalshiInstrumentProvider,
};
