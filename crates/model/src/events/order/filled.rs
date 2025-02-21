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
        ContingencyType, LiquiditySide, OrderSide, OrderSideSpecified, OrderType, TimeInForce,
        TrailingOffsetType, TriggerType,
    },
    events::OrderEvent,
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{Currency, Money, Price, Quantity},
};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[builder(default)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderFilled {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    /// The account ID associated with the event.
    pub account_id: AccountId,
    /// The trade match ID (assigned by the venue).
    pub trade_id: TradeId,
    /// The order side.
    pub order_side: OrderSide,
    /// The order type.
    pub order_type: OrderType,
    /// The fill quantity for this execution.
    pub last_qty: Quantity,
    /// The fill price for this execution.
    pub last_px: Price,
    /// The currency of the `last_px`.
    pub currency: Currency,
    /// The liquidity side of the execution.
    pub liquidity_side: LiquiditySide,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// If the event was generated during reconciliation.
    pub reconciliation: bool,
    /// The position ID (assigned by the venue).
    pub position_id: Option<PositionId>,
    /// The commission generated from this execution.
    pub commission: Option<Money>,
}

impl OrderFilled {
    /// Creates a new [`OrderFilled`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        trade_id: TradeId,
        order_side: OrderSide,
        order_type: OrderType,
        last_qty: Quantity,
        last_px: Price,
        currency: Currency,
        liquidity_side: LiquiditySide,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reconciliation: bool,
        position_id: Option<PositionId>,
        commission: Option<Money>,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            trade_id,
            order_side,
            order_type,
            last_qty,
            last_px,
            currency,
            liquidity_side,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            position_id,
            commission,
        }
    }

    #[must_use]
    pub fn specified_side(&self) -> OrderSideSpecified {
        self.order_side.as_specified()
    }

    #[must_use]
    pub fn is_buy(&self) -> bool {
        self.order_side == OrderSide::Buy
    }

    #[must_use]
    pub fn is_sell(&self) -> bool {
        self.order_side == OrderSide::Sell
    }
}

impl Default for OrderFilled {
    /// Creates a new default [`OrderFilled`] instance for testing.
    fn default() -> Self {
        Self {
            trader_id: TraderId::default(),
            strategy_id: StrategyId::default(),
            instrument_id: InstrumentId::default(),
            client_order_id: ClientOrderId::default(),
            venue_order_id: VenueOrderId::default(),
            account_id: AccountId::default(),
            trade_id: TradeId::default(),
            position_id: None,
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            last_qty: Quantity::new(100_000.0, 0),
            last_px: Price::from("1.00000"),
            currency: Currency::USD(),
            commission: None,
            liquidity_side: LiquiditySide::Taker,
            event_id: Default::default(),
            ts_event: Default::default(),
            ts_init: Default::default(),
            reconciliation: Default::default(),
        }
    }
}

impl Debug for OrderFilled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let position_id_str = match self.position_id {
            Some(position_id) => position_id.to_string(),
            None => "None".to_string(),
        };
        let commission_str = match self.commission {
            Some(commission) => commission.to_string(),
            None => "None".to_string(),
        };
        write!(
            f,
            "{}(\
            trader_id={}, \
            strategy_id={}, \
            instrument_id={}, \
            client_order_id={}, \
            venue_order_id={}, \
            account_id={}, \
            trade_id={}, \
            position_id={}, \
            order_side={}, \
            order_type={}, \
            last_qty={}, \
            last_px={} {}, \
            commission={}, \
            liquidity_side={}, \
            event_id={}, \
            ts_event={}, \
            ts_init={})",
            stringify!(OrderFilled),
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.account_id,
            self.trade_id,
            position_id_str,
            self.order_side,
            self.order_type,
            self.last_qty.to_formatted_string(),
            self.last_px.to_formatted_string(),
            self.currency,
            commission_str,
            self.liquidity_side,
            self.event_id,
            self.ts_event,
            self.ts_init
        )
    }
}

impl Display for OrderFilled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(\
            instrument_id={}, \
            client_order_id={}, \
            venue_order_id={}, \
            account_id={}, \
            trade_id={}, \
            position_id={}, \
            order_side={}, \
            order_type={}, \
            last_qty={}, \
            last_px={} {}, \
            commission={}, \
            liquidity_side={}, \
            ts_event={})",
            stringify!(OrderFilled),
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.account_id,
            self.trade_id,
            self.position_id.unwrap_or_default(),
            self.order_side,
            self.order_type,
            self.last_qty.to_formatted_string(),
            self.last_px.to_formatted_string(),
            self.currency,
            self.commission.unwrap_or(Money::from("0.0 USD")),
            self.liquidity_side,
            self.ts_event
        )
    }
}

impl OrderEvent for OrderFilled {
    fn id(&self) -> UUID4 {
        self.event_id
    }

    fn kind(&self) -> &str {
        stringify!(OrderFilled)
    }

    fn order_type(&self) -> Option<OrderType> {
        Some(self.order_type)
    }

    fn order_side(&self) -> Option<OrderSide> {
        Some(self.order_side)
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
        Some(self.trade_id)
    }

    fn currency(&self) -> Option<Currency> {
        Some(self.currency)
    }

    fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    fn reason(&self) -> Option<Ustr> {
        None
    }

    fn quantity(&self) -> Option<Quantity> {
        Some(self.last_qty)
    }

    fn time_in_force(&self) -> Option<TimeInForce> {
        None
    }

    fn liquidity_side(&self) -> Option<LiquiditySide> {
        Some(self.liquidity_side)
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
        Some(self.last_px)
    }

    fn last_qty(&self) -> Option<Quantity> {
        Some(self.last_qty)
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
        self.position_id
    }

    fn commission(&self) -> Option<Money> {
        self.commission
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
    use rstest::rstest;

    use crate::events::{OrderFilled, order::stubs::*};

    #[rstest]
    fn test_order_filled_display(order_filled: OrderFilled) {
        let display = format!("{order_filled}");
        assert_eq!(
            display,
            "OrderFilled(instrument_id=BTCUSDT.COINBASE, client_order_id=O-19700101-000000-001-001-1, \
            venue_order_id=123456, account_id=SIM-001, trade_id=1, position_id=P-001, \
            order_side=BUY, order_type=LIMIT, last_qty=0.561, last_px=22_000 USDT, \
            commission=12.20000000 USDT, liquidity_side=TAKER, ts_event=0)"
        );
    }

    #[rstest]
    fn test_order_filled_is_buy(order_filled: OrderFilled) {
        assert!(order_filled.is_buy());
        assert!(!order_filled.is_sell());
    }
}
