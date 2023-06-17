// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    cmp,
    fmt::{Display, Formatter},
};

use nautilus_core::{correctness, time::UnixNanos};
use pyo3::prelude::*;

use crate::{
    enums::{AggressorSide, PriceType},
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    types::{fixed::FIXED_PRECISION, price::Price, quantity::Quantity},
};

/// Represents a single quote tick in a financial market.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[pyclass]
pub struct QuoteTick {
    pub instrument_id: InstrumentId,
    pub bid: Price,
    pub ask: Price,
    pub bid_size: Quantity,
    pub ask_size: Quantity,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl QuoteTick {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        bid: Price,
        ask: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        correctness::u8_equal(
            bid.precision,
            ask.precision,
            "bid.precision",
            "ask.precision",
        );
        correctness::u8_equal(
            bid_size.precision,
            ask_size.precision,
            "bid_size.precision",
            "ask_size.precision",
        );
        Self {
            instrument_id,
            bid,
            ask,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        }
    }

    #[must_use]
    pub fn extract_price(&self, price_type: PriceType) -> Price {
        match price_type {
            PriceType::Bid => self.bid,
            PriceType::Ask => self.ask,
            PriceType::Mid => Price::from_raw(
                (self.bid.raw + self.ask.raw) / 2,
                cmp::min(self.bid.precision + 1, FIXED_PRECISION),
            ),
            _ => panic!("Cannot extract with price type {price_type}"),
        }
    }
}

impl Display for QuoteTick {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            self.instrument_id, self.bid, self.ask, self.bid_size, self.ask_size, self.ts_event,
        )
    }
}

/// Represents a single trade tick in a financial market.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[pyclass]
pub struct TradeTick {
    pub instrument_id: InstrumentId,
    pub price: Price,
    pub size: Quantity,
    pub aggressor_side: AggressorSide,
    pub trade_id: TradeId,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl TradeTick {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        price: Price,
        size: Quantity,
        aggressor_side: AggressorSide,
        trade_id: TradeId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        }
    }
}

impl Display for TradeTick {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            self.instrument_id,
            self.price,
            self.size,
            self.aggressor_side,
            self.trade_id,
            self.ts_event,
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use crate::{
        data::tick::{QuoteTick, TradeTick},
        enums::{AggressorSide, PriceType},
        identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
        types::{price::Price, quantity::Quantity},
    };

    #[test]
    fn test_quote_tick_to_string() {
        let tick = QuoteTick {
            instrument_id: InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap(),
            bid: Price::new(10000.0, 4),
            ask: Price::new(10001.0, 4),
            bid_size: Quantity::new(1.0, 8),
            ask_size: Quantity::new(1.0, 8),
            ts_event: 0,
            ts_init: 0,
        };
        assert_eq!(
            tick.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,10001.0000,1.00000000,1.00000000,0"
        );
    }

    #[rstest(
        input,
        expected,
        case(PriceType::Bid, 10_000_000_000_000),
        case(PriceType::Ask, 10_001_000_000_000),
        case(PriceType::Mid, 10_000_500_000_000)
    )]
    fn test_quote_tick_extract_price(input: PriceType, expected: i64) {
        let tick = QuoteTick {
            instrument_id: InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap(),
            bid: Price::new(10000.0, 4),
            ask: Price::new(10001.0, 4),
            bid_size: Quantity::new(1.0, 8),
            ask_size: Quantity::new(1.0, 8),
            ts_event: 0,
            ts_init: 0,
        };

        let result = tick.extract_price(input).raw;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_trade_tick_to_string() {
        let tick = TradeTick {
            instrument_id: InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap(),
            price: Price::new(10000.0, 4),
            size: Quantity::new(1.0, 8),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("123456789"),
            ts_event: 0,
            ts_init: 0,
        };
        assert_eq!(
            tick.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,1.00000000,BUYER,123456789,0"
        );
    }
}
