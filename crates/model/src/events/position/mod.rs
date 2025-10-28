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

use crate::{
    events::{PositionChanged, PositionClosed, PositionOpened},
    identifiers::{AccountId, InstrumentId},
};
pub mod changed;
pub mod closed;
pub mod opened;
pub mod snapshot;

#[derive(Debug)]
pub enum PositionEvent {
    PositionOpened(PositionOpened),
    PositionChanged(PositionChanged),
    PositionClosed(PositionClosed),
}

impl PositionEvent {
    pub fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::PositionOpened(position) => position.instrument_id,
            Self::PositionChanged(position) => position.instrument_id,
            Self::PositionClosed(position) => position.instrument_id,
        }
    }

    pub fn account_id(&self) -> AccountId {
        match self {
            Self::PositionOpened(position) => position.account_id,
            Self::PositionChanged(position) => position.account_id,
            Self::PositionClosed(position) => position.account_id,
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
        enums::{OrderSide, PositionSide},
        events::{PositionChanged, PositionClosed, PositionOpened},
        identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
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

    #[rstest]
    fn test_position_event_opened_instrument_id() {
        let opened = create_test_position_opened();
        let event = PositionEvent::PositionOpened(opened);

        assert_eq!(event.instrument_id(), InstrumentId::from("EURUSD.SIM"));
    }

    #[rstest]
    fn test_position_event_changed_instrument_id() {
        let changed = create_test_position_changed();
        let event = PositionEvent::PositionChanged(changed);

        assert_eq!(event.instrument_id(), InstrumentId::from("EURUSD.SIM"));
    }

    #[rstest]
    fn test_position_event_closed_instrument_id() {
        let closed = create_test_position_closed();
        let event = PositionEvent::PositionClosed(closed);

        assert_eq!(event.instrument_id(), InstrumentId::from("EURUSD.SIM"));
    }

    #[rstest]
    fn test_position_event_opened_account_id() {
        let opened = create_test_position_opened();
        let event = PositionEvent::PositionOpened(opened);

        assert_eq!(event.account_id(), AccountId::from("SIM-001"));
    }

    #[rstest]
    fn test_position_event_changed_account_id() {
        let changed = create_test_position_changed();
        let event = PositionEvent::PositionChanged(changed);

        assert_eq!(event.account_id(), AccountId::from("SIM-001"));
    }

    #[rstest]
    fn test_position_event_closed_account_id() {
        let closed = create_test_position_closed();
        let event = PositionEvent::PositionClosed(closed);

        assert_eq!(event.account_id(), AccountId::from("SIM-001"));
    }

    #[rstest]
    fn test_position_event_debug_formatting() {
        let opened = create_test_position_opened();
        let event = PositionEvent::PositionOpened(opened);

        let debug_str = format!("{event:?}");
        assert!(debug_str.contains("PositionOpened"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("SIM-001"));
    }

    #[rstest]
    fn test_position_event_enum_variants() {
        let opened = create_test_position_opened();
        let changed = create_test_position_changed();
        let closed = create_test_position_closed();

        let event_opened = PositionEvent::PositionOpened(opened);
        let event_changed = PositionEvent::PositionChanged(changed);
        let event_closed = PositionEvent::PositionClosed(closed);

        match event_opened {
            PositionEvent::PositionOpened(_) => {}
            _ => panic!("Expected PositionOpened variant"),
        }

        match event_changed {
            PositionEvent::PositionChanged(_) => {}
            _ => panic!("Expected PositionChanged variant"),
        }

        match event_closed {
            PositionEvent::PositionClosed(_) => {}
            _ => panic!("Expected PositionClosed variant"),
        }
    }
}
