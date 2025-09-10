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

/// Represents an event where an order has been rejected by the trading venue.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Builder)]
#[builder(default)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderRejected {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    /// The account ID associated with the event.
    pub account_id: AccountId,
    /// The reason the order was rejected.
    pub reason: Ustr,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// If the event was generated during reconciliation.
    #[serde(deserialize_with = "from_bool_as_u8")]
    pub reconciliation: u8, // TODO: Change to bool once Cython removed
    /// If the order was rejected because it was post-only and would execute immediately as a taker.
    #[serde(default, deserialize_with = "from_bool_as_u8")]
    pub due_post_only: u8, // TODO: Change to bool once Cython removed
}

impl OrderRejected {
    /// Creates a new [`OrderRejected`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        account_id: AccountId,
        reason: Ustr,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reconciliation: bool,
        due_post_only: bool,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            reason,
            event_id,
            ts_event,
            ts_init,
            reconciliation: u8::from(reconciliation),
            due_post_only: u8::from(due_post_only),
        }
    }
}

impl Debug for OrderRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, strategy_id={}, instrument_id={}, client_order_id={}, account_id={}, reason='{}', event_id={}, ts_event={}, ts_init={})",
            stringify!(OrderRejected),
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.client_order_id,
            self.account_id,
            self.reason,
            self.event_id,
            self.ts_event,
            self.ts_init
        )
    }
}

impl Display for OrderRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={}, client_order_id={}, account_id={}, reason='{}', ts_event={})",
            stringify!(OrderRejected),
            self.instrument_id,
            self.client_order_id,
            self.account_id,
            self.reason,
            self.ts_event
        )
    }
}

impl OrderEvent for OrderRejected {
    fn id(&self) -> UUID4 {
        self.event_id
    }

    fn kind(&self) -> &str {
        stringify!(OrderRejected)
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
        Some(self.reason)
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
        None
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
    use ustr::Ustr;

    use super::*;
    use crate::{
        events::order::stubs::*,
        identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    };

    fn create_test_order_rejected() -> OrderRejected {
        OrderRejected::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("EURUSD.SIM"),
            ClientOrderId::from("O-19700101-000000-001-001-1"),
            AccountId::from("SIM-001"),
            Ustr::from("INSUFFICIENT_MARGIN"),
            Default::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            false,
            false,
        )
    }

    #[rstest]
    fn test_order_rejected_new() {
        let order_rejected = create_test_order_rejected();

        assert_eq!(order_rejected.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(order_rejected.strategy_id, StrategyId::from("EMA-CROSS"));
        assert_eq!(
            order_rejected.instrument_id,
            InstrumentId::from("EURUSD.SIM")
        );
        assert_eq!(
            order_rejected.client_order_id,
            ClientOrderId::from("O-19700101-000000-001-001-1")
        );
        assert_eq!(order_rejected.account_id, AccountId::from("SIM-001"));
        assert_eq!(order_rejected.reason, Ustr::from("INSUFFICIENT_MARGIN"));
        assert_eq!(order_rejected.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(order_rejected.ts_init, UnixNanos::from(2_000_000_000));
        assert_eq!(order_rejected.reconciliation, 0);
        assert_eq!(order_rejected.due_post_only, 0);
    }

    #[rstest]
    fn test_order_rejected_new_with_reconciliation() {
        let order_rejected = OrderRejected::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("EURUSD.SIM"),
            ClientOrderId::from("O-19700101-000000-001-001-1"),
            AccountId::from("SIM-001"),
            Ustr::from("INVALID_PRICE"),
            Default::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            true,
            false,
        );

        assert_eq!(order_rejected.reconciliation, 1);
    }

    #[rstest]
    fn test_order_rejected_clone() {
        let order_rejected1 = create_test_order_rejected();
        let order_rejected2 = order_rejected1;

        assert_eq!(order_rejected1, order_rejected2);
    }

    #[rstest]
    fn test_order_rejected_debug() {
        let order_rejected = create_test_order_rejected();
        let debug_str = format!("{order_rejected:?}");

        assert!(debug_str.contains("OrderRejected"));
        assert!(debug_str.contains("TRADER-001"));
        assert!(debug_str.contains("EMA-CROSS"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("O-19700101-000000-001-001-1"));
        assert!(debug_str.contains("INSUFFICIENT_MARGIN"));
    }

    #[rstest]
    fn test_order_rejected_display(order_rejected_insufficient_margin: OrderRejected) {
        let display = format!("{order_rejected_insufficient_margin}");
        assert_eq!(
            display,
            "OrderRejected(instrument_id=BTCUSDT.COINBASE, client_order_id=O-19700101-000000-001-001-1, \
        account_id=SIM-001, reason='INSUFFICIENT_MARGIN', ts_event=0)"
        );
    }

    #[rstest]
    fn test_order_rejected_partial_eq() {
        let order_rejected1 = create_test_order_rejected();
        let mut order_rejected2 = create_test_order_rejected();
        order_rejected2.event_id = order_rejected1.event_id; // Make event_ids equal
        let mut order_rejected3 = create_test_order_rejected();
        order_rejected3.reason = Ustr::from("INVALID_ORDER");

        assert_eq!(order_rejected1, order_rejected2);
        assert_ne!(order_rejected1, order_rejected3);
    }

    #[rstest]
    fn test_order_rejected_default() {
        let order_rejected = OrderRejected::default();

        assert_eq!(order_rejected.trader_id, TraderId::default());
        assert_eq!(order_rejected.strategy_id, StrategyId::default());
        assert_eq!(order_rejected.instrument_id, InstrumentId::default());
        assert_eq!(order_rejected.client_order_id, ClientOrderId::default());
        assert_eq!(order_rejected.account_id, AccountId::default());
        assert_eq!(order_rejected.reconciliation, 0);
        assert_eq!(order_rejected.due_post_only, 0);
    }

    #[rstest]
    fn test_order_rejected_order_event_trait() {
        let order_rejected = create_test_order_rejected();

        assert_eq!(order_rejected.id(), order_rejected.event_id);
        assert_eq!(order_rejected.kind(), "OrderRejected");
        assert_eq!(order_rejected.order_type(), None);
        assert_eq!(order_rejected.order_side(), None);
        assert_eq!(order_rejected.trader_id(), TraderId::from("TRADER-001"));
        assert_eq!(order_rejected.strategy_id(), StrategyId::from("EMA-CROSS"));
        assert_eq!(
            order_rejected.instrument_id(),
            InstrumentId::from("EURUSD.SIM")
        );
        assert_eq!(order_rejected.trade_id(), None);
        assert_eq!(order_rejected.currency(), None);
        assert_eq!(
            order_rejected.client_order_id(),
            ClientOrderId::from("O-19700101-000000-001-001-1")
        );
        assert_eq!(
            order_rejected.reason(),
            Some(Ustr::from("INSUFFICIENT_MARGIN"))
        );
        assert_eq!(order_rejected.quantity(), None);
        assert_eq!(order_rejected.time_in_force(), None);
        assert_eq!(order_rejected.liquidity_side(), None);
        assert_eq!(order_rejected.post_only(), None);
        assert_eq!(order_rejected.reduce_only(), None);
        assert_eq!(order_rejected.quote_quantity(), None);
        assert!(!order_rejected.reconciliation());
        assert_eq!(order_rejected.venue_order_id(), None);
        assert_eq!(
            order_rejected.account_id(),
            Some(AccountId::from("SIM-001"))
        );
        assert_eq!(order_rejected.position_id(), None);
        assert_eq!(order_rejected.commission(), None);
        assert_eq!(order_rejected.ts_event(), UnixNanos::from(1_000_000_000));
        assert_eq!(order_rejected.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_rejected_different_reasons() {
        let mut insufficient_margin = create_test_order_rejected();
        insufficient_margin.reason = Ustr::from("INSUFFICIENT_MARGIN");

        let mut invalid_price = create_test_order_rejected();
        invalid_price.reason = Ustr::from("INVALID_PRICE");

        let mut market_closed = create_test_order_rejected();
        market_closed.reason = Ustr::from("MARKET_CLOSED");

        assert_ne!(insufficient_margin, invalid_price);
        assert_ne!(invalid_price, market_closed);
        assert_eq!(
            insufficient_margin.reason,
            Ustr::from("INSUFFICIENT_MARGIN")
        );
        assert_eq!(invalid_price.reason, Ustr::from("INVALID_PRICE"));
        assert_eq!(market_closed.reason, Ustr::from("MARKET_CLOSED"));
    }

    #[rstest]
    fn test_order_rejected_timestamps() {
        let order_rejected = create_test_order_rejected();

        assert_eq!(order_rejected.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(order_rejected.ts_init, UnixNanos::from(2_000_000_000));
        assert!(order_rejected.ts_event < order_rejected.ts_init);
    }

    #[rstest]
    fn test_order_rejected_serialization() {
        let original = create_test_order_rejected();

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: OrderRejected = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_order_rejected_different_accounts() {
        let mut live_account = create_test_order_rejected();
        live_account.account_id = AccountId::from("LIVE-001");

        let mut sim_account = create_test_order_rejected();
        sim_account.account_id = AccountId::from("SIM-001");

        assert_ne!(live_account, sim_account);
        assert_eq!(live_account.account_id, AccountId::from("LIVE-001"));
        assert_eq!(sim_account.account_id, AccountId::from("SIM-001"));
    }

    #[rstest]
    fn test_order_rejected_different_instruments() {
        let mut btc_order = create_test_order_rejected();
        btc_order.instrument_id = InstrumentId::from("BTCUSD.COINBASE");

        let mut eth_order = create_test_order_rejected();
        eth_order.instrument_id = InstrumentId::from("ETHUSD.COINBASE");

        assert_ne!(btc_order, eth_order);
        assert_eq!(
            btc_order.instrument_id,
            InstrumentId::from("BTCUSD.COINBASE")
        );
        assert_eq!(
            eth_order.instrument_id,
            InstrumentId::from("ETHUSD.COINBASE")
        );
    }

    #[rstest]
    fn test_order_rejected_with_due_post_only() {
        let order_rejected = OrderRejected::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("EURUSD.SIM"),
            ClientOrderId::from("O-19700101-000000-001-001-1"),
            AccountId::from("SIM-001"),
            Ustr::from("POST_ONLY_WOULD_EXECUTE"),
            Default::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            false,
            true,
        );

        assert_eq!(order_rejected.due_post_only, 1);
        assert_eq!(order_rejected.reason, Ustr::from("POST_ONLY_WOULD_EXECUTE"));
    }
}
