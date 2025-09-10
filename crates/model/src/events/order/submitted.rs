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
use nautilus_core::{UUID4, UnixNanos};
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

/// Represents an event where an order has been submitted by the system to the
/// trading venue.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Builder)]
#[builder(default)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderSubmitted {
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
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

impl OrderSubmitted {
    /// Creates a new [`OrderSubmitted`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        account_id: AccountId,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
        }
    }
}

impl Debug for OrderSubmitted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, strategy_id={}, instrument_id={}, client_order_id={}, account_id={}, event_id={}, ts_event={}, ts_init={})",
            stringify!(OrderSubmitted),
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.client_order_id,
            self.account_id,
            self.event_id,
            self.ts_event,
            self.ts_init
        )
    }
}

impl Display for OrderSubmitted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={}, client_order_id={}, account_id={}, ts_event={})",
            stringify!(OrderSubmitted),
            self.instrument_id,
            self.client_order_id,
            self.account_id,
            self.ts_event
        )
    }
}

impl OrderEvent for OrderSubmitted {
    fn id(&self) -> UUID4 {
        self.event_id
    }

    fn kind(&self) -> &str {
        stringify!(OrderSubmitted)
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

    use super::*;
    use crate::{
        events::order::stubs::*,
        identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    };

    fn create_test_order_submitted() -> OrderSubmitted {
        OrderSubmitted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("EURUSD.SIM"),
            ClientOrderId::from("O-19700101-000000-001-001-1"),
            AccountId::from("SIM-001"),
            Default::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        )
    }

    #[rstest]
    fn test_order_submitted_new() {
        let order_submitted = create_test_order_submitted();

        assert_eq!(order_submitted.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(order_submitted.strategy_id, StrategyId::from("EMA-CROSS"));
        assert_eq!(
            order_submitted.instrument_id,
            InstrumentId::from("EURUSD.SIM")
        );
        assert_eq!(
            order_submitted.client_order_id,
            ClientOrderId::from("O-19700101-000000-001-001-1")
        );
        assert_eq!(order_submitted.account_id, AccountId::from("SIM-001"));
        assert_eq!(order_submitted.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(order_submitted.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_submitted_clone() {
        let order_submitted1 = create_test_order_submitted();
        let order_submitted2 = order_submitted1;

        assert_eq!(order_submitted1, order_submitted2);
    }

    #[rstest]
    fn test_order_submitted_debug() {
        let order_submitted = create_test_order_submitted();
        let debug_str = format!("{order_submitted:?}");

        assert!(debug_str.contains("OrderSubmitted"));
        assert!(debug_str.contains("TRADER-001"));
        assert!(debug_str.contains("EMA-CROSS"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("O-19700101-000000-001-001-1"));
    }

    #[rstest]
    fn test_order_rejected_display(order_submitted: OrderSubmitted) {
        let display = format!("{order_submitted}");
        assert_eq!(
            display,
            "OrderSubmitted(instrument_id=BTCUSDT.COINBASE, client_order_id=O-19700101-000000-001-001-1, account_id=SIM-001, ts_event=0)"
        );
    }

    #[rstest]
    fn test_order_submitted_partial_eq() {
        let order_submitted1 = create_test_order_submitted();
        let mut order_submitted2 = create_test_order_submitted();
        order_submitted2.event_id = order_submitted1.event_id; // Make event_ids equal
        let mut order_submitted3 = create_test_order_submitted();
        order_submitted3.client_order_id = ClientOrderId::from("O-19700101-000000-001-001-2");

        assert_eq!(order_submitted1, order_submitted2);
        assert_ne!(order_submitted1, order_submitted3);
    }

    #[rstest]
    fn test_order_submitted_default() {
        let order_submitted = OrderSubmitted::default();

        assert_eq!(order_submitted.trader_id, TraderId::default());
        assert_eq!(order_submitted.strategy_id, StrategyId::default());
        assert_eq!(order_submitted.instrument_id, InstrumentId::default());
        assert_eq!(order_submitted.client_order_id, ClientOrderId::default());
        assert_eq!(order_submitted.account_id, AccountId::default());
    }

    #[rstest]
    fn test_order_submitted_order_event_trait() {
        let order_submitted = create_test_order_submitted();

        assert_eq!(order_submitted.id(), order_submitted.event_id);
        assert_eq!(order_submitted.kind(), "OrderSubmitted");
        assert_eq!(order_submitted.order_type(), None);
        assert_eq!(order_submitted.order_side(), None);
        assert_eq!(order_submitted.trader_id(), TraderId::from("TRADER-001"));
        assert_eq!(order_submitted.strategy_id(), StrategyId::from("EMA-CROSS"));
        assert_eq!(
            order_submitted.instrument_id(),
            InstrumentId::from("EURUSD.SIM")
        );
        assert_eq!(order_submitted.trade_id(), None);
        assert_eq!(order_submitted.currency(), None);
        assert_eq!(
            order_submitted.client_order_id(),
            ClientOrderId::from("O-19700101-000000-001-001-1")
        );
        assert_eq!(order_submitted.reason(), None);
        assert_eq!(order_submitted.quantity(), None);
        assert_eq!(order_submitted.time_in_force(), None);
        assert_eq!(order_submitted.liquidity_side(), None);
        assert_eq!(order_submitted.post_only(), None);
        assert_eq!(order_submitted.reduce_only(), None);
        assert_eq!(order_submitted.quote_quantity(), None);
        assert!(!order_submitted.reconciliation());
        assert_eq!(order_submitted.venue_order_id(), None);
        assert_eq!(
            order_submitted.account_id(),
            Some(AccountId::from("SIM-001"))
        );
        assert_eq!(order_submitted.position_id(), None);
        assert_eq!(order_submitted.commission(), None);
        assert_eq!(order_submitted.ts_event(), UnixNanos::from(1_000_000_000));
        assert_eq!(order_submitted.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_submitted_timestamps() {
        let order_submitted = create_test_order_submitted();

        assert_eq!(order_submitted.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(order_submitted.ts_init, UnixNanos::from(2_000_000_000));
        assert!(order_submitted.ts_event < order_submitted.ts_init);
    }

    #[rstest]
    fn test_order_submitted_serialization() {
        let original = create_test_order_submitted();

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: OrderSubmitted = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_order_submitted_different_strategies() {
        let mut ema_strategy = create_test_order_submitted();
        ema_strategy.strategy_id = StrategyId::from("EMA-CROSS");

        let mut rsi_strategy = create_test_order_submitted();
        rsi_strategy.strategy_id = StrategyId::from("RSI-MEAN-REVERSION");

        assert_ne!(ema_strategy, rsi_strategy);
        assert_eq!(ema_strategy.strategy_id, StrategyId::from("EMA-CROSS"));
        assert_eq!(
            rsi_strategy.strategy_id,
            StrategyId::from("RSI-MEAN-REVERSION")
        );
    }

    #[rstest]
    fn test_order_submitted_different_traders() {
        let mut trader1 = create_test_order_submitted();
        trader1.trader_id = TraderId::from("TRADER-001");

        let mut trader2 = create_test_order_submitted();
        trader2.trader_id = TraderId::from("TRADER-002");

        assert_ne!(trader1, trader2);
        assert_eq!(trader1.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(trader2.trader_id, TraderId::from("TRADER-002"));
    }

    #[rstest]
    fn test_order_submitted_different_instruments() {
        let mut fx_order = create_test_order_submitted();
        fx_order.instrument_id = InstrumentId::from("EURUSD.SIM");

        let mut crypto_order = create_test_order_submitted();
        crypto_order.instrument_id = InstrumentId::from("BTCUSD.COINBASE");

        assert_ne!(fx_order, crypto_order);
        assert_eq!(fx_order.instrument_id, InstrumentId::from("EURUSD.SIM"));
        assert_eq!(
            crypto_order.instrument_id,
            InstrumentId::from("BTCUSD.COINBASE")
        );
    }
}
