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

use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;
use nautilus_core::{
    UUID4, UnixNanos,
    correctness::{FAILED, check_predicate_false},
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{Order, OrderAny, OrderCore, OrderError};
use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide,
        TimeInForce, TrailingOffsetType, TriggerType,
    },
    events::{OrderEventAny, OrderInitialized, OrderUpdated},
    identifiers::{
        ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId, StrategyId,
        TraderId, Venue, VenueOrderId,
    },
    types::{
        Currency, Money, Price, Quantity, price::check_positive_price,
        quantity::check_positive_quantity,
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct LimitIfTouchedOrder {
    pub price: Price,
    pub trigger_price: Price,
    pub trigger_type: TriggerType,
    pub expire_time: Option<UnixNanos>,
    pub is_post_only: bool,
    pub display_qty: Option<Quantity>,
    pub trigger_instrument_id: Option<InstrumentId>,
    pub is_triggered: bool,
    pub ts_triggered: Option<UnixNanos>,
    core: OrderCore,
}

impl LimitIfTouchedOrder {
    #[allow(clippy::too_many_arguments)]
    pub fn new_checked(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Vec<Ustr>>,
        init_id: UUID4,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        check_positive_quantity(quantity, "quantity")?;
        check_positive_price(price, "price")?;
        check_positive_price(trigger_price, "trigger_price")?;

        if let Some(disp) = display_qty {
            check_positive_quantity(disp, "display_qty")?;
            check_predicate_false(disp > quantity, "`display_qty` may not exceed `quantity`")?;
        }

        if matches!(time_in_force, TimeInForce::Gtd) {
            let exp = expire_time.unwrap_or_default();
            check_predicate_false(exp == 0, "`expire_time` required for GTD orders")?;
        }

        match order_side {
            OrderSide::Buy if trigger_price < price => {
                anyhow::bail!("BUY LIT orders require trigger_price ≥ price")
            }
            OrderSide::Sell if trigger_price > price => {
                anyhow::bail!("SELL LIT orders require trigger_price ≤ price")
            }
            _ => {}
        }

        Ok(Self::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            price,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time,
            post_only,
            reduce_only,
            quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            init_id,
            ts_init,
        ))
    }

    /// Creates a new [`LimitIfTouchedOrder`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `quantity` is not positive.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Vec<Ustr>>,
        init_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            price,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time,
            post_only,
            reduce_only,
            quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            init_id,
            ts_init,
        )
        .expect(FAILED)
    }
}

impl Deref for LimitIfTouchedOrder {
    type Target = OrderCore;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}
impl DerefMut for LimitIfTouchedOrder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Order for LimitIfTouchedOrder {
    fn into_any(self) -> OrderAny {
        OrderAny::LimitIfTouched(self)
    }

    fn status(&self) -> OrderStatus {
        self.status
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

    fn symbol(&self) -> crate::identifiers::Symbol {
        self.instrument_id.symbol
    }

    fn venue(&self) -> Venue {
        self.instrument_id.venue
    }

    fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    fn venue_order_id(&self) -> Option<VenueOrderId> {
        self.venue_order_id
    }

    fn position_id(&self) -> Option<PositionId> {
        self.position_id
    }

    fn account_id(&self) -> Option<crate::identifiers::AccountId> {
        self.account_id
    }

    fn last_trade_id(&self) -> Option<crate::identifiers::TradeId> {
        self.last_trade_id
    }

    fn order_side(&self) -> OrderSide {
        self.side
    }

    fn order_type(&self) -> OrderType {
        self.order_type
    }

    fn quantity(&self) -> Quantity {
        self.quantity
    }

    fn time_in_force(&self) -> TimeInForce {
        self.time_in_force
    }

    fn expire_time(&self) -> Option<UnixNanos> {
        self.expire_time
    }

    fn price(&self) -> Option<Price> {
        Some(self.price)
    }

    fn trigger_price(&self) -> Option<Price> {
        Some(self.trigger_price)
    }

    fn trigger_type(&self) -> Option<TriggerType> {
        Some(self.trigger_type)
    }

    fn liquidity_side(&self) -> Option<LiquiditySide> {
        self.liquidity_side
    }

    fn is_post_only(&self) -> bool {
        self.is_post_only
    }

    fn is_reduce_only(&self) -> bool {
        self.is_reduce_only
    }

    fn is_quote_quantity(&self) -> bool {
        self.is_quote_quantity
    }

    fn has_price(&self) -> bool {
        true
    }

    fn display_qty(&self) -> Option<Quantity> {
        self.display_qty
    }

    fn limit_offset(&self) -> Option<rust_decimal::Decimal> {
        None
    }

    fn trailing_offset(&self) -> Option<rust_decimal::Decimal> {
        None
    }

    fn trailing_offset_type(&self) -> Option<TrailingOffsetType> {
        None
    }

    fn emulation_trigger(&self) -> Option<TriggerType> {
        self.emulation_trigger
    }

    fn trigger_instrument_id(&self) -> Option<InstrumentId> {
        self.trigger_instrument_id
    }

    fn contingency_type(&self) -> Option<ContingencyType> {
        self.contingency_type
    }

    fn order_list_id(&self) -> Option<OrderListId> {
        self.order_list_id
    }

    fn linked_order_ids(&self) -> Option<&[ClientOrderId]> {
        self.linked_order_ids.as_deref()
    }

    fn parent_order_id(&self) -> Option<ClientOrderId> {
        self.parent_order_id
    }

    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id
    }

    fn exec_algorithm_params(&self) -> Option<&IndexMap<Ustr, Ustr>> {
        self.exec_algorithm_params.as_ref()
    }

    fn exec_spawn_id(&self) -> Option<ClientOrderId> {
        self.exec_spawn_id
    }

    fn tags(&self) -> Option<&[Ustr]> {
        self.tags.as_deref()
    }

    fn filled_qty(&self) -> Quantity {
        self.filled_qty
    }

    fn leaves_qty(&self) -> Quantity {
        self.leaves_qty
    }

    fn avg_px(&self) -> Option<f64> {
        self.avg_px
    }

    fn slippage(&self) -> Option<f64> {
        self.slippage
    }

    fn init_id(&self) -> UUID4 {
        self.init_id
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    fn ts_submitted(&self) -> Option<UnixNanos> {
        self.ts_submitted
    }

    fn ts_accepted(&self) -> Option<UnixNanos> {
        self.ts_accepted
    }

    fn ts_closed(&self) -> Option<UnixNanos> {
        self.ts_closed
    }

    fn ts_last(&self) -> UnixNanos {
        self.ts_last
    }

    fn commissions(&self) -> &IndexMap<Currency, Money> {
        &self.commissions
    }

    fn events(&self) -> Vec<&crate::events::OrderEventAny> {
        self.events.iter().collect()
    }

    fn venue_order_ids(&self) -> Vec<&VenueOrderId> {
        self.venue_order_ids.iter().collect()
    }

    fn trade_ids(&self) -> Vec<&crate::identifiers::TradeId> {
        self.trade_ids.iter().collect()
    }

    fn apply(&mut self, event: OrderEventAny) -> Result<(), OrderError> {
        if let OrderEventAny::Updated(ref upd) = event {
            self.update(upd);
        }
        let filled = matches!(event, OrderEventAny::Filled(_));
        self.core.apply(event)?;
        if filled {
            self.core.set_slippage(self.price);
        }
        Ok(())
    }

    fn update(&mut self, event: &OrderUpdated) {
        if let Some(px) = event.price {
            self.price = px;
        }
        if let Some(tp) = event.trigger_price {
            self.trigger_price = tp;
        }
        self.quantity = event.quantity;
        self.leaves_qty = self.quantity - self.filled_qty;
    }

    fn is_triggered(&self) -> Option<bool> {
        Some(self.is_triggered)
    }

    // setters that forward into `self`
    fn set_position_id(&mut self, id: Option<PositionId>) {
        self.position_id = id;
    }
    fn set_quantity(&mut self, q: Quantity) {
        self.quantity = q;
    }
    fn set_leaves_qty(&mut self, lq: Quantity) {
        self.leaves_qty = lq;
    }
    fn set_emulation_trigger(&mut self, trig: Option<TriggerType>) {
        self.emulation_trigger = trig;
    }
    fn set_is_quote_quantity(&mut self, val: bool) {
        self.is_quote_quantity = val;
    }
    fn set_liquidity_side(&mut self, side: LiquiditySide) {
        self.liquidity_side = Some(side);
    }

    fn would_reduce_only(&self, side: PositionSide, pos_qty: Quantity) -> bool {
        self.core.would_reduce_only(side, pos_qty)
    }

    fn previous_status(&self) -> Option<OrderStatus> {
        self.core.previous_status
    }
}

impl From<OrderInitialized> for LimitIfTouchedOrder {
    fn from(e: OrderInitialized) -> Self {
        Self::new(
            e.trader_id,
            e.strategy_id,
            e.instrument_id,
            e.client_order_id,
            e.order_side,
            e.quantity,
            e.price.expect("price missing"),
            e.trigger_price.expect("trigger_price missing"),
            e.trigger_type.expect("trigger_type missing"),
            e.time_in_force,
            e.expire_time,
            e.post_only,
            e.reduce_only,
            e.quote_quantity,
            e.display_qty,
            e.emulation_trigger,
            e.trigger_instrument_id,
            e.contingency_type,
            e.order_list_id,
            e.linked_order_ids,
            e.parent_order_id,
            e.exec_algorithm_id,
            e.exec_algorithm_params,
            e.exec_spawn_id,
            e.tags,
            e.event_id,
            e.ts_event,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        enums::{OrderSide, TimeInForce, TriggerType},
        types::{Price, Quantity},
    };

    fn ids() -> (
        TraderId,
        StrategyId,
        InstrumentId,
        ClientOrderId,
        UUID4,
        UnixNanos,
    ) {
        (
            TraderId::from("TRADER‑1"),
            StrategyId::from("STRAT‑1"),
            InstrumentId::default(),
            ClientOrderId::from("CL‑1"),
            UUID4::new(),
            UnixNanos::new(0),
        )
    }

    #[test]
    fn ok() {
        let (t, s, i, c, u, ts) = ids();
        let o = LimitIfTouchedOrder::new_checked(
            t,
            s,
            i,
            c,
            OrderSide::Buy,
            Quantity::from(1),
            Price::from("30000"),
            Price::from("30200"),
            TriggerType::LastPrice,
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            u,
            ts,
        )
        .unwrap();
        assert_eq!(o.quantity(), Quantity::from(1));
    }

    #[test]
    fn quantity_zero() {
        let (t, s, i, c, u, ts) = ids();
        assert!(
            LimitIfTouchedOrder::new_checked(
                t,
                s,
                i,
                c,
                OrderSide::Buy,
                Quantity::from(0),
                Price::from("1"),
                Price::from("1"),
                TriggerType::LastPrice,
                TimeInForce::Gtc,
                None,
                false,
                false,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                u,
                ts,
            )
            .unwrap_err()
            .to_string()
            .contains("not positive")
        );
    }

    #[test]
    fn gtd_without_expire() {
        let (t, s, i, c, u, ts) = ids();
        assert!(
            LimitIfTouchedOrder::new_checked(
                t,
                s,
                i,
                c,
                OrderSide::Buy,
                Quantity::from(1),
                Price::from("1"),
                Price::from("1"),
                TriggerType::LastPrice,
                TimeInForce::Gtd,
                None,
                false,
                false,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                u,
                ts,
            )
            .unwrap_err()
            .to_string()
            .contains("expire_time")
        );
    }

    #[test]
    fn buy_trigger_lt_price() {
        let (t, s, i, c, u, ts) = ids();
        assert!(
            LimitIfTouchedOrder::new_checked(
                t,
                s,
                i,
                c,
                OrderSide::Buy,
                Quantity::from(1),
                Price::from("31000"),
                Price::from("30000"),
                TriggerType::LastPrice,
                TimeInForce::Gtc,
                None,
                false,
                false,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                u,
                ts,
            )
            .unwrap_err()
            .to_string()
            .contains("trigger_price")
        );
    }
}
