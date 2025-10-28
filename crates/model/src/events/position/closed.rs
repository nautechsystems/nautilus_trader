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

use nautilus_core::{
    UUID4,
    nanos::{DurationNanos, UnixNanos},
};

use crate::{
    enums::{OrderSide, PositionSide},
    events::OrderFilled,
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    position::Position,
    types::{Currency, Money, Price, Quantity},
};

/// Represents an event where a position has been closed.
#[repr(C)]
#[derive(Clone, PartialEq, Debug)]
pub struct PositionClosed {
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
    /// The client order ID for the order which closed the position.
    pub closing_order_id: Option<ClientOrderId>,
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
    /// The average closing price.
    pub avg_px_close: Option<f64>,
    /// The realized return for the position.
    pub realized_return: f64,
    /// The realized PnL for the position (including commissions).
    pub realized_pnl: Option<Money>,
    /// The unrealized PnL for the position (including commissions).
    pub unrealized_pnl: Money,
    /// The total open duration (nanoseconds).
    pub duration: DurationNanos,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the position was opened.
    pub ts_opened: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the position was closed.
    pub ts_closed: Option<UnixNanos>,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

impl PositionClosed {
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
            closing_order_id: position.closing_order_id,
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
            duration: position.duration_ns,
            event_id,
            ts_opened: position.ts_opened,
            ts_closed: position.ts_closed,
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

    fn create_test_position_closed() -> PositionClosed {
        PositionClosed {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("EMA-CROSS"),
            instrument_id: InstrumentId::from("EURUSD.SIM"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-19700101-000000-001-001-1"),
            closing_order_id: Some(ClientOrderId::from("O-19700101-000000-001-001-2")),
            entry: OrderSide::Buy,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::from("0"),
            peak_quantity: Quantity::from("150"),
            last_qty: Quantity::from("150"),
            last_px: Price::from("1.0600"),
            currency: Currency::USD(),
            avg_px_open: 1.0525,
            avg_px_close: Some(1.0600),
            realized_return: 0.0071,
            realized_pnl: Some(Money::new(112.50, Currency::USD())),
            unrealized_pnl: Money::new(0.0, Currency::USD()),
            duration: 3_600_000_000_000, // 1 hour in nanoseconds
            event_id: Default::default(),
            ts_opened: UnixNanos::from(1_000_000_000),
            ts_closed: Some(UnixNanos::from(4_600_000_000)),
            ts_event: UnixNanos::from(4_600_000_000),
            ts_init: UnixNanos::from(5_000_000_000),
        }
    }

    fn create_test_order_filled() -> OrderFilled {
        OrderFilled::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("EURUSD.SIM"),
            ClientOrderId::from("O-19700101-000000-001-001-2"),
            VenueOrderId::from("2"),
            AccountId::from("SIM-001"),
            TradeId::from("T-002"),
            OrderSide::Sell,
            OrderType::Market,
            Quantity::from("150"),
            Price::from("1.0600"),
            Currency::USD(),
            LiquiditySide::Taker,
            Default::default(),
            UnixNanos::from(4_600_000_000),
            UnixNanos::from(5_000_000_000),
            false,
            Some(PositionId::from("P-001")),
            Some(Money::new(2.5, Currency::USD())),
        )
    }

    #[rstest]
    fn test_position_closed_new() {
        let position_closed = create_test_position_closed();

        assert_eq!(position_closed.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(position_closed.strategy_id, StrategyId::from("EMA-CROSS"));
        assert_eq!(
            position_closed.instrument_id,
            InstrumentId::from("EURUSD.SIM")
        );
        assert_eq!(position_closed.position_id, PositionId::from("P-001"));
        assert_eq!(position_closed.account_id, AccountId::from("SIM-001"));
        assert_eq!(
            position_closed.opening_order_id,
            ClientOrderId::from("O-19700101-000000-001-001-1")
        );
        assert_eq!(
            position_closed.closing_order_id,
            Some(ClientOrderId::from("O-19700101-000000-001-001-2"))
        );
        assert_eq!(position_closed.entry, OrderSide::Buy);
        assert_eq!(position_closed.side, PositionSide::Flat);
        assert_eq!(position_closed.signed_qty, 0.0);
        assert_eq!(position_closed.quantity, Quantity::from("0"));
        assert_eq!(position_closed.peak_quantity, Quantity::from("150"));
        assert_eq!(position_closed.last_qty, Quantity::from("150"));
        assert_eq!(position_closed.last_px, Price::from("1.0600"));
        assert_eq!(position_closed.currency, Currency::USD());
        assert_eq!(position_closed.avg_px_open, 1.0525);
        assert_eq!(position_closed.avg_px_close, Some(1.0600));
        assert_eq!(position_closed.realized_return, 0.0071);
        assert_eq!(
            position_closed.realized_pnl,
            Some(Money::new(112.50, Currency::USD()))
        );
        assert_eq!(
            position_closed.unrealized_pnl,
            Money::new(0.0, Currency::USD())
        );
        assert_eq!(position_closed.duration, 3_600_000_000_000);
        assert_eq!(position_closed.ts_opened, UnixNanos::from(1_000_000_000));
        assert_eq!(
            position_closed.ts_closed,
            Some(UnixNanos::from(4_600_000_000))
        );
        assert_eq!(position_closed.ts_event, UnixNanos::from(4_600_000_000));
        assert_eq!(position_closed.ts_init, UnixNanos::from(5_000_000_000));
    }

    #[rstest]
    fn test_position_closed_create() {
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
        let closing_fill = create_test_order_filled();
        let event_id = Default::default();
        let ts_init = UnixNanos::from(6_000_000_000);

        let position_closed = PositionClosed::create(&position, &closing_fill, event_id, ts_init);

        assert_eq!(position_closed.trader_id, position.trader_id);
        assert_eq!(position_closed.strategy_id, position.strategy_id);
        assert_eq!(position_closed.instrument_id, position.instrument_id);
        assert_eq!(position_closed.position_id, position.id);
        assert_eq!(position_closed.account_id, position.account_id);
        assert_eq!(position_closed.opening_order_id, position.opening_order_id);
        assert_eq!(position_closed.closing_order_id, position.closing_order_id);
        assert_eq!(position_closed.entry, position.entry);
        assert_eq!(position_closed.side, position.side);
        assert_eq!(position_closed.signed_qty, position.signed_qty);
        assert_eq!(position_closed.quantity, position.quantity);
        assert_eq!(position_closed.peak_quantity, position.peak_qty);
        assert_eq!(position_closed.last_qty, closing_fill.last_qty);
        assert_eq!(position_closed.last_px, closing_fill.last_px);
        assert_eq!(position_closed.currency, position.quote_currency);
        assert_eq!(position_closed.avg_px_open, position.avg_px_open);
        assert_eq!(position_closed.avg_px_close, position.avg_px_close);
        assert_eq!(position_closed.realized_return, position.realized_return);
        assert_eq!(position_closed.realized_pnl, position.realized_pnl);
        assert_eq!(
            position_closed.unrealized_pnl,
            Money::new(0.0, position.quote_currency)
        );
        assert_eq!(position_closed.duration, position.duration_ns);
        assert_eq!(position_closed.event_id, event_id);
        assert_eq!(position_closed.ts_opened, position.ts_opened);
        assert_eq!(position_closed.ts_closed, position.ts_closed);
        assert_eq!(position_closed.ts_event, closing_fill.ts_event);
        assert_eq!(position_closed.ts_init, ts_init);
    }

    #[rstest]
    fn test_position_closed_clone() {
        let position_closed1 = create_test_position_closed();
        let position_closed2 = position_closed1.clone();

        assert_eq!(position_closed1, position_closed2);
    }

    #[rstest]
    fn test_position_closed_debug() {
        let position_closed = create_test_position_closed();
        let debug_str = format!("{position_closed:?}");

        assert!(debug_str.contains("PositionClosed"));
        assert!(debug_str.contains("TRADER-001"));
        assert!(debug_str.contains("EMA-CROSS"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("P-001"));
    }

    #[rstest]
    fn test_position_closed_partial_eq() {
        let mut position_closed1 = create_test_position_closed();
        let mut position_closed2 = create_test_position_closed();
        let event_id = Default::default();
        position_closed1.event_id = event_id;
        position_closed2.event_id = event_id;

        let mut position_closed3 = create_test_position_closed();
        position_closed3.event_id = event_id;
        position_closed3.realized_return = 0.01;

        assert_eq!(position_closed1, position_closed2);
        assert_ne!(position_closed1, position_closed3);
    }

    #[rstest]
    fn test_position_closed_flat_position() {
        let position_closed = create_test_position_closed();

        assert_eq!(position_closed.side, PositionSide::Flat);
        assert_eq!(position_closed.signed_qty, 0.0);
        assert_eq!(position_closed.quantity, Quantity::from("0"));
        assert_eq!(
            position_closed.unrealized_pnl,
            Money::new(0.0, Currency::USD())
        );
    }

    #[rstest]
    fn test_position_closed_with_closing_order_id() {
        let position_closed = create_test_position_closed();

        assert!(position_closed.closing_order_id.is_some());
        assert_eq!(
            position_closed.closing_order_id,
            Some(ClientOrderId::from("O-19700101-000000-001-001-2"))
        );
    }

    #[rstest]
    fn test_position_closed_without_closing_order_id() {
        let mut position_closed = create_test_position_closed();
        position_closed.closing_order_id = None;

        assert!(position_closed.closing_order_id.is_none());
    }

    #[rstest]
    fn test_position_closed_with_realized_pnl() {
        let position_closed = create_test_position_closed();

        assert!(position_closed.realized_pnl.is_some());
        assert_eq!(
            position_closed.realized_pnl,
            Some(Money::new(112.50, Currency::USD()))
        );
        assert!(position_closed.realized_return > 0.0);
    }

    #[rstest]
    fn test_position_closed_loss_scenario() {
        let mut position_closed = create_test_position_closed();
        position_closed.avg_px_close = Some(1.0400); // Sold below open price
        position_closed.realized_return = -0.0119;
        position_closed.realized_pnl = Some(Money::new(-187.50, Currency::USD()));

        assert_eq!(position_closed.avg_px_close, Some(1.0400));
        assert!(position_closed.realized_return < 0.0);
        assert_eq!(
            position_closed.realized_pnl,
            Some(Money::new(-187.50, Currency::USD()))
        );
    }

    #[rstest]
    fn test_position_closed_duration() {
        let position_closed = create_test_position_closed();

        assert_eq!(position_closed.duration, 3_600_000_000_000); // 1 hour
        assert!(position_closed.duration > 0);
    }

    #[rstest]
    fn test_position_closed_timestamps() {
        let position_closed = create_test_position_closed();

        assert_eq!(position_closed.ts_opened, UnixNanos::from(1_000_000_000));
        assert_eq!(
            position_closed.ts_closed,
            Some(UnixNanos::from(4_600_000_000))
        );
        assert_eq!(position_closed.ts_event, UnixNanos::from(4_600_000_000));
        assert_eq!(position_closed.ts_init, UnixNanos::from(5_000_000_000));

        assert!(position_closed.ts_opened < position_closed.ts_closed.unwrap());
        assert_eq!(position_closed.ts_closed.unwrap(), position_closed.ts_event);
        assert!(position_closed.ts_event < position_closed.ts_init);
    }

    #[rstest]
    fn test_position_closed_peak_quantity() {
        let position_closed = create_test_position_closed();

        assert_eq!(position_closed.peak_quantity, Quantity::from("150"));
        assert!(position_closed.peak_quantity >= position_closed.quantity);
        assert_eq!(position_closed.last_qty, position_closed.peak_quantity);
    }

    #[rstest]
    fn test_position_closed_different_currencies() {
        let mut usd_position = create_test_position_closed();
        usd_position.currency = Currency::USD();

        let mut eur_position = create_test_position_closed();
        eur_position.currency = Currency::EUR();
        eur_position.unrealized_pnl = Money::new(0.0, Currency::EUR());

        assert_eq!(usd_position.currency, Currency::USD());
        assert_eq!(eur_position.currency, Currency::EUR());
        assert_ne!(usd_position, eur_position);
    }

    #[rstest]
    fn test_position_closed_entry_sides() {
        let mut buy_entry = create_test_position_closed();
        buy_entry.entry = OrderSide::Buy;

        let mut sell_entry = create_test_position_closed();
        sell_entry.entry = OrderSide::Sell;

        assert_eq!(buy_entry.entry, OrderSide::Buy);
        assert_eq!(sell_entry.entry, OrderSide::Sell);
    }

    #[rstest]
    fn test_position_closed_prices() {
        let position_closed = create_test_position_closed();

        assert_eq!(position_closed.avg_px_open, 1.0525);
        assert_eq!(position_closed.avg_px_close, Some(1.0600));
        assert_eq!(position_closed.last_px, Price::from("1.0600"));

        assert!(position_closed.avg_px_close.unwrap() > position_closed.avg_px_open);
    }

    #[rstest]
    fn test_position_closed_without_ts_closed() {
        let mut position_closed = create_test_position_closed();
        position_closed.ts_closed = None;

        assert!(position_closed.ts_closed.is_none());
    }
}
