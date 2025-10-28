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
    types::{Currency, Money, Price, Quantity},
};

/// Represents an event where a position has changed.
#[repr(C)]
#[derive(Clone, PartialEq, Debug)]
pub struct PositionChanged {
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
    /// The peak directional quantity reached by the position.
    pub peak_quantity: Quantity,
    /// The last fill quantity for the position.
    pub last_qty: Quantity,
    /// The last fill price for the position.
    pub last_px: Price,
    /// The position quote currency.
    pub currency: Currency,
    /// The average open price.
    pub avg_px_open: f64,
    /// The average close price.
    pub avg_px_close: Option<f64>,
    /// The realized return for the position.
    pub realized_return: f64,
    /// The realized PnL for the position (including commissions).
    pub realized_pnl: Option<Money>,
    /// The unrealized PnL for the position (including commissions).
    pub unrealized_pnl: Money,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the position was opened.
    pub ts_opened: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

impl PositionChanged {
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
            peak_quantity: position.peak_qty,
            last_qty: fill.last_qty,
            last_px: fill.last_px,
            currency: position.quote_currency,
            avg_px_open: position.avg_px_open,
            avg_px_close: position.avg_px_close,
            realized_return: position.realized_return,
            realized_pnl: position.realized_pnl,
            unrealized_pnl: Money::new(0.0, position.quote_currency),
            event_id,
            ts_opened: position.ts_opened,
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

    fn create_test_position_changed() -> PositionChanged {
        PositionChanged {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("EMA-CROSS"),
            instrument_id: InstrumentId::from("EURUSD.SIM"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-19700101-000000-001-001-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 150.0,
            quantity: Quantity::from("150"),
            peak_quantity: Quantity::from("150"),
            last_qty: Quantity::from("50"),
            last_px: Price::from("1.0550"),
            currency: Currency::USD(),
            avg_px_open: 1.0525,
            avg_px_close: None,
            realized_return: 0.0,
            realized_pnl: None,
            unrealized_pnl: Money::new(75.0, Currency::USD()),
            event_id: Default::default(),
            ts_opened: UnixNanos::from(1_000_000_000),
            ts_event: UnixNanos::from(1_500_000_000),
            ts_init: UnixNanos::from(2_500_000_000),
        }
    }

    fn create_test_order_filled() -> OrderFilled {
        OrderFilled::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("AUD/USD.SIM"),
            ClientOrderId::from("O-19700101-000000-001-001-2"),
            VenueOrderId::from("2"),
            AccountId::from("SIM-001"),
            TradeId::from("T-002"),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("50"),
            Price::from("0.8050"),
            Currency::USD(),
            LiquiditySide::Taker,
            Default::default(),
            UnixNanos::from(1_500_000_000),
            UnixNanos::from(2_500_000_000),
            false,
            Some(PositionId::from("P-001")),
            Some(Money::new(1.0, Currency::USD())),
        )
    }

    #[rstest]
    fn test_position_changed_new() {
        let position_changed = create_test_position_changed();

        assert_eq!(position_changed.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(position_changed.strategy_id, StrategyId::from("EMA-CROSS"));
        assert_eq!(
            position_changed.instrument_id,
            InstrumentId::from("EURUSD.SIM")
        );
        assert_eq!(position_changed.position_id, PositionId::from("P-001"));
        assert_eq!(position_changed.account_id, AccountId::from("SIM-001"));
        assert_eq!(
            position_changed.opening_order_id,
            ClientOrderId::from("O-19700101-000000-001-001-1")
        );
        assert_eq!(position_changed.entry, OrderSide::Buy);
        assert_eq!(position_changed.side, PositionSide::Long);
        assert_eq!(position_changed.signed_qty, 150.0);
        assert_eq!(position_changed.quantity, Quantity::from("150"));
        assert_eq!(position_changed.peak_quantity, Quantity::from("150"));
        assert_eq!(position_changed.last_qty, Quantity::from("50"));
        assert_eq!(position_changed.last_px, Price::from("1.0550"));
        assert_eq!(position_changed.currency, Currency::USD());
        assert_eq!(position_changed.avg_px_open, 1.0525);
        assert_eq!(position_changed.avg_px_close, None);
        assert_eq!(position_changed.realized_return, 0.0);
        assert_eq!(position_changed.realized_pnl, None);
        assert_eq!(
            position_changed.unrealized_pnl,
            Money::new(75.0, Currency::USD())
        );
        assert_eq!(position_changed.ts_opened, UnixNanos::from(1_000_000_000));
        assert_eq!(position_changed.ts_event, UnixNanos::from(1_500_000_000));
        assert_eq!(position_changed.ts_init, UnixNanos::from(2_500_000_000));
    }

    #[rstest]
    fn test_position_changed_create() {
        let instrument = audusd_sim();
        let initial_fill = OrderFilled::new(
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
        );

        let position = Position::new(&InstrumentAny::CurrencyPair(instrument), initial_fill);
        let change_fill = create_test_order_filled();
        let event_id = Default::default();
        let ts_init = UnixNanos::from(3_000_000_000);

        let position_changed = PositionChanged::create(&position, &change_fill, event_id, ts_init);

        assert_eq!(position_changed.trader_id, position.trader_id);
        assert_eq!(position_changed.strategy_id, position.strategy_id);
        assert_eq!(position_changed.instrument_id, position.instrument_id);
        assert_eq!(position_changed.position_id, position.id);
        assert_eq!(position_changed.account_id, position.account_id);
        assert_eq!(position_changed.opening_order_id, position.opening_order_id);
        assert_eq!(position_changed.entry, position.entry);
        assert_eq!(position_changed.side, position.side);
        assert_eq!(position_changed.signed_qty, position.signed_qty);
        assert_eq!(position_changed.quantity, position.quantity);
        assert_eq!(position_changed.peak_quantity, position.peak_qty);
        assert_eq!(position_changed.last_qty, change_fill.last_qty);
        assert_eq!(position_changed.last_px, change_fill.last_px);
        assert_eq!(position_changed.currency, position.quote_currency);
        assert_eq!(position_changed.avg_px_open, position.avg_px_open);
        assert_eq!(position_changed.avg_px_close, position.avg_px_close);
        assert_eq!(position_changed.realized_return, position.realized_return);
        assert_eq!(position_changed.realized_pnl, position.realized_pnl);
        assert_eq!(
            position_changed.unrealized_pnl,
            Money::new(0.0, position.quote_currency)
        );
        assert_eq!(position_changed.event_id, event_id);
        assert_eq!(position_changed.ts_opened, position.ts_opened);
        assert_eq!(position_changed.ts_event, change_fill.ts_event);
        assert_eq!(position_changed.ts_init, ts_init);
    }

    #[rstest]
    fn test_position_changed_clone() {
        let position_changed1 = create_test_position_changed();
        let position_changed2 = position_changed1.clone();

        assert_eq!(position_changed1, position_changed2);
    }

    #[rstest]
    fn test_position_changed_debug() {
        let position_changed = create_test_position_changed();
        let debug_str = format!("{position_changed:?}");

        assert!(debug_str.contains("PositionChanged"));
        assert!(debug_str.contains("TRADER-001"));
        assert!(debug_str.contains("EMA-CROSS"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("P-001"));
    }

    #[rstest]
    fn test_position_changed_partial_eq() {
        let mut position_changed1 = create_test_position_changed();
        let mut position_changed2 = create_test_position_changed();
        let event_id = Default::default();
        position_changed1.event_id = event_id;
        position_changed2.event_id = event_id;

        let mut position_changed3 = create_test_position_changed();
        position_changed3.event_id = event_id;
        position_changed3.quantity = Quantity::from("200");

        assert_eq!(position_changed1, position_changed2);
        assert_ne!(position_changed1, position_changed3);
    }

    #[rstest]
    fn test_position_changed_with_pnl() {
        let mut position_changed = create_test_position_changed();
        position_changed.realized_pnl = Some(Money::new(25.0, Currency::USD()));
        position_changed.unrealized_pnl = Money::new(50.0, Currency::USD());

        assert_eq!(
            position_changed.realized_pnl,
            Some(Money::new(25.0, Currency::USD()))
        );
        assert_eq!(
            position_changed.unrealized_pnl,
            Money::new(50.0, Currency::USD())
        );
    }

    #[rstest]
    fn test_position_changed_with_closing_prices() {
        let mut position_changed = create_test_position_changed();
        position_changed.avg_px_close = Some(1.0575);
        position_changed.realized_return = 0.0048;

        assert_eq!(position_changed.avg_px_close, Some(1.0575));
        assert_eq!(position_changed.realized_return, 0.0048);
    }

    #[rstest]
    fn test_position_changed_peak_quantity() {
        let mut position_changed = create_test_position_changed();
        position_changed.peak_quantity = Quantity::from("300");

        assert_eq!(position_changed.peak_quantity, Quantity::from("300"));
        assert!(position_changed.peak_quantity >= position_changed.quantity);
    }

    #[rstest]
    fn test_position_changed_different_sides() {
        let mut long_position = create_test_position_changed();
        long_position.side = PositionSide::Long;
        long_position.signed_qty = 150.0;

        let mut short_position = create_test_position_changed();
        short_position.side = PositionSide::Short;
        short_position.signed_qty = -150.0;

        assert_eq!(long_position.side, PositionSide::Long);
        assert_eq!(long_position.signed_qty, 150.0);

        assert_eq!(short_position.side, PositionSide::Short);
        assert_eq!(short_position.signed_qty, -150.0);
    }

    #[rstest]
    fn test_position_changed_timestamps() {
        let position_changed = create_test_position_changed();

        assert_eq!(position_changed.ts_opened, UnixNanos::from(1_000_000_000));
        assert_eq!(position_changed.ts_event, UnixNanos::from(1_500_000_000));
        assert_eq!(position_changed.ts_init, UnixNanos::from(2_500_000_000));
        assert!(position_changed.ts_opened < position_changed.ts_event);
        assert!(position_changed.ts_event < position_changed.ts_init);
    }

    #[rstest]
    fn test_position_changed_quantities_relationship() {
        let position_changed = create_test_position_changed();

        assert!(position_changed.peak_quantity >= position_changed.quantity);
        assert!(position_changed.last_qty <= position_changed.quantity);
    }

    #[rstest]
    fn test_position_changed_with_zero_unrealized_pnl() {
        let mut position_changed = create_test_position_changed();
        position_changed.unrealized_pnl = Money::new(0.0, Currency::USD());

        assert_eq!(
            position_changed.unrealized_pnl,
            Money::new(0.0, Currency::USD())
        );
    }
}
