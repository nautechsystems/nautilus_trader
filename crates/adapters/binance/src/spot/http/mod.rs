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

//! Binance Spot HTTP client with SBE encoding support.

pub mod client;
pub mod error;
pub mod models;
pub mod parse;
pub mod query;

pub use client::{BinanceRawSpotHttpClient, BinanceSpotHttpClient, SBE_SCHEMA_HEADER};
pub use error::{BinanceSpotHttpError, BinanceSpotHttpResult, SbeDecodeError};
pub use models::{BinanceDepth, BinancePriceLevel, BinanceTrade, BinanceTrades};
pub use query::{DepthParams, TradesParams};
