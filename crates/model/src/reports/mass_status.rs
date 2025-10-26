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

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use serde::{Deserialize, Serialize};

use crate::{
    identifiers::{AccountId, ClientId, InstrumentId, Venue, VenueOrderId},
    reports::{fill::FillReport, order::OrderStatusReport, position::PositionStatusReport},
};

/// Represents an execution mass status report for an execution client - including
/// status of all orders, trades for those orders and open positions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct ExecutionMassStatus {
    /// The client ID for the report.
    pub client_id: ClientId,
    /// The account ID for the report.
    pub account_id: AccountId,
    /// The venue for the report.
    pub venue: Venue,
    /// The report ID.
    pub report_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the object was initialized.
    pub ts_init: UnixNanos,
    /// The order status reports.
    order_reports: IndexMap<VenueOrderId, OrderStatusReport>,
    /// The fill reports.
    fill_reports: IndexMap<VenueOrderId, Vec<FillReport>>,
    /// The position status reports.
    position_reports: IndexMap<InstrumentId, Vec<PositionStatusReport>>,
}

impl ExecutionMassStatus {
    /// Creates a new execution mass status report.
    #[must_use]
    pub fn new(
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        ts_init: UnixNanos,
        report_id: Option<UUID4>,
    ) -> Self {
        Self {
            client_id,
            account_id,
            venue,
            report_id: report_id.unwrap_or_default(),
            ts_init,
            order_reports: IndexMap::new(),
            fill_reports: IndexMap::new(),
            position_reports: IndexMap::new(),
        }
    }

    /// Get a copy of the order reports map.
    #[must_use]
    pub fn order_reports(&self) -> IndexMap<VenueOrderId, OrderStatusReport> {
        self.order_reports.clone()
    }

    /// Get a copy of the fill reports map.
    #[must_use]
    pub fn fill_reports(&self) -> IndexMap<VenueOrderId, Vec<FillReport>> {
        self.fill_reports.clone()
    }

    /// Get a copy of the position reports map.
    #[must_use]
    pub fn position_reports(&self) -> IndexMap<InstrumentId, Vec<PositionStatusReport>> {
        self.position_reports.clone()
    }

    /// Add order reports to the mass status.
    pub fn add_order_reports(&mut self, reports: Vec<OrderStatusReport>) {
        for report in reports {
            self.order_reports.insert(report.venue_order_id, report);
        }
    }

    /// Add fill reports to the mass status.
    pub fn add_fill_reports(&mut self, reports: Vec<FillReport>) {
        for report in reports {
            self.fill_reports
                .entry(report.venue_order_id)
                .or_default()
                .push(report);
        }
    }

    /// Add position reports to the mass status.
    pub fn add_position_reports(&mut self, reports: Vec<PositionStatusReport>) {
        for report in reports {
            self.position_reports
                .entry(report.instrument_id)
                .or_default()
                .push(report);
        }
    }
}

impl std::fmt::Display for ExecutionMassStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ExecutionMassStatus(client_id={}, account_id={}, venue={}, order_reports={:?}, fill_reports={:?}, position_reports={:?}, report_id={}, ts_init={})",
            self.client_id,
            self.account_id,
            self.venue,
            self.order_reports,
            self.fill_reports,
            self.position_reports,
            self.report_id,
            self.ts_init,
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
        enums::{
            LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSideSpecified, TimeInForce,
        },
        identifiers::{
            AccountId, ClientId, InstrumentId, PositionId, TradeId, Venue, VenueOrderId,
        },
        reports::{fill::FillReport, order::OrderStatusReport, position::PositionStatusReport},
        types::{Currency, Money, Price, Quantity},
    };

    fn test_execution_mass_status() -> ExecutionMassStatus {
        ExecutionMassStatus::new(
            ClientId::from("IB"),
            AccountId::from("IB-DU123456"),
            Venue::from("NASDAQ"),
            UnixNanos::from(1_000_000_000),
            None,
        )
    }

    fn create_test_order_report() -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("IB-DU123456"),
            InstrumentId::from("AAPL.NASDAQ"),
            None,
            VenueOrderId::from("1"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("100"),
            Quantity::from("0"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        )
    }

    fn create_test_fill_report() -> FillReport {
        FillReport::new(
            AccountId::from("IB-DU123456"),
            InstrumentId::from("AAPL.NASDAQ"),
            VenueOrderId::from("1"),
            TradeId::from("T-001"),
            OrderSide::Buy,
            Quantity::from("50"),
            Price::from("150.00"),
            Money::new(1.0, Currency::USD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1_500_000_000),
            UnixNanos::from(2_500_000_000),
            None,
        )
    }

    fn create_test_position_report() -> PositionStatusReport {
        PositionStatusReport::new(
            AccountId::from("IB-DU123456"),
            InstrumentId::from("AAPL.NASDAQ"),
            PositionSideSpecified::Long,
            Quantity::from("50"),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,                            // report_id
            Some(PositionId::from("P-001")), // venue_position_id
            None,                            // avg_px_open
        )
    }

    #[rstest]
    fn test_execution_mass_status_new() {
        let mass_status = test_execution_mass_status();

        assert_eq!(mass_status.client_id, ClientId::from("IB"));
        assert_eq!(mass_status.account_id, AccountId::from("IB-DU123456"));
        assert_eq!(mass_status.venue, Venue::from("NASDAQ"));
        assert_eq!(mass_status.ts_init, UnixNanos::from(1_000_000_000));
        assert!(mass_status.order_reports().is_empty());
        assert!(mass_status.fill_reports().is_empty());
        assert!(mass_status.position_reports().is_empty());
    }

    #[rstest]
    fn test_execution_mass_status_with_generated_report_id() {
        let mass_status = ExecutionMassStatus::new(
            ClientId::from("IB"),
            AccountId::from("IB-DU123456"),
            Venue::from("NASDAQ"),
            UnixNanos::from(1_000_000_000),
            None, // No report ID provided, should generate one
        );

        // Should have a generated UUID
        assert_ne!(
            mass_status.report_id.to_string(),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[rstest]
    fn test_add_order_reports() {
        let mut mass_status = test_execution_mass_status();
        let order_report1 = create_test_order_report();
        let order_report2 = OrderStatusReport::new(
            AccountId::from("IB-DU123456"),
            InstrumentId::from("MSFT.NASDAQ"),
            None,
            VenueOrderId::from("2"),
            OrderSide::Sell,
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Filled,
            Quantity::from("200"),
            Quantity::from("200"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        );

        mass_status.add_order_reports(vec![order_report1.clone(), order_report2.clone()]);

        let order_reports = mass_status.order_reports();
        assert_eq!(order_reports.len(), 2);
        assert_eq!(
            order_reports.get(&VenueOrderId::from("1")),
            Some(&order_report1)
        );
        assert_eq!(
            order_reports.get(&VenueOrderId::from("2")),
            Some(&order_report2)
        );
    }

    #[rstest]
    fn test_add_fill_reports() {
        let mut mass_status = test_execution_mass_status();
        let fill_report1 = create_test_fill_report();
        let fill_report2 = FillReport::new(
            AccountId::from("IB-DU123456"),
            InstrumentId::from("AAPL.NASDAQ"),
            VenueOrderId::from("1"), // Same venue order ID
            TradeId::from("T-002"),
            OrderSide::Buy,
            Quantity::from("50"),
            Price::from("151.00"),
            Money::new(1.5, Currency::USD()),
            LiquiditySide::Maker,
            None,
            None,
            UnixNanos::from(1_600_000_000),
            UnixNanos::from(2_600_000_000),
            None,
        );

        mass_status.add_fill_reports(vec![fill_report1.clone(), fill_report2.clone()]);

        let fill_reports = mass_status.fill_reports();
        assert_eq!(fill_reports.len(), 1); // One entry because same venue order ID

        let fills_for_order = fill_reports.get(&VenueOrderId::from("1")).unwrap();
        assert_eq!(fills_for_order.len(), 2);
        assert_eq!(fills_for_order[0], fill_report1);
        assert_eq!(fills_for_order[1], fill_report2);
    }

    #[rstest]
    fn test_add_position_reports() {
        let mut mass_status = test_execution_mass_status();
        let position_report1 = create_test_position_report();
        let position_report2 = PositionStatusReport::new(
            AccountId::from("IB-DU123456"),
            InstrumentId::from("AAPL.NASDAQ"), // Same instrument ID
            PositionSideSpecified::Short,
            Quantity::from("25"),
            UnixNanos::from(2_100_000_000),
            UnixNanos::from(3_100_000_000),
            None,
            None,
            None,
        );
        let position_report3 = PositionStatusReport::new(
            AccountId::from("IB-DU123456"),
            InstrumentId::from("MSFT.NASDAQ"), // Different instrument
            PositionSideSpecified::Long,
            Quantity::from("100"),
            UnixNanos::from(2_200_000_000),
            UnixNanos::from(3_200_000_000),
            None,
            None,
            None,
        );

        mass_status.add_position_reports(vec![
            position_report1.clone(),
            position_report2.clone(),
            position_report3.clone(),
        ]);

        let position_reports = mass_status.position_reports();
        assert_eq!(position_reports.len(), 2); // Two instruments

        // Check AAPL positions
        let aapl_positions = position_reports
            .get(&InstrumentId::from("AAPL.NASDAQ"))
            .unwrap();
        assert_eq!(aapl_positions.len(), 2);
        assert_eq!(aapl_positions[0], position_report1);
        assert_eq!(aapl_positions[1], position_report2);

        // Check MSFT positions
        let msft_positions = position_reports
            .get(&InstrumentId::from("MSFT.NASDAQ"))
            .unwrap();
        assert_eq!(msft_positions.len(), 1);
        assert_eq!(msft_positions[0], position_report3);
    }

    #[rstest]
    fn test_add_multiple_fills_for_different_orders() {
        let mut mass_status = test_execution_mass_status();
        let fill_report1 = create_test_fill_report(); // venue_order_id = "1"
        let fill_report2 = FillReport::new(
            AccountId::from("IB-DU123456"),
            InstrumentId::from("MSFT.NASDAQ"),
            VenueOrderId::from("2"), // Different venue order ID
            TradeId::from("T-003"),
            OrderSide::Sell,
            Quantity::from("75"),
            Price::from("300.00"),
            Money::new(2.0, Currency::USD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1_700_000_000),
            UnixNanos::from(2_700_000_000),
            None,
        );

        mass_status.add_fill_reports(vec![fill_report1.clone(), fill_report2.clone()]);

        let fill_reports = mass_status.fill_reports();
        assert_eq!(fill_reports.len(), 2); // Two different venue order IDs

        let fills_order_1 = fill_reports.get(&VenueOrderId::from("1")).unwrap();
        assert_eq!(fills_order_1.len(), 1);
        assert_eq!(fills_order_1[0], fill_report1);

        let fills_order_2 = fill_reports.get(&VenueOrderId::from("2")).unwrap();
        assert_eq!(fills_order_2.len(), 1);
        assert_eq!(fills_order_2[0], fill_report2);
    }

    #[rstest]
    fn test_comprehensive_mass_status() {
        let mut mass_status = test_execution_mass_status();

        // Add various reports
        let order_report = create_test_order_report();
        let fill_report = create_test_fill_report();
        let position_report = create_test_position_report();

        mass_status.add_order_reports(vec![order_report.clone()]);
        mass_status.add_fill_reports(vec![fill_report.clone()]);
        mass_status.add_position_reports(vec![position_report.clone()]);

        // Verify all reports are present
        assert_eq!(mass_status.order_reports().len(), 1);
        assert_eq!(mass_status.fill_reports().len(), 1);
        assert_eq!(mass_status.position_reports().len(), 1);

        // Verify specific content
        assert_eq!(
            mass_status.order_reports().get(&VenueOrderId::from("1")),
            Some(&order_report)
        );
        assert_eq!(
            mass_status
                .fill_reports()
                .get(&VenueOrderId::from("1"))
                .unwrap()[0],
            fill_report
        );
        assert_eq!(
            mass_status
                .position_reports()
                .get(&InstrumentId::from("AAPL.NASDAQ"))
                .unwrap()[0],
            position_report
        );
    }

    #[rstest]
    fn test_display() {
        let mass_status = test_execution_mass_status();
        let display_str = format!("{mass_status}");

        assert!(display_str.contains("ExecutionMassStatus"));
        assert!(display_str.contains("IB"));
        assert!(display_str.contains("IB-DU123456"));
        assert!(display_str.contains("NASDAQ"));
    }

    #[rstest]
    fn test_clone_and_equality() {
        let mass_status1 = test_execution_mass_status();
        let mass_status2 = mass_status1.clone();

        assert_eq!(mass_status1, mass_status2);
    }

    #[rstest]
    fn test_serialization_roundtrip() {
        let original = test_execution_mass_status();

        // Test JSON serialization
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ExecutionMassStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_empty_mass_status_accessors() {
        let mass_status = test_execution_mass_status();

        // All collections should be empty initially
        assert!(mass_status.order_reports().is_empty());
        assert!(mass_status.fill_reports().is_empty());
        assert!(mass_status.position_reports().is_empty());
    }

    #[rstest]
    fn test_add_empty_reports() {
        let mut mass_status = test_execution_mass_status();

        // Adding empty vectors should work without issues
        mass_status.add_order_reports(vec![]);
        mass_status.add_fill_reports(vec![]);
        mass_status.add_position_reports(vec![]);

        // Should still be empty
        assert!(mass_status.order_reports().is_empty());
        assert!(mass_status.fill_reports().is_empty());
        assert!(mass_status.position_reports().is_empty());
    }

    #[rstest]
    fn test_overwrite_order_reports() {
        let mut mass_status = test_execution_mass_status();
        let venue_order_id = VenueOrderId::from("1");

        // Add first order report
        let order_report1 = create_test_order_report();
        mass_status.add_order_reports(vec![order_report1.clone()]);

        // Add second order report with same venue order ID (should overwrite)
        let order_report2 = OrderStatusReport::new(
            AccountId::from("IB-DU123456"),
            InstrumentId::from("AAPL.NASDAQ"),
            None,
            venue_order_id,
            OrderSide::Sell, // Different side
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Filled,
            Quantity::from("200"),
            Quantity::from("200"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        );
        mass_status.add_order_reports(vec![order_report2.clone()]);

        // Should have only one report (the latest one)
        let order_reports = mass_status.order_reports();
        assert_eq!(order_reports.len(), 1);
        assert_eq!(order_reports.get(&venue_order_id), Some(&order_report2));
        assert_ne!(order_reports.get(&venue_order_id), Some(&order_report1));
    }
}
