// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    collections::HashMap,
    fmt::Display,
    ops::{Deref, DerefMut},
};

use nautilus_core::{time::UnixNanos, uuid::UUID4};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::base::{Order, OrderCore};
use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce,
        TrailingOffsetType, TriggerType,
    },
    events::order::{event::OrderEvent, initialized::OrderInitialized, updated::OrderUpdated},
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, exec_algorithm_id::ExecAlgorithmId,
        instrument_id::InstrumentId, order_list_id::OrderListId, position_id::PositionId,
        strategy_id::StrategyId, symbol::Symbol, trade_id::TradeId, trader_id::TraderId,
        venue::Venue, venue_order_id::VenueOrderId,
    },
    orders::base::OrderError,
    types::{
        price::Price,
        quantity::{check_quantity_positive, Quantity},
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct MarketOrder {
    core: OrderCore,
}

impl MarketOrder {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: TimeInForce,
        init_id: UUID4,
        ts_init: UnixNanos,
        reduce_only: bool,
        quote_quantity: bool,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<HashMap<Ustr, Ustr>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Ustr>,
    ) -> anyhow::Result<Self> {
        check_quantity_positive(quantity)?;
        if time_in_force == TimeInForce::Gtd {
            anyhow::bail!("{}", "GTD not supported for Market orders");
        }

        Ok(Self {
            core: OrderCore::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                order_side,
                OrderType::Market,
                quantity,
                time_in_force,
                reduce_only,
                quote_quantity,
                None, // Emulation trigger
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
            ),
        })
    }
}

impl Deref for MarketOrder {
    type Target = OrderCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for MarketOrder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl PartialEq for MarketOrder {
    fn eq(&self, other: &Self) -> bool {
        self.client_order_id == other.client_order_id
    }
}

impl Order for MarketOrder {
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

    fn symbol(&self) -> Symbol {
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

    fn account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    fn last_trade_id(&self) -> Option<TradeId> {
        self.last_trade_id
    }

    fn side(&self) -> OrderSide {
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
        None
    }

    fn price(&self) -> Option<Price> {
        None
    }

    fn trigger_price(&self) -> Option<Price> {
        None
    }

    fn trigger_type(&self) -> Option<TriggerType> {
        None
    }

    fn liquidity_side(&self) -> Option<LiquiditySide> {
        self.liquidity_side
    }

    fn is_post_only(&self) -> bool {
        false
    }

    fn is_reduce_only(&self) -> bool {
        self.is_reduce_only
    }

    fn is_quote_quantity(&self) -> bool {
        self.is_quote_quantity
    }

    fn display_qty(&self) -> Option<Quantity> {
        None
    }

    fn limit_offset(&self) -> Option<Price> {
        None
    }

    fn trailing_offset(&self) -> Option<Price> {
        None
    }

    fn trailing_offset_type(&self) -> Option<TrailingOffsetType> {
        None
    }

    fn emulation_trigger(&self) -> Option<TriggerType> {
        None
    }

    fn trigger_instrument_id(&self) -> Option<InstrumentId> {
        None
    }

    fn contingency_type(&self) -> Option<ContingencyType> {
        self.contingency_type
    }

    fn order_list_id(&self) -> Option<OrderListId> {
        self.order_list_id
    }

    fn linked_order_ids(&self) -> Option<Vec<ClientOrderId>> {
        self.linked_order_ids.clone()
    }

    fn parent_order_id(&self) -> Option<ClientOrderId> {
        self.parent_order_id
    }

    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id
    }

    fn exec_algorithm_params(&self) -> Option<HashMap<Ustr, Ustr>> {
        self.exec_algorithm_params.clone()
    }

    fn exec_spawn_id(&self) -> Option<ClientOrderId> {
        self.exec_spawn_id
    }

    fn tags(&self) -> Option<Ustr> {
        self.tags
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

    fn ts_last(&self) -> UnixNanos {
        self.ts_last
    }

    fn events(&self) -> Vec<&OrderEvent> {
        self.events.iter().collect()
    }

    fn venue_order_ids(&self) -> Vec<&VenueOrderId> {
        self.venue_order_ids.iter().collect()
    }

    fn trade_ids(&self) -> Vec<&TradeId> {
        self.trade_ids.iter().collect()
    }

    fn apply(&mut self, event: OrderEvent) -> Result<(), OrderError> {
        if let OrderEvent::OrderUpdated(ref event) = event {
            self.update(event);
        };

        self.core.apply(event)?;

        Ok(())
    }

    fn update(&mut self, event: &OrderUpdated) {
        assert!(event.price.is_none(), "{}", OrderError::InvalidOrderEvent);
        assert!(
            event.trigger_price.is_none(),
            "{}",
            OrderError::InvalidOrderEvent
        );

        self.quantity = event.quantity;
        self.leaves_qty = self.quantity - self.filled_qty;
    }
}

impl Display for MarketOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MarketOrder(\
            {} {} {} @ {} {}, \
            status={}, \
            client_order_id={}, \
            venue_order_id={}, \
            position_id={}, \
            exec_algorithm_id={}, \
            exec_spawn_id={}, \
            tags={:?}\
            )",
            self.side,
            self.quantity.to_formatted_string(),
            self.instrument_id,
            self.order_type,
            self.time_in_force,
            self.status,
            self.client_order_id,
            self.venue_order_id.map_or_else(
                || "None".to_string(),
                |venue_order_id| format!("{venue_order_id}")
            ),
            self.position_id.map_or_else(
                || "None".to_string(),
                |position_id| format!("{position_id}")
            ),
            self.exec_algorithm_id
                .map_or_else(|| "None".to_string(), |id| format!("{id}")),
            self.exec_spawn_id
                .map_or_else(|| "None".to_string(), |id| format!("{id}")),
            self.tags
        )
    }
}

impl From<OrderInitialized> for MarketOrder {
    fn from(event: OrderInitialized) -> Self {
        Self::new(
            event.trader_id,
            event.strategy_id,
            event.instrument_id,
            event.client_order_id,
            event.order_side,
            event.quantity,
            event.time_in_force,
            event.event_id,
            event.ts_event,
            event.reduce_only,
            event.quote_quantity,
            event.contingency_type,
            event.order_list_id,
            event.linked_order_ids,
            event.parent_order_id,
            event.exec_algorithm_id,
            event.exec_algorithm_params,
            event.exec_spawn_id,
            event.tags,
        )
        .unwrap() // SAFETY: From can panic
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        enums::{OrderSide, TimeInForce},
        instruments::{currency_pair::CurrencyPair, stubs::*},
        orders::stubs::*,
        types::quantity::Quantity,
    };

    #[rstest]
    #[should_panic(expected = "Condition failed: invalid `Quantity`, should be positive and was 0")]
    fn test_positive_quantity_condition(audusd_sim: CurrencyPair) {
        let _ = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(0),
            None,
            None,
        );
    }

    #[rstest]
    #[should_panic(expected = "GTD not supported for Market orders")]
    fn test_gtd_condition(audusd_sim: CurrencyPair) {
        let _ = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(100),
            None,
            Some(TimeInForce::Gtd),
        );
    }
}
