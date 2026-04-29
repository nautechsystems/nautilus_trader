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

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{
    enums::{OrderSide, PositionSide},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    position::Position,
    types::{Currency, Money, Quantity},
};

/// Represents a position state snapshot as a certain instant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct PositionSnapshot {
    /// The trader ID associated with the snapshot.
    pub trader_id: TraderId,
    /// The strategy ID associated with the snapshot.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the snapshot.
    pub instrument_id: InstrumentId,
    /// The position ID associated with the snapshot.
    pub position_id: PositionId,
    /// The account ID associated with the position.
    pub account_id: AccountId,
    /// The client order ID for the order which opened the position.
    pub opening_order_id: ClientOrderId,
    /// The client order ID for the order which closed the position.
    pub closing_order_id: Option<ClientOrderId>,
    /// The entry direction from open.
    pub entry: OrderSide,
    /// The position side.
    pub side: PositionSide,
    /// The position signed quantity (positive for LONG, negative for SHOT).
    pub signed_qty: f64,
    /// The position open quantity.
    pub quantity: Quantity,
    /// The peak directional quantity reached by the position.
    pub peak_qty: Quantity,
    /// The position quote currency.
    pub quote_currency: Currency,
    /// The position base currency.
    pub base_currency: Option<Currency>,
    /// The position settlement currency.
    pub settlement_currency: Currency,
    /// The average open price.
    pub avg_px_open: f64,
    /// The average closing price.
    pub avg_px_close: Option<f64>,
    /// The realized return for the position.
    pub realized_return: Option<f64>,
    /// The realized PnL for the position (including commissions).
    pub realized_pnl: Option<Money>,
    /// The unrealized PnL for the position (including commissions).
    pub unrealized_pnl: Option<Money>,
    /// The commissions for the position.
    pub commissions: Vec<Money>,
    /// The open duration for the position (nanoseconds).
    pub duration_ns: Option<u64>,
    /// UNIX timestamp (nanoseconds) when the position opened.
    pub ts_opened: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the position closed.
    pub ts_closed: Option<UnixNanos>,
    /// UNIX timestamp (nanoseconds) when the snapshot was initialized.
    pub ts_init: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the last position event occurred.
    pub ts_last: UnixNanos,
}

impl PositionSnapshot {
    #[must_use]
    pub fn from(position: &Position, unrealized_pnl: Option<Money>) -> Self {
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
            peak_qty: position.peak_qty,
            quote_currency: position.quote_currency,
            base_currency: position.base_currency,
            settlement_currency: position.settlement_currency,
            avg_px_open: position.avg_px_open,
            avg_px_close: position.avg_px_close,
            realized_return: Some(position.realized_return), // TODO: Standardize
            realized_pnl: position.realized_pnl,
            unrealized_pnl,
            commissions: position.commissions.values().copied().collect(), // TODO: Optimize
            duration_ns: Some(position.duration_ns),                       // TODO: Standardize
            ts_opened: position.ts_opened,
            ts_closed: position.ts_closed,
            ts_init: position.ts_init,
            ts_last: position.ts_last,
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
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

    fn create_test_position_snapshot() -> PositionSnapshot {
        PositionSnapshot {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("EMA-CROSS"),
            instrument_id: InstrumentId::from("EURUSD.SIM"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-19700101-000000-001-001-1"),
            closing_order_id: Some(ClientOrderId::from("O-19700101-000000-001-001-2")),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 100.0,
            quantity: Quantity::from("100"),
            peak_qty: Quantity::from("100"),
            quote_currency: Currency::USD(),
            base_currency: Some(Currency::EUR()),
            settlement_currency: Currency::USD(),
            avg_px_open: 1.0500,
            avg_px_close: Some(1.0600),
            realized_return: Some(0.0095),
            realized_pnl: Some(Money::new(100.0, Currency::USD())),
            unrealized_pnl: Some(Money::new(50.0, Currency::USD())),
            commissions: vec![Money::new(2.0, Currency::USD())],
            duration_ns: Some(3_600_000_000_000), // 1 hour in nanoseconds
            ts_opened: UnixNanos::from(1_000_000_000),
            ts_closed: Some(UnixNanos::from(4_600_000_000)),
            ts_init: UnixNanos::from(2_000_000_000),
            ts_last: UnixNanos::from(4_600_000_000),
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
            UUID4::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            false,
            Some(PositionId::from("P-001")),
            Some(Money::new(2.0, Currency::USD())),
        )
    }

    #[rstest]
    fn test_position_snapshot_from() {
        let instrument = audusd_sim();
        let fill = create_test_order_filled();
        let position = Position::new(&InstrumentAny::CurrencyPair(instrument), fill);
        let unrealized_pnl = Some(Money::new(75.0, Currency::USD()));

        let snapshot = PositionSnapshot::from(&position, unrealized_pnl);

        assert_eq!(snapshot.trader_id, position.trader_id);
        assert_eq!(snapshot.strategy_id, position.strategy_id);
        assert_eq!(snapshot.instrument_id, position.instrument_id);
        assert_eq!(snapshot.position_id, position.id);
        assert_eq!(snapshot.account_id, position.account_id);
        assert_eq!(snapshot.opening_order_id, position.opening_order_id);
        assert_eq!(snapshot.closing_order_id, position.closing_order_id);
        assert_eq!(snapshot.entry, position.entry);
        assert_eq!(snapshot.side, position.side);
        assert_eq!(snapshot.signed_qty, position.signed_qty);
        assert_eq!(snapshot.quantity, position.quantity);
        assert_eq!(snapshot.peak_qty, position.peak_qty);
        assert_eq!(snapshot.quote_currency, position.quote_currency);
        assert_eq!(snapshot.base_currency, position.base_currency);
        assert_eq!(snapshot.settlement_currency, position.settlement_currency);
        assert_eq!(snapshot.avg_px_open, position.avg_px_open);
        assert_eq!(snapshot.avg_px_close, position.avg_px_close);
        assert_eq!(snapshot.realized_return, Some(position.realized_return));
        assert_eq!(snapshot.realized_pnl, position.realized_pnl);
        assert_eq!(snapshot.unrealized_pnl, unrealized_pnl);
        assert_eq!(snapshot.duration_ns, Some(position.duration_ns));
        assert_eq!(snapshot.ts_opened, position.ts_opened);
        assert_eq!(snapshot.ts_closed, position.ts_closed);
        assert_eq!(snapshot.ts_init, position.ts_init);
        assert_eq!(snapshot.ts_last, position.ts_last);
    }

    #[rstest]
    fn test_position_snapshot_from_with_no_unrealized_pnl() {
        let instrument = audusd_sim();
        let fill = create_test_order_filled();
        let position = Position::new(&InstrumentAny::CurrencyPair(instrument), fill);

        let snapshot = PositionSnapshot::from(&position, None);

        assert_eq!(snapshot.unrealized_pnl, None);
    }

    #[rstest]
    fn test_position_snapshot_serialization() {
        let original = create_test_position_snapshot();

        // Test JSON serialization
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PositionSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }
}
