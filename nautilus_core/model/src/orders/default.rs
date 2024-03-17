// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::uuid::UUID4;

use super::{
    limit::LimitOrder, limit_if_touched::LimitIfTouchedOrder, market::MarketOrder,
    market_if_touched::MarketIfTouchedOrder, market_to_limit::MarketToLimitOrder,
    stop_limit::StopLimitOrder, stop_market::StopMarketOrder,
    trailing_stop_limit::TrailingStopLimitOrder, trailing_stop_market::TrailingStopMarketOrder,
};
use crate::{
    enums::{OrderSide, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::{
        client_order_id::ClientOrderId, instrument_id::InstrumentId, strategy_id::StrategyId,
        trader_id::TraderId,
    },
    types::{price::Price, quantity::Quantity},
};

/// Provides a default [`LimitOrder`] used for testing.
impl Default for LimitOrder {
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
            0,
        )
        .unwrap() // SAFETY: Valid default values are used
    }
}

/// Provides a default [`LimitIfTouchedOrder`] used for testing.
impl Default for LimitIfTouchedOrder {
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
            0,
        )
        .unwrap() // SAFETY: Valid default values are used
    }
}

/// Provides a default [`MarketOrder`] used for testing.
impl Default for MarketOrder {
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
            0,
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
        .unwrap() // SAFETY: Valid default values are used
    }
}

/// Provides a default [`MarketIfTouchedOrder`] used for testing.
impl Default for MarketIfTouchedOrder {
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
            0,
        )
        .unwrap() // SAFETY: Valid default values are used
    }
}

/// Provides a default [`MarketToLimitOrder`] used for testing.
impl Default for MarketToLimitOrder {
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
            0,
        )
        .unwrap() // SAFETY: Valid default values are used
    }
}

/// Provides a default [`StopLimitOrder`] used for testing.
impl Default for StopLimitOrder {
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
            0,
        )
        .unwrap() // SAFETY: Valid default values are used
    }
}

/// Provides a default [`StopMarketOrder`] used for testing.
impl Default for StopMarketOrder {
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
            0,
        )
        .unwrap() // SAFETY: Valid default values are used
    }
}

/// Provides a default [`TrailingStopLimitOrder`] used for testing.
impl Default for TrailingStopLimitOrder {
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
            Price::from("0.00100"),
            Price::from("0.00100"),
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
            0,
        )
        .unwrap() // SAFETY: Valid default values are used
    }
}

/// Provides a default [`TrailingStopMarketOrder`] used for testing.
impl Default for TrailingStopMarketOrder {
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
            Price::from("0.00100"),
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
            0,
        )
        .unwrap() // SAFETY: Valid default values are used
    }
}
