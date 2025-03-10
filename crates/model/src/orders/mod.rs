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

//! Order types for the trading domain model.

#![allow(dead_code)]

pub mod any;
pub mod base;
pub mod builder;
pub mod default;
pub mod limit;
pub mod limit_if_touched;
pub mod list;
pub mod market;
pub mod market_if_touched;
pub mod market_to_limit;
pub mod stop_limit;
pub mod stop_market;
pub mod trailing_stop_limit;
pub mod trailing_stop_market;

#[cfg(feature = "stubs")]
pub mod stubs;

// Re-exports
pub use crate::orders::{
    any::{LimitOrderAny, OrderAny, PassiveOrderAny, StopOrderAny},
    base::{Order, OrderError},
    builder::OrderTestBuilder,
    limit::LimitOrder,
    limit_if_touched::LimitIfTouchedOrder,
    list::OrderList,
    market::MarketOrder,
    market_if_touched::MarketIfTouchedOrder,
    market_to_limit::MarketToLimitOrder,
    stop_limit::StopLimitOrder,
    stop_market::StopMarketOrder,
    trailing_stop_limit::TrailingStopLimitOrder,
    trailing_stop_market::TrailingStopMarketOrder,
};
