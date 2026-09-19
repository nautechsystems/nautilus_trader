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

//! Status report types for trading operations.
//!
//! This module provides report types for tracking and communicating the status
//! of various trading operations, including order fills, order status, position
//! status, and mass status requests.

pub mod fill;
pub mod mass_status;
pub mod order;
pub mod position;

// Re-exports
pub use fill::FillReport;
pub use mass_status::ExecutionMassStatus;
use nautilus_core::UnixNanos;
pub use order::OrderStatusReport;
pub use position::PositionStatusReport;

use crate::data::HasTsInit;

impl HasTsInit for FillReport {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderStatusReport {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionStatusReport {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for ExecutionMassStatus {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce},
        identifiers::{AccountId, ClientId, InstrumentId, TradeId, Venue, VenueOrderId},
        types::{Currency, Money, Price, Quantity},
    };

    #[rstest]
    fn test_fill_report_ts_init() {
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
            None,
        );

        assert_eq!(report.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_status_report_ts_init() {
        let report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            None,
            VenueOrderId::from("1"),
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("100"),
            Quantity::from("0"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        );

        assert_eq!(report.ts_init(), UnixNanos::from(3_000_000_000));
    }

    #[rstest]
    fn test_position_status_report_ts_init() {
        let report = PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::Long,
            Quantity::from("100"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
            None,
            None,
        );

        assert_eq!(report.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_execution_mass_status_ts_init() {
        let report = ExecutionMassStatus::new(
            ClientId::from("IB"),
            AccountId::from("IB-DU123456"),
            Venue::from("NASDAQ"),
            UnixNanos::from(4_000_000_000),
            None,
        );

        assert_eq!(report.ts_init(), UnixNanos::from(4_000_000_000));
    }
}
