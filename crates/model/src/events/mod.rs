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

//! Events for the trading domain model.

pub mod account;
pub mod funding;
pub mod order;
pub mod portfolio;
pub mod position;

use nautilus_core::UnixNanos;

use crate::data::HasTsInit;
// Re-exports
pub use crate::events::{
    account::state::AccountState,
    funding::settlement::FundingSettlement,
    order::{
        OrderEvent, OrderEventType,
        accepted::OrderAccepted,
        accepted_batch::OrderAcceptedBatch,
        any::OrderEventAny,
        cancel_rejected::OrderCancelRejected,
        canceled::OrderCanceled,
        canceled_batch::OrderCanceledBatch,
        denied::OrderDenied,
        denied_reason::{OrderDeniedCode, OrderDeniedReason, OrderPriceField},
        emulated::OrderEmulated,
        expired::OrderExpired,
        fill_voided::OrderFillVoided,
        filled::OrderFilled,
        initialized::OrderInitialized,
        modify_rejected::OrderModifyRejected,
        pending_cancel::OrderPendingCancel,
        pending_update::OrderPendingUpdate,
        rejected::OrderRejected,
        released::OrderReleased,
        snapshot::OrderSnapshot,
        submitted::OrderSubmitted,
        submitted_batch::OrderSubmittedBatch,
        triggered::OrderTriggered,
        updated::OrderUpdated,
    },
    portfolio::snapshot::PortfolioSnapshot,
    position::{
        PositionEvent, adjusted::PositionAdjusted, changed::PositionChanged,
        closed::PositionClosed, opened::PositionOpened, snapshot::PositionSnapshot,
    },
};

impl HasTsInit for AccountState {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for FundingSettlement {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderInitialized {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderDenied {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderEmulated {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderSubmitted {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderAccepted {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderRejected {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderPendingCancel {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderCanceled {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderCancelRejected {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderExpired {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderTriggered {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderPendingUpdate {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderReleased {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderModifyRejected {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderUpdated {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderFilled {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderFillVoided {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderSnapshot {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionOpened {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionChanged {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionClosed {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionAdjusted {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionSnapshot {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PortfolioSnapshot {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UUID4;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        enums::{AccountType, OrderSide, OrderType, PositionAdjustmentType},
        events::order::spec::{
            OrderAcceptedSpec, OrderCancelRejectedSpec, OrderCanceledSpec, OrderDeniedSpec,
            OrderEmulatedSpec, OrderExpiredSpec, OrderFillVoidedSpec, OrderFilledSpec,
            OrderInitializedSpec, OrderModifyRejectedSpec, OrderPendingCancelSpec,
            OrderPendingUpdateSpec, OrderRejectedSpec, OrderReleasedSpec, OrderSubmittedSpec,
            OrderTriggeredSpec, OrderUpdatedSpec,
        },
        identifiers::{AccountId, InstrumentId, PositionId, StrategyId, TraderId},
        orders::builder::OrderTestBuilder,
        position::Position,
        stubs::stub_position_long,
        types::{Currency, Money, Price, Quantity},
    };

    #[rstest]
    fn test_order_lifecycle_event_ts_init() {
        let ts_init = UnixNanos::from(1);

        assert_eq!(
            HasTsInit::ts_init(&OrderInitializedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderDeniedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderEmulatedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderReleasedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderSubmittedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderAcceptedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderRejectedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderTriggeredSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderExpiredSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderCanceledSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
    }

    #[rstest]
    fn test_order_amendment_event_ts_init() {
        let ts_init = UnixNanos::from(2);

        assert_eq!(
            HasTsInit::ts_init(&OrderPendingUpdateSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderPendingCancelSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderModifyRejectedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderCancelRejectedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderUpdatedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
    }

    #[rstest]
    fn test_order_fill_event_ts_init() {
        let ts_init = UnixNanos::from(3);

        assert_eq!(
            HasTsInit::ts_init(&OrderFilledSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&OrderFillVoidedSpec::builder().ts_init(ts_init).build()),
            ts_init
        );
    }

    #[rstest]
    fn test_order_snapshot_ts_init() {
        let ts_init = UnixNanos::from(4);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::from("AUD/USD.SIM"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from(1))
            .build();
        let mut snapshot = OrderSnapshot::from(order);
        snapshot.ts_init = ts_init;

        assert_eq!(HasTsInit::ts_init(&snapshot), ts_init);
    }

    #[rstest]
    fn test_position_event_ts_init(mut stub_position_long: Position) {
        let ts_init = UnixNanos::from(5);
        let event_id = UUID4::default();
        let fill = stub_position_long.last_event().unwrap();

        assert_eq!(
            HasTsInit::ts_init(&PositionOpened::create(
                &stub_position_long,
                &fill,
                event_id,
                ts_init
            )),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&PositionChanged::create(
                &stub_position_long,
                &fill,
                event_id,
                ts_init
            )),
            ts_init
        );
        assert_eq!(
            HasTsInit::ts_init(&PositionClosed::create(
                &stub_position_long,
                &fill,
                event_id,
                ts_init
            )),
            ts_init
        );

        stub_position_long.ts_init = ts_init;

        assert_eq!(
            HasTsInit::ts_init(&PositionSnapshot::from(&stub_position_long, None)),
            ts_init
        );
    }

    #[rstest]
    fn test_position_adjusted_ts_init() {
        let ts_init = UnixNanos::from(6);

        let event = PositionAdjusted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("AUD/USD.SIM"),
            PositionId::from("P-001"),
            AccountId::from("SIM-001"),
            PositionAdjustmentType::Funding,
            None,
            Some(Money::new(1.0, Currency::USD())),
            None,
            UUID4::default(),
            UnixNanos::from(5),
            ts_init,
        );

        assert_eq!(HasTsInit::ts_init(&event), ts_init);
    }

    #[rstest]
    fn test_account_state_ts_init() {
        let ts_init = UnixNanos::from(7);

        let event = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::default(),
            UnixNanos::from(6),
            ts_init,
            Some(Currency::USD()),
        );

        assert_eq!(HasTsInit::ts_init(&event), ts_init);
    }

    #[rstest]
    fn test_portfolio_snapshot_ts_init() {
        let ts_init = UnixNanos::from(8);

        let event = PortfolioSnapshot::new(
            AccountId::from("SIM-001"),
            AccountType::Cash,
            Some(Currency::USD()),
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            None,
            false,
            vec![],
            vec![],
            vec![],
            UUID4::default(),
            UnixNanos::from(7),
            ts_init,
        );

        assert_eq!(HasTsInit::ts_init(&event), ts_init);
    }

    #[rstest]
    fn test_funding_settlement_ts_init() {
        let ts_init = UnixNanos::from(9);

        let event = FundingSettlement::new(
            TraderId::from("TRADER-001"),
            InstrumentId::from("AUD/USD.SIM"),
            AccountId::from("SIM-001"),
            dec!(0.0001),
            Price::from("1.00000"),
            Currency::USD(),
            UUID4::default(),
            UnixNanos::from(8),
            ts_init,
        );

        assert_eq!(HasTsInit::ts_init(&event), ts_init);
    }
}
