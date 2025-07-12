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

use derive_builder::Builder;
use nautilus_core::{UUID4, UnixNanos, serialization::from_bool_as_u8};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderType, TimeInForce, TrailingOffsetType,
        TriggerType,
    },
    events::OrderEvent,
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{Currency, Money, Price, Quantity},
};

/// Represents an event where an order has been accepted by the trading venue.
///
/// This event often corresponds to a `NEW` OrdStatus <39> field in FIX execution reports.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Builder)]
#[builder(default)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderAccepted {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    /// The venue order ID associated with the event.
    pub venue_order_id: VenueOrderId,
    /// The account ID associated with the event.
    pub account_id: AccountId,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// If the event was generated during reconciliation.
    #[serde(deserialize_with = "from_bool_as_u8")]
    pub reconciliation: u8, // TODO: Change to bool once Cython removed
}

impl OrderAccepted {
    /// Creates a new [`OrderAccepted`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reconciliation: bool,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation: u8::from(reconciliation),
        }
    }
}

impl Debug for OrderAccepted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, strategy_id={}, instrument_id={}, client_order_id={}, venue_order_id={}, account_id={}, event_id={}, ts_event={}, ts_init={})",
            stringify!(OrderAccepted),
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.account_id,
            self.event_id,
            self.ts_event,
            self.ts_init
        )
    }
}

impl Display for OrderAccepted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={}, client_order_id={}, venue_order_id={}, account_id={}, ts_event={})",
            stringify!(OrderAccepted),
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.account_id,
            self.ts_event
        )
    }
}

impl OrderEvent for OrderAccepted {
    fn id(&self) -> UUID4 {
        self.event_id
    }

    fn kind(&self) -> &str {
        stringify!(OrderAccepted)
    }

    fn order_type(&self) -> Option<OrderType> {
        None
    }

    fn order_side(&self) -> Option<OrderSide> {
        None
    }

    fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    fn strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    fn trade_id(&self) -> Option<TradeId> {
        None
    }

    fn currency(&self) -> Option<Currency> {
        None
    }

    fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    fn reason(&self) -> Option<Ustr> {
        None
    }

    fn quantity(&self) -> Option<Quantity> {
        None
    }

    fn time_in_force(&self) -> Option<TimeInForce> {
        None
    }

    fn liquidity_side(&self) -> Option<LiquiditySide> {
        None
    }

    fn post_only(&self) -> Option<bool> {
        None
    }

    fn reduce_only(&self) -> Option<bool> {
        None
    }

    fn quote_quantity(&self) -> Option<bool> {
        None
    }

    fn reconciliation(&self) -> bool {
        false
    }

    fn price(&self) -> Option<Price> {
        None
    }

    fn last_px(&self) -> Option<Price> {
        None
    }

    fn last_qty(&self) -> Option<Quantity> {
        None
    }

    fn trigger_price(&self) -> Option<Price> {
        None
    }

    fn trigger_type(&self) -> Option<TriggerType> {
        None
    }

    fn limit_offset(&self) -> Option<Decimal> {
        None
    }

    fn trailing_offset(&self) -> Option<Decimal> {
        None
    }

    fn trailing_offset_type(&self) -> Option<TrailingOffsetType> {
        None
    }

    fn expire_time(&self) -> Option<UnixNanos> {
        None
    }

    fn display_qty(&self) -> Option<Quantity> {
        None
    }

    fn emulation_trigger(&self) -> Option<TriggerType> {
        None
    }

    fn trigger_instrument_id(&self) -> Option<InstrumentId> {
        None
    }

    fn contingency_type(&self) -> Option<ContingencyType> {
        None
    }

    fn order_list_id(&self) -> Option<OrderListId> {
        None
    }

    fn linked_order_ids(&self) -> Option<Vec<ClientOrderId>> {
        None
    }

    fn parent_order_id(&self) -> Option<ClientOrderId> {
        None
    }

    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        None
    }

    fn exec_spawn_id(&self) -> Option<ClientOrderId> {
        None
    }

    fn venue_order_id(&self) -> Option<VenueOrderId> {
        Some(self.venue_order_id)
    }

    fn account_id(&self) -> Option<AccountId> {
        Some(self.account_id)
    }

    fn position_id(&self) -> Option<PositionId> {
        None
    }

    fn commission(&self) -> Option<Money> {
        None
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::{
        events::order::stubs::*,
        identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    };

    fn create_test_order_accepted() -> OrderAccepted {
        OrderAccepted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("EURUSD.SIM"),
            ClientOrderId::from("O-19700101-000000-001-001-1"),
            VenueOrderId::from("V-001"),
            AccountId::from("SIM-001"),
            Default::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            false,
        )
    }

    #[rstest]
    fn test_order_accepted_new() {
        let order_accepted = create_test_order_accepted();

        assert_eq!(order_accepted.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(order_accepted.strategy_id, StrategyId::from("EMA-CROSS"));
        assert_eq!(
            order_accepted.instrument_id,
            InstrumentId::from("EURUSD.SIM")
        );
        assert_eq!(
            order_accepted.client_order_id,
            ClientOrderId::from("O-19700101-000000-001-001-1")
        );
        assert_eq!(order_accepted.venue_order_id, VenueOrderId::from("V-001"));
        assert_eq!(order_accepted.account_id, AccountId::from("SIM-001"));
        assert_eq!(order_accepted.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(order_accepted.ts_init, UnixNanos::from(2_000_000_000));
        assert_eq!(order_accepted.reconciliation, 0);
    }

    #[rstest]
    fn test_order_accepted_new_with_reconciliation() {
        let order_accepted = OrderAccepted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("EURUSD.SIM"),
            ClientOrderId::from("O-19700101-000000-001-001-1"),
            VenueOrderId::from("V-001"),
            AccountId::from("SIM-001"),
            Default::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            true,
        );

        assert_eq!(order_accepted.reconciliation, 1);
    }

    #[rstest]
    fn test_order_accepted_display(order_accepted: OrderAccepted) {
        let display = format!("{order_accepted}");
        assert_eq!(
            display,
            "OrderAccepted(instrument_id=BTCUSDT.COINBASE, client_order_id=O-19700101-000000-001-001-1, venue_order_id=001, account_id=SIM-001, ts_event=0)"
        );
    }

    #[rstest]
    fn test_order_accepted_default() {
        let order_accepted = OrderAccepted::default();

        assert_eq!(order_accepted.trader_id, TraderId::default());
        assert_eq!(order_accepted.strategy_id, StrategyId::default());
        assert_eq!(order_accepted.instrument_id, InstrumentId::default());
        assert_eq!(order_accepted.client_order_id, ClientOrderId::default());
        assert_eq!(order_accepted.venue_order_id, VenueOrderId::default());
        assert_eq!(order_accepted.account_id, AccountId::default());
        assert_eq!(order_accepted.reconciliation, 0);
    }

    #[rstest]
    fn test_order_accepted_order_event_trait() {
        let order_accepted = create_test_order_accepted();

        assert_eq!(order_accepted.id(), order_accepted.event_id);
        assert_eq!(order_accepted.kind(), "OrderAccepted");
        assert_eq!(order_accepted.order_type(), None);
        assert_eq!(order_accepted.order_side(), None);
        assert_eq!(order_accepted.trader_id(), TraderId::from("TRADER-001"));
        assert_eq!(order_accepted.strategy_id(), StrategyId::from("EMA-CROSS"));
        assert_eq!(
            order_accepted.instrument_id(),
            InstrumentId::from("EURUSD.SIM")
        );
        assert_eq!(order_accepted.trade_id(), None);
        assert_eq!(order_accepted.currency(), None);
        assert_eq!(
            order_accepted.client_order_id(),
            ClientOrderId::from("O-19700101-000000-001-001-1")
        );
        assert_eq!(order_accepted.reason(), None);
        assert_eq!(order_accepted.quantity(), None);
        assert_eq!(order_accepted.time_in_force(), None);
        assert_eq!(order_accepted.liquidity_side(), None);
        assert_eq!(order_accepted.post_only(), None);
        assert_eq!(order_accepted.reduce_only(), None);
        assert_eq!(order_accepted.quote_quantity(), None);
        assert!(!order_accepted.reconciliation());
        assert_eq!(
            order_accepted.venue_order_id(),
            Some(VenueOrderId::from("V-001"))
        );
        assert_eq!(
            order_accepted.account_id(),
            Some(AccountId::from("SIM-001"))
        );
        assert_eq!(order_accepted.position_id(), None);
        assert_eq!(order_accepted.commission(), None);
        assert_eq!(order_accepted.ts_event(), UnixNanos::from(1_000_000_000));
        assert_eq!(order_accepted.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_accepted_clone() {
        let order_accepted1 = create_test_order_accepted();
        let order_accepted2 = order_accepted1;

        assert_eq!(order_accepted1, order_accepted2);
    }

    #[rstest]
    fn test_order_accepted_debug() {
        let order_accepted = create_test_order_accepted();
        let debug_str = format!("{order_accepted:?}");

        assert!(debug_str.contains("OrderAccepted"));
        assert!(debug_str.contains("TRADER-001"));
        assert!(debug_str.contains("EMA-CROSS"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("O-19700101-000000-001-001-1"));
    }

    #[rstest]
    fn test_order_accepted_partial_eq() {
        let order_accepted1 = create_test_order_accepted();
        let mut order_accepted2 = create_test_order_accepted();
        order_accepted2.event_id = order_accepted1.event_id; // Make event_ids equal
        let mut order_accepted3 = create_test_order_accepted();
        order_accepted3.venue_order_id = VenueOrderId::from("V-002");

        assert_eq!(order_accepted1, order_accepted2);
        assert_ne!(order_accepted1, order_accepted3);
    }

    #[rstest]
    fn test_order_accepted_timestamps() {
        let order_accepted = create_test_order_accepted();

        assert_eq!(order_accepted.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(order_accepted.ts_init, UnixNanos::from(2_000_000_000));
        assert!(order_accepted.ts_event < order_accepted.ts_init);
    }

    #[rstest]
    fn test_order_accepted_serialization() {
        let original = create_test_order_accepted();

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: OrderAccepted = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_order_accepted_different_venues() {
        let mut venue1 = create_test_order_accepted();
        venue1.venue_order_id = VenueOrderId::from("COINBASE-001");

        let mut venue2 = create_test_order_accepted();
        venue2.venue_order_id = VenueOrderId::from("BINANCE-001");

        assert_ne!(venue1, venue2);
        assert_eq!(venue1.venue_order_id, VenueOrderId::from("COINBASE-001"));
        assert_eq!(venue2.venue_order_id, VenueOrderId::from("BINANCE-001"));
    }

    #[rstest]
    fn test_order_accepted_different_accounts() {
        let mut account1 = create_test_order_accepted();
        account1.account_id = AccountId::from("LIVE-001");

        let mut account2 = create_test_order_accepted();
        account2.account_id = AccountId::from("SIM-001");

        assert_ne!(account1, account2);
        assert_eq!(account1.account_id, AccountId::from("LIVE-001"));
        assert_eq!(account2.account_id, AccountId::from("SIM-001"));
    }
}
