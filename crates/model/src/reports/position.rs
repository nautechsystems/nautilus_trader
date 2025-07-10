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

use std::fmt::{Debug, Display};

use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    enums::PositionSide,
    identifiers::{AccountId, InstrumentId, PositionId},
    types::Quantity,
};

/// Represents a position status at a point in time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct PositionStatusReport {
    /// The account ID associated with the position.
    pub account_id: AccountId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The position side.
    pub position_side: PositionSide,
    /// The current open quantity.
    pub quantity: Quantity,
    /// The current signed quantity as a decimal (positive for position side `LONG`, negative for `SHORT`).
    pub signed_decimal_qty: Decimal,
    /// The unique identifier for the event.
    pub report_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the last event occurred.
    pub ts_last: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// The position ID (assigned by the venue).
    pub venue_position_id: Option<PositionId>,
}

impl PositionStatusReport {
    /// Creates a new [`PositionStatusReport`] instance with required fields.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        account_id: AccountId,
        instrument_id: InstrumentId,
        position_side: PositionSide,
        quantity: Quantity,
        venue_position_id: Option<PositionId>,
        ts_last: UnixNanos,
        ts_init: UnixNanos,
        report_id: Option<UUID4>,
    ) -> Self {
        // Calculate signed decimal quantity based on position side
        let signed_decimal_qty = match position_side {
            PositionSide::Long => quantity.as_decimal(),
            PositionSide::Short => -quantity.as_decimal(),
            PositionSide::Flat => Decimal::ZERO,
            PositionSide::NoPositionSide => Decimal::ZERO, // TODO: Consider disallowing this?
        };

        Self {
            account_id,
            instrument_id,
            position_side,
            quantity,
            signed_decimal_qty,
            report_id: report_id.unwrap_or_default(),
            ts_last,
            ts_init,
            venue_position_id,
        }
    }

    /// Checks if the position has a venue position ID.
    #[must_use]
    pub const fn has_venue_position_id(&self) -> bool {
        self.venue_position_id.is_some()
    }

    /// Checks if this is a flat position (quantity is zero).
    #[must_use]
    pub const fn is_flat(&self) -> bool {
        matches!(
            self.position_side,
            PositionSide::Flat | PositionSide::NoPositionSide
        )
    }

    /// Checks if this is a long position.
    #[must_use]
    pub fn is_long(&self) -> bool {
        self.position_side == PositionSide::Long
    }

    /// Checks if this is a short position.
    #[must_use]
    pub fn is_short(&self) -> bool {
        self.position_side == PositionSide::Short
    }
}

impl Display for PositionStatusReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PositionStatusReport(account={}, instrument={}, side={}, qty={}, venue_pos_id={:?}, ts_last={}, ts_init={})",
            self.account_id,
            self.instrument_id,
            self.position_side,
            self.signed_decimal_qty,
            self.venue_position_id,
            self.ts_last,
            self.ts_init
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
    use rust_decimal::Decimal;

    use super::*;
    use crate::{
        enums::PositionSide,
        identifiers::{AccountId, InstrumentId, PositionId},
        types::Quantity,
    };

    fn test_position_status_report_long() -> PositionStatusReport {
        PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::Long,
            Quantity::from("100"),
            Some(PositionId::from("P-001")),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        )
    }

    fn test_position_status_report_short() -> PositionStatusReport {
        PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::Short,
            Quantity::from("50"),
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        )
    }

    fn test_position_status_report_flat() -> PositionStatusReport {
        PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::Flat,
            Quantity::from("0"),
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        )
    }

    #[rstest]
    fn test_position_status_report_new_long() {
        let report = test_position_status_report_long();

        assert_eq!(report.account_id, AccountId::from("SIM-001"));
        assert_eq!(report.instrument_id, InstrumentId::from("AUDUSD.SIM"));
        assert_eq!(report.position_side, PositionSide::Long);
        assert_eq!(report.quantity, Quantity::from("100"));
        assert_eq!(report.signed_decimal_qty, Decimal::from(100));
        assert_eq!(report.venue_position_id, Some(PositionId::from("P-001")));
        assert_eq!(report.ts_last, UnixNanos::from(1_000_000_000));
        assert_eq!(report.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_position_status_report_new_short() {
        let report = test_position_status_report_short();

        assert_eq!(report.position_side, PositionSide::Short);
        assert_eq!(report.quantity, Quantity::from("50"));
        assert_eq!(report.signed_decimal_qty, Decimal::from(-50));
        assert_eq!(report.venue_position_id, None);
    }

    #[rstest]
    fn test_position_status_report_new_flat() {
        let report = test_position_status_report_flat();

        assert_eq!(report.position_side, PositionSide::Flat);
        assert_eq!(report.quantity, Quantity::from("0"));
        assert_eq!(report.signed_decimal_qty, Decimal::ZERO);
    }

    #[rstest]
    fn test_position_status_report_no_position_side() {
        let report = PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::NoPositionSide,
            Quantity::from("0"),
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        );

        assert_eq!(report.position_side, PositionSide::NoPositionSide);
        assert_eq!(report.signed_decimal_qty, Decimal::ZERO);
    }

    #[rstest]
    fn test_position_status_report_with_generated_report_id() {
        let report = PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::Long,
            Quantity::from("100"),
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
    fn test_has_venue_position_id() {
        let mut report = test_position_status_report_long();
        assert!(report.has_venue_position_id());

        report.venue_position_id = None;
        assert!(!report.has_venue_position_id());
    }

    #[rstest]
    fn test_is_flat() {
        let long_report = test_position_status_report_long();
        let short_report = test_position_status_report_short();
        let flat_report = test_position_status_report_flat();

        let no_position_report = PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::NoPositionSide,
            Quantity::from("0"),
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        );

        assert!(!long_report.is_flat());
        assert!(!short_report.is_flat());
        assert!(flat_report.is_flat());
        assert!(no_position_report.is_flat());
    }

    #[rstest]
    fn test_is_long() {
        let long_report = test_position_status_report_long();
        let short_report = test_position_status_report_short();
        let flat_report = test_position_status_report_flat();

        assert!(long_report.is_long());
        assert!(!short_report.is_long());
        assert!(!flat_report.is_long());
    }

    #[rstest]
    fn test_is_short() {
        let long_report = test_position_status_report_long();
        let short_report = test_position_status_report_short();
        let flat_report = test_position_status_report_flat();

        assert!(!long_report.is_short());
        assert!(short_report.is_short());
        assert!(!flat_report.is_short());
    }

    #[rstest]
    fn test_display() {
        let report = test_position_status_report_long();
        let display_str = format!("{report}");

        assert!(display_str.contains("PositionStatusReport"));
        assert!(display_str.contains("SIM-001"));
        assert!(display_str.contains("AUDUSD.SIM"));
        assert!(display_str.contains("LONG"));
        assert!(display_str.contains("100"));
        assert!(display_str.contains("P-001"));
    }

    #[rstest]
    fn test_clone_and_equality() {
        let report1 = test_position_status_report_long();
        let report2 = report1.clone();

        assert_eq!(report1, report2);
    }

    #[rstest]
    fn test_serialization_roundtrip() {
        let original = test_position_status_report_long();

        // Test JSON serialization
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PositionStatusReport = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_signed_decimal_qty_calculation() {
        // Test with various quantities to ensure signed decimal calculation is correct
        let long_100 = PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::Long,
            Quantity::from("100.5"),
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        );

        let short_200 = PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::Short,
            Quantity::from("200.75"),
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        );

        assert_eq!(
            long_100.signed_decimal_qty,
            Decimal::from_f64_retain(100.5).unwrap()
        );
        assert_eq!(
            short_200.signed_decimal_qty,
            Decimal::from_f64_retain(-200.75).unwrap()
        );
    }

    #[rstest]
    fn test_different_position_sides_not_equal() {
        let long_report = test_position_status_report_long();
        let short_report = PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::Short,
            Quantity::from("100"), // Same quantity but different side
            Some(PositionId::from("P-001")),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
        );

        assert_ne!(long_report, short_report);
        assert_ne!(
            long_report.signed_decimal_qty,
            short_report.signed_decimal_qty
        );
    }
}
