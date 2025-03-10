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

use nautilus_core::{UUID4, UnixNanos};
use rust_decimal_macros::dec;

use super::{
    limit::LimitOrder, limit_if_touched::LimitIfTouchedOrder, market::MarketOrder,
    market_if_touched::MarketIfTouchedOrder, market_to_limit::MarketToLimitOrder,
    stop_limit::StopLimitOrder, stop_market::StopMarketOrder,
    trailing_stop_limit::TrailingStopLimitOrder, trailing_stop_market::TrailingStopMarketOrder,
};
use crate::{
    enums::{OrderSide, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
    types::{Price, Quantity},
};

impl Default for LimitOrder {
    /// Creates a new default [`LimitOrder`] instance for testing.
    fn default() -> Self {
        Self::new(
            TraderId::default(),
            StrategyId::default(),
            InstrumentId::default(),
            ClientOrderId::default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
        .unwrap() // SAFETY: Valid default values are used
    }
}

impl Default for LimitIfTouchedOrder {
    /// Creates a new default [`LimitIfTouchedOrder`] instance for testing.
    fn default() -> Self {
        Self::new(
            TraderId::default(),
            StrategyId::default(),
            InstrumentId::default(),
            ClientOrderId::default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl Default for MarketOrder {
    /// Creates a new default [`MarketOrder`] instance for testing.
    fn default() -> Self {
        Self::new(
            TraderId::default(),
            StrategyId::default(),
            InstrumentId::default(),
            ClientOrderId::default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            TimeInForce::Day,
            UUID4::default(),
            UnixNanos::default(),
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }
}

impl Default for MarketIfTouchedOrder {
    /// Creates a new default [`MarketIfTouchedOrder`] instance for testing.
    fn default() -> Self {
        Self::new(
            TraderId::default(),
            StrategyId::default(),
            InstrumentId::default(),
            ClientOrderId::default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            TimeInForce::Gtc,
            None,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl Default for MarketToLimitOrder {
    /// Creates a new default [`MarketToLimitOrder`] instance for testing.
    fn default() -> Self {
        Self::new(
            TraderId::default(),
            StrategyId::default(),
            InstrumentId::default(),
            ClientOrderId::default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl Default for StopLimitOrder {
    /// Creates a new default [`StopLimitOrder`] instance for testing.
    fn default() -> Self {
        Self::new(
            TraderId::default(),
            StrategyId::default(),
            InstrumentId::default(),
            ClientOrderId::default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl Default for StopMarketOrder {
    /// Creates a new default [`StopMarketOrder`] instance for testing.
    fn default() -> Self {
        Self::new(
            TraderId::default(),
            StrategyId::default(),
            InstrumentId::default(),
            ClientOrderId::default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            TimeInForce::Gtc,
            None,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl Default for TrailingStopLimitOrder {
    /// Creates a new default [`TrailingStopLimitOrder`] instance for testing.
    fn default() -> Self {
        Self::new(
            TraderId::default(),
            StrategyId::default(),
            InstrumentId::default(),
            ClientOrderId::default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            dec!(0.001),
            dec!(0.001),
            TrailingOffsetType::Price,
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl Default for TrailingStopMarketOrder {
    /// Creates a new default [`TrailingStopMarketOrder`] instance for testing.
    fn default() -> Self {
        Self::new(
            TraderId::default(),
            StrategyId::default(),
            InstrumentId::default(),
            ClientOrderId::default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            dec!(0.001),
            TrailingOffsetType::Price,
            TimeInForce::Gtc,
            None,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}
