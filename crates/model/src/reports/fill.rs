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

use std::fmt::Display;

use nautilus_core::{UUID4, UnixNanos};
use serde::{Deserialize, Serialize};

use crate::{
    enums::{LiquiditySide, OrderSide},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
    types::{Money, Price, Quantity},
};

/// Represents a fill report of a single order execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct FillReport {
    /// The account ID associated with the position.
    pub account_id: AccountId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The venue assigned order ID.
    pub venue_order_id: VenueOrderId,
    /// The trade match ID (assigned by the venue).
    pub trade_id: TradeId,
    /// The order side.
    pub order_side: OrderSide,
    /// The last fill quantity for the position.
    pub last_qty: Quantity,
    /// The last fill price for the position.
    pub last_px: Price,
    /// The commission generated from the fill.
    pub commission: Money,
    /// The liquidity side of the execution.
    pub liquidity_side: LiquiditySide,
    /// The unique identifier for the event.
    pub report_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// The client order ID.
    pub client_order_id: Option<ClientOrderId>,
    /// The position ID (assigned by the venue).
    pub venue_position_id: Option<PositionId>,
}

impl FillReport {
    /// Creates a new [`FillReport`] instance with required fields.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        trade_id: TradeId,
        order_side: OrderSide,
        last_qty: Quantity,
        last_px: Price,
        commission: Money,
        liquidity_side: LiquiditySide,
        client_order_id: Option<ClientOrderId>,
        venue_position_id: Option<PositionId>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        report_id: Option<UUID4>,
    ) -> Self {
        Self {
            account_id,
            instrument_id,
            venue_order_id,
            trade_id,
            order_side,
            last_qty,
            last_px,
            commission,
            liquidity_side,
            report_id: report_id.unwrap_or_default(),
            ts_event,
            ts_init,
            client_order_id,
            venue_position_id,
        }
    }

    /// Checks if the fill has a client order ID.
    #[must_use]
    pub const fn has_client_order_id(&self) -> bool {
        self.client_order_id.is_some()
    }

    /// Utility method to check if the fill has a venue position ID.
    #[must_use]
    pub const fn has_venue_position_id(&self) -> bool {
        self.venue_position_id.is_some()
    }
}

impl Display for FillReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FillReport(instrument={}, side={}, qty={}, last_px={}, trade_id={}, venue_order_id={}, commission={}, liquidity={})",
            self.instrument_id,
            self.order_side,
            self.last_qty,
            self.last_px,
            self.trade_id,
            self.venue_order_id,
            self.commission,
            self.liquidity_side,
        )
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
        enums::{LiquiditySide, OrderSide},
        identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
        types::{Currency, Money, Price, Quantity},
    };

    fn test_fill_report() -> FillReport {
        FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            VenueOrderId::from("1"),
            TradeId::from("1"),
            OrderSide::Buy,
            Quantity::from("100"),
            Price::from("0.80000"),
            Money::new(5.0, Currency::USD()),
            LiquiditySide::Taker,
            Some(ClientOrderId::from("O-19700101-000000-001-001-1")),
            Some(PositionId::from("P-001")),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        )
    }

    #[rstest]
    fn test_fill_report_new() {
        let report = test_fill_report();

        assert_eq!(report.account_id, AccountId::from("SIM-001"));
        assert_eq!(report.instrument_id, InstrumentId::from("AUDUSD.SIM"));
        assert_eq!(report.venue_order_id, VenueOrderId::from("1"));
        assert_eq!(report.trade_id, TradeId::from("1"));
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.last_qty, Quantity::from("100"));
        assert_eq!(report.last_px, Price::from("0.80000"));
        assert_eq!(report.commission, Money::new(5.0, Currency::USD()));
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-19700101-000000-001-001-1"))
        );
        assert_eq!(report.venue_position_id, Some(PositionId::from("P-001")));
        assert_eq!(report.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(report.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_fill_report_new_with_generated_report_id() {
        let report = FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            VenueOrderId::from("1"),
            TradeId::from("1"),
            OrderSide::Buy,
            Quantity::from("100"),
            Price::from("0.80000"),
            Money::new(5.0, Currency::USD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None, // No report ID provided, should generate one
        );

        // Should have a generated UUID
        assert_ne!(
            report.report_id.to_string(),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[rstest]
    fn test_has_client_order_id() {
        let mut report = test_fill_report();
        assert!(report.has_client_order_id());

        report.client_order_id = None;
        assert!(!report.has_client_order_id());
    }

    #[rstest]
    fn test_has_venue_position_id() {
        let mut report = test_fill_report();
        assert!(report.has_venue_position_id());

        report.venue_position_id = None;
        assert!(!report.has_venue_position_id());
    }

    #[rstest]
    fn test_display() {
        let report = test_fill_report();
        let display_str = format!("{report}");

        assert!(display_str.contains("FillReport"));
        assert!(display_str.contains("AUDUSD.SIM"));
        assert!(display_str.contains("BUY"));
        assert!(display_str.contains("100"));
        assert!(display_str.contains("0.80000"));
        assert!(display_str.contains("5.00 USD"));
        assert!(display_str.contains("TAKER"));
    }

    #[rstest]
    fn test_clone_and_equality() {
        let report1 = test_fill_report();
        let report2 = report1.clone();

        assert_eq!(report1, report2);
    }

    #[rstest]
    fn test_serialization_roundtrip() {
        let original = test_fill_report();

        // Test JSON serialization
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: FillReport = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_fill_report_with_different_liquidity_sides() {
        let maker_report = FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            VenueOrderId::from("1"),
            TradeId::from("1"),
            OrderSide::Buy,
            Quantity::from("100"),
            Price::from("0.80000"),
            Money::new(2.0, Currency::USD()),
            LiquiditySide::Maker,
            None,
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        );

        let taker_report = FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            VenueOrderId::from("2"),
            TradeId::from("2"),
            OrderSide::Sell,
            Quantity::from("100"),
            Price::from("0.80000"),
            Money::new(5.0, Currency::USD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        );

        assert_eq!(maker_report.liquidity_side, LiquiditySide::Maker);
        assert_eq!(taker_report.liquidity_side, LiquiditySide::Taker);
        assert_ne!(maker_report, taker_report);
    }

    #[rstest]
    fn test_fill_report_with_different_order_sides() {
        let buy_report = FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            VenueOrderId::from("1"),
            TradeId::from("1"),
            OrderSide::Buy,
            Quantity::from("100"),
            Price::from("0.80000"),
            Money::new(5.0, Currency::USD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        );

        let sell_report = FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            VenueOrderId::from("1"),
            TradeId::from("1"),
            OrderSide::Sell,
            Quantity::from("100"),
            Price::from("0.80000"),
            Money::new(5.0, Currency::USD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        );

        assert_eq!(buy_report.order_side, OrderSide::Buy);
        assert_eq!(sell_report.order_side, OrderSide::Sell);
        assert_ne!(buy_report, sell_report);
    }
}
