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

use crate::{
    enums::{OrderSide, PositionSide},
    events::OrderFilled,
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    position::Position,
    types::{Currency, Price, Quantity},
};

/// Represents an event where a position has been opened.
#[repr(C)]
#[derive(Clone, PartialEq, Debug)]
pub struct PositionOpened {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The position ID associated with the event.
    pub position_id: PositionId,
    /// The account ID associated with the position.
    pub account_id: AccountId,
    /// The client order ID for the order which opened the position.
    pub opening_order_id: ClientOrderId,
    /// The position entry order side.
    pub entry: OrderSide,
    /// The position side.
    pub side: PositionSide,
    /// The current signed quantity (positive for position side `LONG`, negative for `SHORT`).
    pub signed_qty: f64,
    /// The current open quantity.
    pub quantity: Quantity,
    /// The last fill quantity for the position.
    pub last_qty: Quantity,
    /// The last fill price for the position.
    pub last_px: Price,
    /// The position quote currency.
    pub currency: Currency,
    /// The average open price.
    pub avg_px_open: f64,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

impl PositionOpened {
    pub fn create(
        position: &Position,
        fill: &OrderFilled,
        event_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id: position.trader_id,
            strategy_id: position.strategy_id,
            instrument_id: position.instrument_id,
            position_id: position.id,
            account_id: position.account_id,
            opening_order_id: position.opening_order_id,
            entry: position.entry,
            side: position.side,
            signed_qty: position.signed_qty,
            quantity: position.quantity,
            last_qty: fill.last_qty,
            last_px: fill.last_px,
            currency: position.quote_currency,
            avg_px_open: position.avg_px_open,
            event_id,
            ts_event: fill.ts_event,
            ts_init,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::*;

    use super::*;
    use crate::{
        enums::{LiquiditySide, OrderSide, OrderType, PositionSide},
        events::OrderFilled,
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
            VenueOrderId,
        },
        instruments::{InstrumentAny, stubs::audusd_sim},
        position::Position,
        types::{Currency, Money, Price, Quantity},
    };

    fn create_test_position_opened() -> PositionOpened {
        PositionOpened {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("EMA-CROSS"),
            instrument_id: InstrumentId::from("EURUSD.SIM"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-19700101-000000-001-001-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 100.0,
            quantity: Quantity::from("100"),
            last_qty: Quantity::from("100"),
            last_px: Price::from("1.0500"),
            currency: Currency::USD(),
            avg_px_open: 1.0500,
            event_id: Default::default(),
            ts_event: UnixNanos::from(1_000_000_000),
            ts_init: UnixNanos::from(2_000_000_000),
        }
    }

    fn create_test_order_filled() -> OrderFilled {
        OrderFilled::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("AUD/USD.SIM"),
            ClientOrderId::from("O-19700101-000000-001-001-1"),
            VenueOrderId::from("1"),
            AccountId::from("SIM-001"),
            TradeId::from("T-001"),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("100"),
            Price::from("0.8000"),
            Currency::USD(),
            LiquiditySide::Taker,
            Default::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            false,
            Some(PositionId::from("P-001")),
            Some(Money::new(2.0, Currency::USD())),
        )
    }

    #[rstest]
    fn test_position_opened_new() {
        let position_opened = create_test_position_opened();

        assert_eq!(position_opened.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(position_opened.strategy_id, StrategyId::from("EMA-CROSS"));
        assert_eq!(
            position_opened.instrument_id,
            InstrumentId::from("EURUSD.SIM")
        );
        assert_eq!(position_opened.position_id, PositionId::from("P-001"));
        assert_eq!(position_opened.account_id, AccountId::from("SIM-001"));
        assert_eq!(
            position_opened.opening_order_id,
            ClientOrderId::from("O-19700101-000000-001-001-1")
        );
        assert_eq!(position_opened.entry, OrderSide::Buy);
        assert_eq!(position_opened.side, PositionSide::Long);
        assert_eq!(position_opened.signed_qty, 100.0);
        assert_eq!(position_opened.quantity, Quantity::from("100"));
        assert_eq!(position_opened.last_qty, Quantity::from("100"));
        assert_eq!(position_opened.last_px, Price::from("1.0500"));
        assert_eq!(position_opened.currency, Currency::USD());
        assert_eq!(position_opened.avg_px_open, 1.0500);
        assert_eq!(position_opened.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(position_opened.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_position_opened_create() {
        let instrument = audusd_sim();
        let fill = create_test_order_filled();
        let position = Position::new(&InstrumentAny::CurrencyPair(instrument), fill);
        let event_id = Default::default();
        let ts_init = UnixNanos::from(3_000_000_000);

        let position_opened = PositionOpened::create(&position, &fill, event_id, ts_init);

        assert_eq!(position_opened.trader_id, position.trader_id);
        assert_eq!(position_opened.strategy_id, position.strategy_id);
        assert_eq!(position_opened.instrument_id, position.instrument_id);
        assert_eq!(position_opened.position_id, position.id);
        assert_eq!(position_opened.account_id, position.account_id);
        assert_eq!(position_opened.opening_order_id, position.opening_order_id);
        assert_eq!(position_opened.entry, position.entry);
        assert_eq!(position_opened.side, position.side);
        assert_eq!(position_opened.signed_qty, position.signed_qty);
        assert_eq!(position_opened.quantity, position.quantity);
        assert_eq!(position_opened.last_qty, fill.last_qty);
        assert_eq!(position_opened.last_px, fill.last_px);
        assert_eq!(position_opened.currency, position.quote_currency);
        assert_eq!(position_opened.avg_px_open, position.avg_px_open);
        assert_eq!(position_opened.event_id, event_id);
        assert_eq!(position_opened.ts_event, fill.ts_event);
        assert_eq!(position_opened.ts_init, ts_init);
    }

    #[rstest]
    fn test_position_opened_clone() {
        let position_opened1 = create_test_position_opened();
        let position_opened2 = position_opened1.clone();

        assert_eq!(position_opened1, position_opened2);
    }

    #[rstest]
    fn test_position_opened_debug() {
        let position_opened = create_test_position_opened();
        let debug_str = format!("{position_opened:?}");

        assert!(debug_str.contains("PositionOpened"));
        assert!(debug_str.contains("TRADER-001"));
        assert!(debug_str.contains("EMA-CROSS"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("P-001"));
    }

    #[rstest]
    fn test_position_opened_partial_eq() {
        let mut position_opened1 = create_test_position_opened();
        let mut position_opened2 = create_test_position_opened();
        let event_id = Default::default();
        position_opened1.event_id = event_id;
        position_opened2.event_id = event_id;

        let mut position_opened3 = create_test_position_opened();
        position_opened3.event_id = event_id;
        position_opened3.quantity = Quantity::from("200");

        assert_eq!(position_opened1, position_opened2);
        assert_ne!(position_opened1, position_opened3);
    }

    #[rstest]
    fn test_position_opened_with_different_sides() {
        let mut long_position = create_test_position_opened();
        long_position.side = PositionSide::Long;
        long_position.entry = OrderSide::Buy;
        long_position.signed_qty = 100.0;

        let mut short_position = create_test_position_opened();
        short_position.side = PositionSide::Short;
        short_position.entry = OrderSide::Sell;
        short_position.signed_qty = -100.0;

        assert_eq!(long_position.side, PositionSide::Long);
        assert_eq!(long_position.entry, OrderSide::Buy);
        assert_eq!(long_position.signed_qty, 100.0);

        assert_eq!(short_position.side, PositionSide::Short);
        assert_eq!(short_position.entry, OrderSide::Sell);
        assert_eq!(short_position.signed_qty, -100.0);
    }

    #[rstest]
    fn test_position_opened_different_currencies() {
        let mut usd_position = create_test_position_opened();
        usd_position.currency = Currency::USD();

        let mut eur_position = create_test_position_opened();
        eur_position.currency = Currency::EUR();

        assert_eq!(usd_position.currency, Currency::USD());
        assert_eq!(eur_position.currency, Currency::EUR());
        assert_ne!(usd_position, eur_position);
    }

    #[rstest]
    fn test_position_opened_timestamps() {
        let position_opened = create_test_position_opened();

        assert_eq!(position_opened.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(position_opened.ts_init, UnixNanos::from(2_000_000_000));
        assert!(position_opened.ts_event < position_opened.ts_init);
    }

    #[rstest]
    fn test_position_opened_quantities() {
        let mut position_opened = create_test_position_opened();
        position_opened.quantity = Quantity::from("500");
        position_opened.last_qty = Quantity::from("250");

        assert_eq!(position_opened.quantity, Quantity::from("500"));
        assert_eq!(position_opened.last_qty, Quantity::from("250"));
    }

    #[rstest]
    fn test_position_opened_prices() {
        let mut position_opened = create_test_position_opened();
        position_opened.last_px = Price::from("1.2345");
        position_opened.avg_px_open = 1.2345;

        assert_eq!(position_opened.last_px, Price::from("1.2345"));
        assert_eq!(position_opened.avg_px_open, 1.2345);
    }
}
