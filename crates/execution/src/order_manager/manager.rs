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

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use ahash::AHashMap;
use nautilus_common::{cache::Cache, clock::Clock, messages::execution::SubmitOrder};
use nautilus_core::UUID4;
use nautilus_model::{
    enums::ContingencyType,
    events::{
        OrderCanceled, OrderEventAny, OrderExpired, OrderFilled, OrderRejected, OrderUpdated,
    },
    identifiers::{ClientId, ClientOrderId, PositionId},
    orders::{Order, OrderAny},
    types::Quantity,
};

use super::OrderManagerAction;

/// Manages the lifecycle and state of orders with contingency handling.
///
/// The order manager is responsible for managing local order state, handling
/// contingent orders (OTO, OCO, OUO), and coordinating with emulation and
/// execution systems. It tracks order commands and manages complex order
/// relationships for advanced order types.
pub struct OrderManager {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    active_local: bool,
    submit_order_commands: AHashMap<ClientOrderId, SubmitOrder>,
    oto_target_quantities: AHashMap<ClientOrderId, Quantity>,
}

impl Debug for OrderManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OrderManager))
            .field("pending_commands", &self.submit_order_commands.len())
            .finish()
    }
}

impl OrderManager {
    /// Creates a new [`OrderManager`] instance.
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        active_local: bool,
    ) -> Self {
        Self {
            clock,
            cache,
            active_local,
            submit_order_commands: AHashMap::new(),
            oto_target_quantities: AHashMap::new(),
        }
    }

    #[must_use]
    /// Returns a copy of all cached submit order commands.
    pub fn get_submit_order_commands(&self) -> AHashMap<ClientOrderId, SubmitOrder> {
        self.submit_order_commands.clone()
    }

    /// Caches a submit order command for later processing.
    pub fn cache_submit_order_command(&mut self, command: SubmitOrder) {
        self.submit_order_commands
            .insert(command.client_order_id, command);
    }

    /// Removes and returns a cached submit order command.
    pub fn pop_submit_order_command(
        &mut self,
        client_order_id: ClientOrderId,
    ) -> Option<SubmitOrder> {
        self.submit_order_commands.remove(&client_order_id)
    }

    /// Resets the order manager by clearing all stateful values.
    pub fn reset(&mut self) {
        self.submit_order_commands.clear();
        self.oto_target_quantities.clear();
    }

    /// Cancels an order if it's not already pending cancellation or closed.
    pub fn cancel_order(&mut self, order: &OrderAny) -> Vec<OrderManagerAction> {
        let client_order_id = order.client_order_id();
        let cache = self.cache.borrow();

        if cache.is_order_pending_cancel_local(&client_order_id) {
            return Vec::new();
        }

        if order.is_closed() || cache.is_order_closed(&client_order_id) {
            log::warn!("Cannot cancel order: already closed");
            return Vec::new();
        }

        drop(cache);
        self.submit_order_commands.remove(&client_order_id);

        vec![OrderManagerAction::CancelLocal(order.clone())]
    }

    /// Modifies the quantity of an existing order.
    pub fn modify_order_quantity(
        &mut self,
        order: &OrderAny,
        new_quantity: Quantity,
    ) -> Vec<OrderManagerAction> {
        vec![OrderManagerAction::ModifyLocalQuantity {
            order: order.clone(),
            quantity: new_quantity,
        }]
    }

    /// # Errors
    ///
    /// Returns an error if creating a new submit order fails.
    pub fn create_new_submit_order(
        &mut self,
        order: &OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        correlation_id: Option<UUID4>,
    ) -> anyhow::Result<Vec<OrderManagerAction>> {
        let mut actions = Vec::new();
        let order_exists = self.cache.borrow().order_exists(&order.client_order_id());

        self.cache
            .borrow_mut()
            .add_order(order.clone(), position_id, client_id, true)?;

        if !order_exists {
            actions.push(initialized_action(order));
        }

        let submit = SubmitOrder::new(
            order.trader_id(),
            client_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.init_event().clone(),
            order.exec_algorithm_id(),
            position_id,
            None, // params
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
            correlation_id,
        );

        if order.emulation_trigger().is_some() {
            self.cache_submit_order_command(submit.clone());
            actions.push(OrderManagerAction::SubmitToEmulator(submit));
        } else {
            self.cache_submit_order_command(submit.clone());

            match order.exec_algorithm_id() {
                Some(exec_algorithm_id) => {
                    actions.push(OrderManagerAction::SubmitToAlgorithm {
                        command: submit,
                        exec_algorithm_id,
                    });
                }
                None => actions.push(OrderManagerAction::SubmitToRisk(submit)),
            }
        }

        Ok(actions)
    }

    #[must_use]
    /// Returns true if the order manager should manage the given order.
    pub fn should_manage_order(&self, order: &OrderAny) -> bool {
        self.active_local == order.is_active_local()
    }

    /// Handles an order event by routing it to the appropriate handler method.
    ///
    /// Note: Only handles specific terminal/actionable events. Other events
    /// like `OrderSubmitted`, `OrderAccepted`, etc. are no-ops for the order manager.
    pub fn handle_event(&mut self, event: &OrderEventAny) -> Vec<OrderManagerAction> {
        match event {
            OrderEventAny::Rejected(event) => self.handle_order_rejected(*event),
            OrderEventAny::Canceled(event) => self.handle_order_canceled(*event),
            OrderEventAny::Expired(event) => self.handle_order_expired(*event),
            OrderEventAny::Updated(event) => self.handle_order_updated(*event),
            OrderEventAny::ModifyRejected(event) => {
                self.oto_target_quantities.remove(&event.client_order_id);
                Vec::new()
            }
            OrderEventAny::Filled(event) => self.handle_order_filled(event),
            _ => Vec::new(),
        }
    }

    /// Handles an order rejected event and manages any contingent orders.
    pub fn handle_order_rejected(&mut self, rejected: OrderRejected) -> Vec<OrderManagerAction> {
        self.oto_target_quantities.remove(&rejected.client_order_id);
        let Some(order) = self.cache.borrow().order_owned(&rejected.client_order_id) else {
            log::error!(
                "Cannot handle `OrderRejected`: order for client_order_id: {} not found, {}",
                rejected.client_order_id,
                rejected
            );
            return Vec::new();
        };

        if order.contingency_type().is_some() {
            self.handle_contingencies(&order)
        } else {
            Vec::new()
        }
    }

    pub fn handle_order_canceled(&mut self, canceled: OrderCanceled) -> Vec<OrderManagerAction> {
        self.oto_target_quantities.remove(&canceled.client_order_id);
        let Some(order) = self.cache.borrow().order_owned(&canceled.client_order_id) else {
            log::error!(
                "Cannot handle `OrderCanceled`: order for client_order_id: {} not found, {}",
                canceled.client_order_id,
                canceled
            );
            return Vec::new();
        };

        if order.contingency_type().is_some() {
            self.handle_contingencies(&order)
        } else {
            Vec::new()
        }
    }

    pub fn handle_order_expired(&mut self, expired: OrderExpired) -> Vec<OrderManagerAction> {
        self.oto_target_quantities.remove(&expired.client_order_id);
        let Some(order) = self.cache.borrow().order_owned(&expired.client_order_id) else {
            log::error!(
                "Cannot handle `OrderExpired`: order for client_order_id: {} not found, {}",
                expired.client_order_id,
                expired
            );
            return Vec::new();
        };

        if order.contingency_type().is_some() {
            self.handle_contingencies(&order)
        } else {
            Vec::new()
        }
    }

    pub fn handle_order_updated(&mut self, updated: OrderUpdated) -> Vec<OrderManagerAction> {
        let Some(order) = self.cache.borrow().order_owned(&updated.client_order_id) else {
            log::error!(
                "Cannot handle `OrderUpdated`: order for client_order_id: {} not found, {}",
                updated.client_order_id,
                updated
            );
            return Vec::new();
        };

        let mut actions = Vec::new();

        if self.should_manage_order(&order) && !order.is_closed() {
            if let Some(quantity) = self
                .oto_target_quantities
                .get(&updated.client_order_id)
                .copied()
                .filter(|quantity| *quantity != order.quantity())
                .filter(|quantity| order.filled_qty() < *quantity)
            {
                actions.extend(self.modify_order_quantity(&order, quantity));
            }
        } else {
            self.oto_target_quantities.remove(&updated.client_order_id);
        }

        if order.contingency_type().is_some() {
            actions.extend(self.handle_contingencies_update(&order));
        }

        actions
    }

    pub fn handle_order_filled(&mut self, filled: &OrderFilled) -> Vec<OrderManagerAction> {
        let Some(order) = self.cache.borrow().order_owned(&filled.client_order_id) else {
            log::error!(
                "Cannot handle `OrderFilled`: order for client_order_id: {} not found, {}",
                filled.client_order_id,
                filled
            );
            return Vec::new();
        };

        let mut actions = Vec::new();

        if !self.should_manage_order(&order) || order.is_closed() {
            self.oto_target_quantities.remove(&filled.client_order_id);
        }

        match order.contingency_type() {
            Some(ContingencyType::Oto) => {
                let position_id = self
                    .cache
                    .borrow()
                    .position_id(&order.client_order_id())
                    .copied();
                let client_id = self
                    .cache
                    .borrow()
                    .client_id(&order.client_order_id())
                    .copied();

                let parent_filled_qty = match order.exec_spawn_id() {
                    Some(spawn_id) => {
                        if let Some(qty) = self
                            .cache
                            .borrow()
                            .exec_spawn_total_filled_qty(&spawn_id, false)
                        {
                            qty
                        } else {
                            log::error!("Failed to get spawn filled quantity for {spawn_id}");
                            return actions;
                        }
                    }
                    None => order.filled_qty(),
                };

                let Some(linked_orders) = order.linked_order_ids() else {
                    log::error!("No linked orders found for OTO order");
                    return actions;
                };

                for client_order_id in linked_orders {
                    if order.parent_order_id().as_ref() == Some(client_order_id) {
                        continue;
                    }

                    let Some(mut child_order) = self.cache.borrow().order_owned(client_order_id)
                    else {
                        log::error!(
                            "Cannot find OTO child order for client_order_id: {client_order_id}"
                        );
                        continue;
                    };

                    if !self.should_manage_order(&child_order) || child_order.is_closed() {
                        self.oto_target_quantities.remove(client_order_id);
                        continue;
                    }

                    if self.active_local && child_order.position_id().is_none() {
                        child_order.set_position_id(position_id);
                    }

                    actions.extend(self.sync_oto_quantity(&child_order, parent_filled_qty));

                    if self.active_local
                        && !self
                            .submit_order_commands
                            .contains_key(&child_order.client_order_id())
                    {
                        match self.create_new_submit_order(
                            &child_order,
                            position_id,
                            client_id,
                            None,
                        ) {
                            Ok(new_actions) => actions.extend(new_actions),
                            Err(e) => log::error!("Failed to create new submit order: {e}"),
                        }
                    }
                }
            }
            Some(ContingencyType::Oco) => {
                let Some(linked_orders) = order.linked_order_ids() else {
                    log::error!("No linked orders found for OCO order");
                    return actions;
                };

                for client_order_id in linked_orders {
                    let Some(contingent_order) = self.cache.borrow().order_owned(client_order_id)
                    else {
                        log::error!(
                            "Cannot find OCO contingent order for client_order_id: {client_order_id}"
                        );
                        continue;
                    };

                    // Not being managed || Already completed
                    if !self.should_manage_order(&contingent_order) || contingent_order.is_closed()
                    {
                        continue;
                    }

                    if contingent_order.client_order_id() != order.client_order_id() {
                        actions.extend(self.cancel_order(&contingent_order));
                    }
                }
            }
            Some(ContingencyType::Ouo) => actions.extend(self.handle_contingencies(&order)),
            _ => {}
        }

        actions
    }

    pub fn handle_contingencies(&mut self, order: &OrderAny) -> Vec<OrderManagerAction> {
        let mut actions = Vec::new();
        let (filled_qty, leaves_qty, is_spawn_active) =
            if let Some(exec_spawn_id) = order.exec_spawn_id() {
                if let (Some(filled), Some(leaves)) = (
                    self.cache
                        .borrow()
                        .exec_spawn_total_filled_qty(&exec_spawn_id, false),
                    self.cache
                        .borrow()
                        .exec_spawn_total_leaves_qty(&exec_spawn_id, true),
                ) {
                    (filled, leaves, leaves.raw > 0)
                } else {
                    log::error!("Failed to get spawn quantities for {exec_spawn_id}");
                    return actions;
                }
            } else {
                (order.filled_qty(), order.leaves_qty(), false)
            };

        let Some(linked_orders) = order.linked_order_ids() else {
            log::error!("No linked orders found");
            return actions;
        };

        for client_order_id in linked_orders {
            if order.contingency_type() == Some(ContingencyType::Oto)
                && order.parent_order_id().as_ref() == Some(client_order_id)
            {
                continue;
            }

            let Some(contingent_order) = self.cache.borrow().order_owned(client_order_id) else {
                log::error!("Cannot find contingent order for client_order_id: {client_order_id}");
                continue;
            };

            if !self.should_manage_order(&contingent_order) {
                self.oto_target_quantities.remove(client_order_id);
                continue;
            }

            if client_order_id == &order.client_order_id() {
                continue;
            }

            if contingent_order.is_closed() {
                self.submit_order_commands.remove(client_order_id);
                self.oto_target_quantities.remove(client_order_id);
                continue;
            }

            match order.contingency_type() {
                Some(ContingencyType::Oto) => {
                    if order.is_closed()
                        && filled_qty.raw == 0
                        && (order.exec_spawn_id().is_none() || !is_spawn_active)
                    {
                        actions.extend(self.cancel_order(&contingent_order));
                    } else if filled_qty.raw > 0 {
                        actions.extend(self.sync_oto_quantity(&contingent_order, filled_qty));
                    }
                }
                Some(ContingencyType::Oco)
                    if order.is_closed()
                        && (order.exec_spawn_id().is_none() || !is_spawn_active) =>
                {
                    actions.extend(self.cancel_order(&contingent_order));
                }
                Some(ContingencyType::Ouo) => {
                    if (leaves_qty.raw == 0 && order.exec_spawn_id().is_some())
                        || (order.is_closed()
                            && (order.exec_spawn_id().is_none() || !is_spawn_active))
                        || contingent_order.filled_qty() >= leaves_qty
                    {
                        actions.extend(self.cancel_order(&contingent_order));
                    } else if leaves_qty != contingent_order.leaves_qty() {
                        actions.extend(self.modify_order_quantity(&contingent_order, leaves_qty));
                    }
                }
                _ => {}
            }
        }

        actions
    }

    pub fn handle_contingencies_update(&mut self, order: &OrderAny) -> Vec<OrderManagerAction> {
        let mut actions = Vec::new();
        let contingency_type = order.contingency_type();
        let quantity = match order.exec_spawn_id() {
            Some(exec_spawn_id) => {
                if let Some(qty) = self
                    .cache
                    .borrow()
                    .exec_spawn_total_quantity(&exec_spawn_id, true)
                {
                    qty
                } else {
                    log::error!("Failed to get spawn total quantity for {exec_spawn_id}");
                    return actions;
                }
            }
            None => order.quantity(),
        };

        if quantity.raw == 0 {
            return actions;
        }

        let oto_quantity = if contingency_type == Some(ContingencyType::Oto) {
            let filled_qty = match order.exec_spawn_id() {
                Some(exec_spawn_id) => {
                    if let Some(qty) = self
                        .cache
                        .borrow()
                        .exec_spawn_total_filled_qty(&exec_spawn_id, false)
                    {
                        qty
                    } else {
                        log::error!("Failed to get spawn filled quantity for {exec_spawn_id}");
                        return actions;
                    }
                }
                None => order.filled_qty(),
            };

            if filled_qty.raw > 0 {
                filled_qty
            } else {
                quantity
            }
        } else {
            quantity
        };

        let Some(linked_orders) = order.linked_order_ids() else {
            log::error!("No linked orders found for contingent order");
            return actions;
        };

        for client_order_id in linked_orders {
            if contingency_type == Some(ContingencyType::Oto)
                && order.parent_order_id().as_ref() == Some(client_order_id)
            {
                continue;
            }

            let Some(contingent_order) = self.cache.borrow().order_owned(client_order_id) else {
                log::error!(
                    "Cannot find OCO contingent order for client_order_id: {client_order_id}"
                );
                continue;
            };

            if !self.should_manage_order(&contingent_order) {
                self.oto_target_quantities.remove(client_order_id);
                continue;
            }

            if client_order_id == &order.client_order_id() || contingent_order.is_closed() {
                self.oto_target_quantities.remove(client_order_id);
                continue;
            }

            match contingency_type {
                Some(ContingencyType::Oto) => {
                    actions.extend(self.sync_oto_quantity(&contingent_order, oto_quantity));
                }
                Some(ContingencyType::Ouo) if contingent_order.filled_qty() >= quantity => {
                    actions.extend(self.cancel_order(&contingent_order));
                }
                Some(ContingencyType::Ouo) if quantity != contingent_order.quantity() => {
                    actions.extend(self.modify_order_quantity(&contingent_order, quantity));
                }
                _ => {}
            }
        }

        actions
    }

    fn sync_oto_quantity(
        &mut self,
        order: &OrderAny,
        quantity: Quantity,
    ) -> Vec<OrderManagerAction> {
        let client_order_id = order.client_order_id();

        if order.filled_qty() >= quantity {
            self.oto_target_quantities.remove(&client_order_id);
            return self.cancel_order(order);
        }

        match self.oto_target_quantities.get(&client_order_id).copied() {
            Some(target) if target == quantity => return Vec::new(),
            Some(_) if order.quantity() == quantity && !order.is_pending_update() => {
                self.oto_target_quantities.insert(client_order_id, quantity);
                return Vec::new();
            }
            Some(_) => {}
            None if order.quantity() == quantity => return Vec::new(),
            None => {}
        }

        self.oto_target_quantities.insert(client_order_id, quantity);
        self.modify_order_quantity(order, quantity)
    }
}

fn initialized_action(order: &OrderAny) -> OrderManagerAction {
    let event = OrderEventAny::Initialized(order.init_event().clone());
    OrderManagerAction::PublishInitialized(event)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{ContingencyType, OrderSide, OrderType, TriggerType},
        events::order::spec::{
            OrderAcceptedSpec, OrderCanceledSpec, OrderExpiredSpec, OrderModifyRejectedSpec,
            OrderPendingUpdateSpec, OrderRejectedSpec, OrderSubmittedSpec, OrderUpdatedSpec,
        },
        identifiers::{
            AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, StrategyId, TradeId, TraderId,
            VenueOrderId,
        },
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        orders::{Order, OrderTestBuilder, stubs::TestOrderEventStubs},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    /// Verifies unhandled order events are no-ops and don't panic.
    /// Previously, unhandled events would hit a todo!() panic.
    #[rstest]
    fn test_handle_event_unhandled_events_are_noop() {
        let submitted = OrderEventAny::Submitted(
            OrderSubmittedSpec::builder()
                .trader_id(TraderId::from("TRADER-001"))
                .strategy_id(StrategyId::from("STRATEGY-001"))
                .instrument_id(InstrumentId::from("BTC-USDT.OKX"))
                .client_order_id(ClientOrderId::from("O-001"))
                .account_id(AccountId::from("ACCOUNT-001"))
                .build(),
        );
        let accepted = OrderEventAny::Accepted(
            OrderAcceptedSpec::builder()
                .trader_id(TraderId::from("TRADER-001"))
                .strategy_id(StrategyId::from("STRATEGY-001"))
                .instrument_id(InstrumentId::from("BTC-USDT.OKX"))
                .client_order_id(ClientOrderId::from("O-001"))
                .venue_order_id(VenueOrderId::from("V-001"))
                .account_id(AccountId::from("ACCOUNT-001"))
                .build(),
        );

        match submitted {
            OrderEventAny::Rejected(_) => panic!("Should not match"),
            OrderEventAny::Canceled(_) => panic!("Should not match"),
            OrderEventAny::Expired(_) => panic!("Should not match"),
            OrderEventAny::Updated(_) => panic!("Should not match"),
            OrderEventAny::Filled(_) => panic!("Should not match"),
            _ => {}
        }

        match accepted {
            OrderEventAny::Rejected(_) => panic!("Should not match"),
            OrderEventAny::Canceled(_) => panic!("Should not match"),
            OrderEventAny::Expired(_) => panic!("Should not match"),
            OrderEventAny::Updated(_) => panic!("Should not match"),
            OrderEventAny::Filled(_) => panic!("Should not match"),
            _ => {}
        }
    }

    #[rstest]
    fn test_actionable_events_for_missing_orders_are_noops() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, false);
        let trader_id = TraderId::from("TRADER-001");
        let strategy_id = StrategyId::from("STRATEGY-001");
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let client_order_id = ClientOrderId::from("O-MISSING");
        let account_id = AccountId::from("ACCOUNT-001");
        let events = [
            OrderEventAny::Rejected(
                OrderRejectedSpec::builder()
                    .trader_id(trader_id)
                    .strategy_id(strategy_id)
                    .instrument_id(instrument_id)
                    .client_order_id(client_order_id)
                    .account_id(account_id)
                    .reason("test rejection".into())
                    .build(),
            ),
            OrderEventAny::Canceled(
                OrderCanceledSpec::builder()
                    .trader_id(trader_id)
                    .strategy_id(strategy_id)
                    .instrument_id(instrument_id)
                    .client_order_id(client_order_id)
                    .account_id(account_id)
                    .build(),
            ),
            OrderEventAny::Expired(
                OrderExpiredSpec::builder()
                    .trader_id(trader_id)
                    .strategy_id(strategy_id)
                    .instrument_id(instrument_id)
                    .client_order_id(client_order_id)
                    .account_id(account_id)
                    .build(),
            ),
            OrderEventAny::Updated(
                OrderUpdatedSpec::builder()
                    .trader_id(trader_id)
                    .strategy_id(strategy_id)
                    .instrument_id(instrument_id)
                    .client_order_id(client_order_id)
                    .quantity(Quantity::from(100_000))
                    .venue_order_id(VenueOrderId::from("V-MISSING"))
                    .account_id(account_id)
                    .build(),
            ),
        ];

        for event in events {
            assert!(manager.handle_event(&event).is_empty());
        }
    }

    fn create_test_components() -> (Rc<RefCell<dyn Clock>>, Rc<RefCell<Cache>>) {
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        (clock, cache)
    }

    fn create_test_stop_order() -> OrderAny {
        let instrument = audusd_sim();
        OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .trigger_price(Price::from("1.00050"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .build()
    }

    // Creates a `SubmitOrder` command suitable for seeding `submit_order_commands`
    // so that whether `cancel_order` removed the entry can be observed.
    fn make_submit_command(order: &OrderAny) -> SubmitOrder {
        SubmitOrder::new(
            order.trader_id(),
            None,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.init_event().clone(),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None, // correlation_id
        )
    }

    #[rstest]
    fn test_create_new_submit_order_returns_emulator_submit_action() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let order = create_test_stop_order();

        let actions = manager
            .create_new_submit_order(&order, None, None, None)
            .unwrap();

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            &actions[0],
            OrderManagerAction::PublishInitialized(OrderEventAny::Initialized(event))
                if event.client_order_id == order.client_order_id()
        ));
        assert!(matches!(
            &actions[1],
            OrderManagerAction::SubmitToEmulator(command)
                if command.client_order_id == order.client_order_id()
        ));
        assert!(
            manager
                .submit_order_commands
                .contains_key(&order.client_order_id())
        );
    }

    #[rstest]
    fn test_reset_clears_submit_commands_and_oto_targets() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let order = create_test_stop_order();
        let client_order_id = order.client_order_id();
        manager.cache_submit_order_command(make_submit_command(&order));
        manager
            .oto_target_quantities
            .insert(client_order_id, Quantity::from(40_000));

        manager.reset();

        assert!(manager.submit_order_commands.is_empty());
        assert!(manager.oto_target_quantities.is_empty());
    }

    #[rstest]
    fn test_create_new_submit_order_returns_risk_submit_action() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim().id())
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();

        let actions = manager
            .create_new_submit_order(&order, None, None, None)
            .unwrap();

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            &actions[0],
            OrderManagerAction::PublishInitialized(OrderEventAny::Initialized(event))
                if event.client_order_id == order.client_order_id()
        ));
        assert!(matches!(
            &actions[1],
            OrderManagerAction::SubmitToRisk(command)
                if command.client_order_id == order.client_order_id()
        ));
    }

    #[rstest]
    fn test_create_new_submit_order_returns_risk_action_for_none_trigger() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim().id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        let actions = manager
            .create_new_submit_order(&order, None, None, None)
            .unwrap();

        assert_eq!(actions.len(), 2);
        assert!(order.emulation_trigger().is_none());
        assert!(matches!(
            &actions[1],
            OrderManagerAction::SubmitToRisk(command)
                if command.client_order_id == order.client_order_id()
        ));
    }

    #[rstest]
    fn test_create_new_submit_order_returns_algorithm_submit_action() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let exec_algorithm_id = ExecAlgorithmId::from("ALG-001");
        let client_order_id = ClientOrderId::from("O-001");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id)
            .instrument_id(audusd_sim().id())
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .exec_algorithm_id(exec_algorithm_id)
            .exec_spawn_id(client_order_id)
            .build();

        let actions = manager
            .create_new_submit_order(&order, None, None, None)
            .unwrap();

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            &actions[1],
            OrderManagerAction::SubmitToAlgorithm {
                command,
                exec_algorithm_id: action_exec_algorithm_id,
            } if command.client_order_id == order.client_order_id()
                && *action_exec_algorithm_id == exec_algorithm_id
        ));
    }

    #[rstest]
    fn test_create_new_submit_order_does_not_republish_initialized_for_existing_order() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), true);
        let order = create_test_stop_order();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();

        let actions = manager
            .create_new_submit_order(&order, None, None, None)
            .unwrap();

        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            OrderManagerAction::SubmitToEmulator(command)
                if command.client_order_id == order.client_order_id()
        ));
    }

    #[rstest]
    fn test_cancel_order_returns_cancel_local_action() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), true);
        let order = create_test_stop_order();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        manager
            .submit_order_commands
            .insert(order.client_order_id(), make_submit_command(&order));

        let actions = manager.cancel_order(&order);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::CancelLocal(action_order)]
                if action_order.client_order_id() == order.client_order_id()
        ));
        assert!(
            !manager
                .submit_order_commands
                .contains_key(&order.client_order_id()),
            "expected cancel action path to remove the submit command",
        );
    }

    #[rstest]
    fn test_modify_order_quantity_returns_modify_local_action() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let order = create_test_stop_order();
        let new_quantity = Quantity::from(50_000);

        let actions = manager.modify_order_quantity(&order, new_quantity);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::ModifyLocalQuantity { order: action_order, quantity }]
                if action_order.client_order_id() == order.client_order_id()
                    && *quantity == new_quantity
        ));
    }

    #[rstest]
    fn test_sync_oto_quantity_reasserts_same_quantity_while_pending_update() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let client_order_id = ClientOrderId::from("O-CHILD");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim().id())
            .client_order_id(client_order_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        apply_accepted(&cache, &order, "V-CHILD");
        let pending_update = OrderEventAny::PendingUpdate(
            OrderPendingUpdateSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(AccountId::from("ACCOUNT-001"))
                .venue_order_id(VenueOrderId::from("V-CHILD"))
                .build(),
        );
        cache.borrow_mut().update_order(&pending_update).unwrap();
        manager
            .oto_target_quantities
            .insert(client_order_id, Quantity::from(40_000));
        let cached_order = cache.borrow().order_owned(&client_order_id).unwrap();

        let actions = manager.sync_oto_quantity(&cached_order, Quantity::from(100_000));

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::ModifyLocalQuantity { order, quantity }]
                if order.client_order_id() == client_order_id
                    && *quantity == Quantity::from(100_000)
        ));
        assert_eq!(
            manager.oto_target_quantities.get(&client_order_id),
            Some(&Quantity::from(100_000)),
        );
    }

    #[rstest]
    fn test_handle_event_unhandled_events_return_no_actions() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let order = create_test_stop_order();
        let event = OrderEventAny::Submitted(
            OrderSubmittedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(order.client_order_id())
                .account_id(AccountId::from("ACCOUNT-001"))
                .build(),
        );

        let actions = manager.handle_event(&event);

        assert!(actions.is_empty());
    }

    #[rstest]
    fn test_handle_order_filled_skips_missing_oco_contingent_order() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), true);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let missing_client_order_id = ClientOrderId::from("O-MISSING");
        let valid_client_order_id = ClientOrderId::from("O-CHILD");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oco)
            .linked_order_ids(vec![missing_client_order_id, valid_client_order_id])
            .build();
        let child_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(valid_client_order_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child_order.clone(), None, None, false)
            .unwrap();
        manager
            .submit_order_commands
            .insert(valid_client_order_id, make_submit_command(&child_order));
        let filled = match TestOrderEventStubs::filled(
            &order,
            &instrument,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        ) {
            OrderEventAny::Filled(event) => event,
            event => panic!("expected OrderFilled, was {event:?}"),
        };

        let actions = manager.handle_order_filled(&filled);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::CancelLocal(action_order)]
                if action_order.client_order_id() == valid_client_order_id
        ));
        assert!(
            !manager
                .submit_order_commands
                .contains_key(&valid_client_order_id)
        );
    }

    #[rstest]
    fn test_handle_event_non_active_local_manager_returns_oco_cancel_action() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let parent_id = ClientOrderId::from("O-PARENT");
        let child_id = ClientOrderId::from("O-CHILD");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oco)
            .linked_order_ids(vec![parent_id, child_id])
            .submit(true)
            .build();
        let child_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child_order.clone(), None, None, false)
            .unwrap();
        manager
            .submit_order_commands
            .insert(child_id, make_submit_command(&child_order));
        let event = TestOrderEventStubs::filled(
            &order,
            &instrument,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        );

        let actions = manager.handle_event(&event);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::CancelLocal(order)]
                if order.client_order_id() == child_id
        ));
        assert!(
            !manager.submit_order_commands.contains_key(&child_id),
            "cancel action must remove the cached submit command",
        );
    }

    #[rstest]
    fn test_handle_order_filled_skips_missing_oto_child_order() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), true);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let missing_client_order_id = ClientOrderId::from("O-MISSING");
        let valid_client_order_id = ClientOrderId::from("O-CHILD");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![missing_client_order_id, valid_client_order_id])
            .submit(true)
            .build();
        let child_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(valid_client_order_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child_order, None, None, false)
            .unwrap();
        let filled = match apply_fill(
            &cache,
            &order,
            &instrument,
            "T-PARTIAL",
            Quantity::from(40_000),
        ) {
            OrderEventAny::Filled(event) => event,
            event => panic!("expected OrderFilled, was {event:?}"),
        };

        let actions = manager.handle_order_filled(&filled);

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            &actions[0],
            OrderManagerAction::ModifyLocalQuantity { order, quantity }
                if order.client_order_id() == valid_client_order_id
                    && *quantity == Quantity::from(40_000)
        ));
        assert!(matches!(
            &actions[1],
            OrderManagerAction::SubmitToRisk(command)
                if command.client_order_id == valid_client_order_id
        ));
        assert!(
            manager
                .submit_order_commands
                .contains_key(&valid_client_order_id)
        );
    }

    #[rstest]
    fn test_handle_contingencies_skips_missing_linked_order() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let instrument = audusd_sim();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oco)
            .linked_order_ids(vec![ClientOrderId::from("O-MISSING")])
            .build();

        let actions = manager.handle_contingencies(&order);

        assert!(actions.is_empty());
        assert!(manager.submit_order_commands.is_empty());
    }

    #[rstest]
    fn test_handle_contingencies_update_skips_missing_linked_order() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let instrument = audusd_sim();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oco)
            .linked_order_ids(vec![ClientOrderId::from("O-MISSING")])
            .build();

        let actions = manager.handle_contingencies_update(&order);

        assert!(actions.is_empty());
        assert!(manager.submit_order_commands.is_empty());
    }

    #[rstest]
    fn test_oto_child_contingency_handlers_do_not_drive_parent() {
        let (clock, cache) = create_test_components();
        let mut terminal_manager = OrderManager::new(clock.clone(), cache.clone(), false);
        let mut update_manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let parent_id = ClientOrderId::from("O-PARENT");
        let child_id = ClientOrderId::from("O-CHILD");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .submit(true)
            .build();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![parent_id])
            .parent_order_id(parent_id)
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        apply_accepted(&cache, &parent, "V-PARENT");
        apply_accepted(&cache, &child, "V-CHILD");
        apply_fill(
            &cache,
            &child,
            &instrument,
            "T-CHILD",
            Quantity::from(40_000),
        );
        let cached_child = cache.borrow().order_owned(&child_id).unwrap();

        let terminal_actions = terminal_manager.handle_contingencies(&cached_child);
        let update_actions = update_manager.handle_contingencies_update(&cached_child);

        assert!(terminal_actions.is_empty());
        assert!(update_actions.is_empty());
        assert!(terminal_manager.oto_target_quantities.is_empty());
        assert!(update_manager.oto_target_quantities.is_empty());
    }

    #[rstest]
    fn test_oto_chained_child_drives_its_child_contingencies() {
        let (clock, cache) = create_test_components();
        let mut terminal_manager = OrderManager::new(clock.clone(), cache.clone(), false);
        let mut update_manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let root_id = ClientOrderId::from("O-ROOT");
        let parent_id = ClientOrderId::from("O-PARENT");
        let child_id = ClientOrderId::from("O-CHILD");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(120_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .parent_order_id(root_id)
            .submit(true)
            .build();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .parent_order_id(parent_id)
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        apply_accepted(&cache, &parent, "V-PARENT");
        apply_accepted(&cache, &child, "V-CHILD");
        let cached_parent = cache.borrow().order_owned(&parent_id).unwrap();

        let update_actions = update_manager.handle_contingencies_update(&cached_parent);
        let terminal = apply_terminal(&cache, &cached_parent, TerminalEvent::Canceled);
        let terminal_actions = terminal_manager.handle_event(&terminal);

        assert!(matches!(
            update_actions.as_slice(),
            [OrderManagerAction::ModifyLocalQuantity { order, quantity }]
                if order.client_order_id() == child_id
                    && *quantity == Quantity::from(120_000)
        ));
        assert!(matches!(
            terminal_actions.as_slice(),
            [OrderManagerAction::CancelLocal(order)]
                if order.client_order_id() == child_id
        ));
    }

    #[rstest]
    fn test_cancel_order_skips_when_pending_cancel_local() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), true);
        let order = create_test_stop_order();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        cache.borrow_mut().update_order_pending_cancel_local(&order);
        manager
            .submit_order_commands
            .insert(order.client_order_id(), make_submit_command(&order));

        manager.cancel_order(&order);

        assert!(
            manager
                .submit_order_commands
                .contains_key(&order.client_order_id()),
            "pending-cancel-local gate should short-circuit before removing the submit command",
        );
    }

    #[rstest]
    fn test_cancel_order_skips_when_passed_order_is_closed() {
        // The caller has applied a closing event to its local clone but has
        // not yet called `cache.update_order`, so the cache index still
        // reports open. The gate must short-circuit on the local state.
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), true);

        let mut order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(audusd_sim().id())
            .side(OrderSide::Buy)
            .trigger_price(Price::from("1.00050"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .submit(true)
            .build();

        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        let canceled_event =
            TestOrderEventStubs::canceled(&order, AccountId::from("ACCOUNT-001"), None);
        order.apply(canceled_event).unwrap();

        assert!(order.is_closed());
        assert!(!cache.borrow().is_order_closed(&order.client_order_id()));

        manager
            .submit_order_commands
            .insert(order.client_order_id(), make_submit_command(&order));

        manager.cancel_order(&order);

        assert!(
            manager
                .submit_order_commands
                .contains_key(&order.client_order_id()),
            "closed-order gate should short-circuit on the local state when the cache index is stale",
        );
    }

    #[rstest]
    fn test_cancel_order_skips_when_cache_index_marks_closed() {
        // The passed `OrderAny` is intentionally a stale (Submitted) clone so
        // this test would fail if `cancel_order` checked `order.is_closed()`
        // on the argument instead of `cache.is_order_closed(&id)`.
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), true);

        let mut order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(audusd_sim().id())
            .side(OrderSide::Buy)
            .trigger_price(Price::from("1.00050"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .submit(true)
            .build();

        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        let stale_order = order.clone();

        let canceled_event =
            TestOrderEventStubs::canceled(&order, AccountId::from("ACCOUNT-001"), None);
        order = cache.borrow_mut().update_order(&canceled_event).unwrap();

        assert!(cache.borrow().is_order_closed(&order.client_order_id()));

        manager.submit_order_commands.insert(
            stale_order.client_order_id(),
            make_submit_command(&stale_order),
        );

        manager.cancel_order(&stale_order);

        assert!(
            manager
                .submit_order_commands
                .contains_key(&stale_order.client_order_id()),
            "closed-order gate should short-circuit even when the passed reference is stale",
        );
    }

    #[rstest]
    fn test_handle_contingencies_update_syncs_quantity_for_ouo_sibling() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), true);
        let instrument = audusd_sim();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-CHILD"))
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(50_000))
            .build();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Ouo)
            .linked_order_ids(vec![child.client_order_id()])
            .build();

        let actions = manager.handle_contingencies_update(&parent);

        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            OrderManagerAction::ModifyLocalQuantity { order, quantity }
                if order.client_order_id() == child.client_order_id()
                    && *quantity == Quantity::from(100_000)
        ));
    }

    #[rstest]
    fn test_handle_contingencies_update_syncs_oto_child_before_parent_fill() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = audusd_sim();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-CHILD"))
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(120_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child.client_order_id()])
            .submit(true)
            .build();

        let actions = manager.handle_contingencies_update(&parent);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::ModifyLocalQuantity { order, quantity }]
                if order.client_order_id() == child.client_order_id()
                    && *quantity == Quantity::from(120_000)
        ));
        assert_eq!(
            manager.oto_target_quantities.get(&child.client_order_id()),
            Some(&Quantity::from(120_000)),
        );
    }

    #[rstest]
    fn test_handle_contingencies_update_clears_target_for_closed_oto_child() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = audusd_sim();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-CHILD"))
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        apply_terminal(&cache, &child, TerminalEvent::Canceled);
        manager
            .oto_target_quantities
            .insert(child.client_order_id(), Quantity::from(40_000));
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child.client_order_id()])
            .submit(true)
            .build();

        let actions = manager.handle_contingencies_update(&parent);

        assert!(actions.is_empty());
        assert!(manager.oto_target_quantities.is_empty());
    }

    #[rstest]
    fn test_handle_contingencies_update_cancels_ouo_sibling_filled_past_target() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-CHILD"))
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        apply_fill(
            &cache,
            &child,
            &instrument,
            "T-CHILD",
            Quantity::from(60_000),
        );
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(50_000))
            .contingency_type(ContingencyType::Ouo)
            .linked_order_ids(vec![child.client_order_id()])
            .submit(true)
            .build();

        let actions = manager.handle_contingencies_update(&parent);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::CancelLocal(order)]
                if order.client_order_id() == child.client_order_id()
        ));
    }

    #[rstest]
    fn test_handle_contingencies_update_does_not_sync_quantity_for_oco_sibling() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), true);
        let instrument = audusd_sim();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-CHILD"))
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(50_000))
            .build();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oco)
            .linked_order_ids(vec![child.client_order_id()])
            .build();

        let actions = manager.handle_contingencies_update(&parent);

        assert!(actions.is_empty());
    }

    #[derive(Debug, Clone, Copy)]
    enum TerminalEvent {
        Canceled,
        Expired,
        Rejected,
    }

    fn apply_fill(
        cache: &Rc<RefCell<Cache>>,
        order: &OrderAny,
        instrument: &InstrumentAny,
        trade_id: &str,
        last_qty: Quantity,
    ) -> OrderEventAny {
        let event = TestOrderEventStubs::filled(
            order,
            instrument,
            Some(TradeId::from(trade_id)),
            None,
            None,
            Some(last_qty),
            None,
            None,
            None,
            Some(AccountId::from("ACCOUNT-001")),
        );
        cache.borrow_mut().update_order(&event).unwrap();
        event
    }

    fn apply_accepted(cache: &Rc<RefCell<Cache>>, order: &OrderAny, venue_order_id: &str) {
        let event = TestOrderEventStubs::accepted(
            order,
            AccountId::from("ACCOUNT-001"),
            VenueOrderId::from(venue_order_id),
        );
        cache.borrow_mut().update_order(&event).unwrap();
    }

    fn apply_update(
        cache: &Rc<RefCell<Cache>>,
        order: &OrderAny,
        quantity: Quantity,
    ) -> OrderEventAny {
        let event = OrderEventAny::Updated(
            OrderUpdatedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(order.client_order_id())
                .quantity(quantity)
                .venue_order_id(VenueOrderId::from("V-CHILD"))
                .account_id(AccountId::from("ACCOUNT-001"))
                .build(),
        );
        cache.borrow_mut().update_order(&event).unwrap();
        event
    }

    fn apply_terminal(
        cache: &Rc<RefCell<Cache>>,
        order: &OrderAny,
        terminal: TerminalEvent,
    ) -> OrderEventAny {
        let event = match terminal {
            TerminalEvent::Canceled => OrderEventAny::Canceled(
                OrderCanceledSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .account_id(AccountId::from("ACCOUNT-001"))
                    .build(),
            ),
            TerminalEvent::Expired => OrderEventAny::Expired(
                OrderExpiredSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .account_id(AccountId::from("ACCOUNT-001"))
                    .build(),
            ),
            TerminalEvent::Rejected => OrderEventAny::Rejected(
                OrderRejectedSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .account_id(AccountId::from("ACCOUNT-001"))
                    .reason("test rejection".into())
                    .build(),
            ),
        };
        cache.borrow_mut().update_order(&event).unwrap();
        event
    }

    #[rstest]
    fn test_should_manage_order_selects_symmetric_ownership_classes() {
        let (clock, cache) = create_test_components();
        let active_local = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim().id())
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();
        let non_active_local = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim().id())
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        let emulator_manager = OrderManager::new(clock.clone(), cache.clone(), true);
        let strategy_manager = OrderManager::new(clock, cache, false);

        assert!(emulator_manager.should_manage_order(&active_local));
        assert!(!emulator_manager.should_manage_order(&non_active_local));
        assert!(!strategy_manager.should_manage_order(&active_local));
        assert!(strategy_manager.should_manage_order(&non_active_local));
    }

    #[rstest]
    #[case::strategy(false, true, false)]
    #[case::emulator(true, false, true)]
    fn test_handle_oto_partial_fill_resizes_once_and_submits_only_for_active_local(
        #[case] active_local: bool,
        #[case] child_submitted: bool,
        #[case] expects_submit: bool,
    ) {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), active_local);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let parent_id = ClientOrderId::from("O-PARENT");
        let child_id = ClientOrderId::from("O-CHILD");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .submit(true)
            .build();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(child_submitted)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        let event = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-PARTIAL",
            Quantity::from(40_000),
        );

        let actions = manager.handle_event(&event);
        let repeated_actions = manager.handle_event(&event);

        assert_eq!(actions.len(), if expects_submit { 2 } else { 1 });
        assert!(matches!(
            &actions[0],
            OrderManagerAction::ModifyLocalQuantity { order, quantity }
                if order.client_order_id() == child_id
                    && *quantity == Quantity::from(40_000)
        ));

        if expects_submit {
            assert!(matches!(
                &actions[1],
                OrderManagerAction::SubmitToRisk(command)
                    if command.client_order_id == child_id
            ));
        }
        assert!(repeated_actions.is_empty());

        let modify_rejected = OrderEventAny::ModifyRejected(
            OrderModifyRejectedSpec::builder()
                .trader_id(child.trader_id())
                .strategy_id(child.strategy_id())
                .instrument_id(child.instrument_id())
                .client_order_id(child_id)
                .reason("test rejection".into())
                .build(),
        );
        assert!(manager.handle_event(&modify_rejected).is_empty());
        let retry_actions = manager.handle_event(&event);
        assert!(matches!(
            retry_actions.as_slice(),
            [OrderManagerAction::ModifyLocalQuantity { order, quantity }]
                if order.client_order_id() == child_id
                    && *quantity == Quantity::from(40_000)
        ));

        let next_event = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-PARTIAL-2",
            Quantity::from(60_000),
        );
        let next_actions = manager.handle_event(&next_event);

        assert!(next_actions.is_empty());
        assert_eq!(
            manager.oto_target_quantities.get(&child_id),
            Some(&Quantity::from(100_000)),
        );
    }

    #[rstest]
    fn test_handle_oto_chained_child_releases_its_child() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), true);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let root_id = ClientOrderId::from("O-ROOT");
        let parent_id = ClientOrderId::from("O-PARENT");
        let child_id = ClientOrderId::from("O-CHILD");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .parent_order_id(root_id)
            .submit(true)
            .build();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .parent_order_id(parent_id)
            .emulation_trigger(TriggerType::BidAsk)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child, None, None, false)
            .unwrap();
        apply_accepted(&cache, &parent, "V-PARENT");
        let fill = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-PARENT",
            Quantity::from(100_000),
        );

        let actions = manager.handle_event(&fill);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::SubmitToEmulator(command)]
                if command.client_order_id == child_id
        ));
    }

    #[rstest]
    fn test_handle_oto_cancels_child_filled_to_target() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let child_id = ClientOrderId::from("O-CHILD");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .submit(true)
            .build();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        apply_accepted(&cache, &parent, "V-PARENT");
        apply_accepted(&cache, &child, "V-CHILD");
        apply_fill(
            &cache,
            &child,
            &instrument,
            "T-CHILD",
            Quantity::from(60_000),
        );
        let parent_fill = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-PARENT",
            Quantity::from(40_000),
        );

        let actions = manager.handle_event(&parent_fill);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::CancelLocal(order)]
                if order.client_order_id() == child_id
        ));
        assert!(!manager.oto_target_quantities.contains_key(&child_id));
    }

    #[rstest]
    fn test_handle_oto_clears_target_when_child_leaves_active_local() {
        let (clock, cache) = create_test_components();
        let mut terminal_manager = OrderManager::new(clock.clone(), cache.clone(), true);
        let mut update_manager = OrderManager::new(clock.clone(), cache.clone(), true);
        let mut contingency_update_manager = OrderManager::new(clock.clone(), cache.clone(), true);
        let mut fill_manager = OrderManager::new(clock, cache.clone(), true);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let parent_id = ClientOrderId::from("O-PARENT");
        let child_id = ClientOrderId::from("O-CHILD");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .submit(true)
            .build();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        let fill = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-PARTIAL",
            Quantity::from(40_000),
        );
        terminal_manager.handle_event(&fill);
        update_manager.handle_event(&fill);
        contingency_update_manager.handle_event(&fill);
        fill_manager.handle_event(&fill);

        let submitted = TestOrderEventStubs::submitted(&child, AccountId::from("ACCOUNT-001"));
        cache.borrow_mut().update_order(&submitted).unwrap();
        apply_accepted(&cache, &child, "V-CHILD");

        let cached_child = cache.borrow().order_owned(&child_id).unwrap();
        let child_fill = apply_fill(
            &cache,
            &cached_child,
            &instrument,
            "T-CHILD",
            Quantity::from(60_000),
        );
        assert!(fill_manager.handle_event(&child_fill).is_empty());
        assert!(!fill_manager.oto_target_quantities.contains_key(&child_id));

        let cached_parent = cache.borrow().order_owned(&parent_id).unwrap();
        assert!(
            contingency_update_manager
                .handle_contingencies_update(&cached_parent)
                .is_empty()
        );
        assert!(
            !contingency_update_manager
                .oto_target_quantities
                .contains_key(&child_id)
        );

        let terminal = apply_terminal(&cache, &parent, TerminalEvent::Canceled);
        assert!(terminal_manager.handle_event(&terminal).is_empty());
        assert!(
            !terminal_manager
                .oto_target_quantities
                .contains_key(&child_id)
        );

        let update = apply_update(&cache, &child, Quantity::from(80_000));
        assert!(update_manager.handle_event(&update).is_empty());
        assert!(!update_manager.oto_target_quantities.contains_key(&child_id));
    }

    #[rstest]
    fn test_handle_oto_out_of_order_updates_reassert_latest_target() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let child_id = ClientOrderId::from("O-CHILD");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .submit(true)
            .build();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        apply_accepted(&cache, &child, "V-CHILD");

        let first_fill = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-OUT-OF-ORDER-1",
            Quantity::from(40_000),
        );
        manager.handle_event(&first_fill);
        let second_fill = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-OUT-OF-ORDER-2",
            Quantity::from(60_000),
        );
        manager.handle_event(&second_fill);

        let latest_update = apply_update(&cache, &child, Quantity::from(100_000));
        let latest_actions = manager.handle_event(&latest_update);
        let stale_update = apply_update(&cache, &child, Quantity::from(40_000));
        let corrective_actions = manager.handle_event(&stale_update);

        assert!(latest_actions.is_empty());
        assert!(matches!(
            corrective_actions.as_slice(),
            [OrderManagerAction::ModifyLocalQuantity { order, quantity }]
                if order.client_order_id() == child_id
                    && *quantity == Quantity::from(100_000)
        ));
    }

    #[rstest]
    fn test_handle_oto_child_fill_waits_for_parent_target_refresh() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let parent_id = ClientOrderId::from("O-PARENT");
        let child_id = ClientOrderId::from("O-CHILD");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .submit(true)
            .build();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![parent_id])
            .parent_order_id(parent_id)
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        apply_accepted(&cache, &parent, "V-PARENT");
        apply_accepted(&cache, &child, "V-CHILD");
        let parent_fill = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-PARENT",
            Quantity::from(40_000),
        );
        manager.handle_event(&parent_fill);
        let child_fill = apply_fill(
            &cache,
            &child,
            &instrument,
            "T-CHILD",
            Quantity::from(60_000),
        );

        let actions = manager.handle_event(&child_fill);

        assert!(actions.is_empty());
        assert_eq!(
            manager.oto_target_quantities.get(&child_id),
            Some(&Quantity::from(40_000)),
        );

        let cached_parent = cache.borrow().order_owned(&parent_id).unwrap();
        let final_parent_fill = apply_fill(
            &cache,
            &cached_parent,
            &instrument,
            "T-PARENT-FINAL",
            Quantity::from(60_000),
        );
        let final_actions = manager.handle_event(&final_parent_fill);

        assert!(final_actions.is_empty());
        assert_eq!(
            manager.oto_target_quantities.get(&child_id),
            Some(&Quantity::from(100_000)),
        );
    }

    #[rstest]
    fn test_handle_oto_child_update_waits_when_filled_past_stored_target() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let child_id = ClientOrderId::from("O-CHILD");
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        apply_accepted(&cache, &child, "V-CHILD");
        apply_fill(
            &cache,
            &child,
            &instrument,
            "T-CHILD",
            Quantity::from(60_000),
        );
        manager
            .oto_target_quantities
            .insert(child_id, Quantity::from(40_000));
        let update = apply_update(&cache, &child, Quantity::from(90_000));

        let actions = manager.handle_event(&update);

        assert!(actions.is_empty());
        assert_eq!(
            manager.oto_target_quantities.get(&child_id),
            Some(&Quantity::from(40_000)),
        );
    }

    #[rstest]
    fn test_handle_oto_parent_update_preserves_partial_fill_target() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let child_id = ClientOrderId::from("O-CHILD");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-PARENT"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .submit(true)
            .build();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child, None, None, false)
            .unwrap();
        let fill = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-PARTIAL",
            Quantity::from(40_000),
        );
        manager.handle_event(&fill);
        let parent_update = apply_update(&cache, &parent, Quantity::from(120_000));

        let actions = manager.handle_event(&parent_update);

        assert!(actions.is_empty());
        assert_eq!(
            manager.oto_target_quantities.get(&child_id),
            Some(&Quantity::from(40_000)),
        );

        let terminal = apply_terminal(&cache, &parent, TerminalEvent::Canceled);
        assert!(manager.handle_event(&terminal).is_empty());
        assert_eq!(
            manager.oto_target_quantities.get(&child_id),
            Some(&Quantity::from(40_000)),
        );
    }

    #[rstest]
    fn test_handle_oto_fill_uses_completed_and_active_spawn_totals() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let spawn_id = ClientOrderId::from("O-SPAWN");
        let first_id = ClientOrderId::from("O-SPAWN-1");
        let second_id = ClientOrderId::from("O-SPAWN-2");
        let child_id = ClientOrderId::from("O-CHILD");
        let exec_algorithm_id = ExecAlgorithmId::from("TWAP");
        let first = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(first_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(60_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .exec_algorithm_id(exec_algorithm_id)
            .exec_spawn_id(spawn_id)
            .submit(true)
            .build();
        let second = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(second_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(40_000))
            .contingency_type(ContingencyType::Oto)
            .linked_order_ids(vec![child_id])
            .exec_algorithm_id(exec_algorithm_id)
            .exec_spawn_id(spawn_id)
            .submit(true)
            .build();
        let child = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();

        for order in [&first, &second, &child] {
            cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
        }
        apply_accepted(&cache, &first, "V-SPAWN-1");
        apply_accepted(&cache, &second, "V-SPAWN-2");
        apply_fill(
            &cache,
            &first,
            &instrument,
            "T-SPAWN-1",
            Quantity::from(60_000),
        );
        let event = apply_fill(
            &cache,
            &second,
            &instrument,
            "T-SPAWN-2",
            Quantity::from(10_000),
        );

        let actions = manager.handle_event(&event);

        assert!(matches!(
            actions.first(),
            Some(OrderManagerAction::ModifyLocalQuantity { order, quantity })
                if order.client_order_id() == child_id
                    && *quantity == Quantity::from(70_000)
        ));

        let update = apply_update(&cache, &second, Quantity::from(50_000));
        assert!(manager.handle_event(&update).is_empty());
        assert_eq!(
            manager.oto_target_quantities.get(&child_id),
            Some(&Quantity::from(70_000)),
        );

        let cached_second = cache.borrow().order_owned(&second_id).unwrap();
        assert!(manager.handle_contingencies(&cached_second).is_empty());
    }

    #[rstest]
    fn test_handle_ouo_partial_fill_resizes_open_sibling() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let parent_id = ClientOrderId::from("O-PARENT");
        let sibling_id = ClientOrderId::from("O-SIBLING");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Ouo)
            .linked_order_ids(vec![parent_id, sibling_id])
            .submit(true)
            .build();
        let sibling = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(sibling_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(sibling, None, None, false)
            .unwrap();
        let event = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-OUO",
            Quantity::from(25_000),
        );

        let actions = manager.handle_event(&event);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::ModifyLocalQuantity { order, quantity }]
                if order.client_order_id() == sibling_id
                    && *quantity == Quantity::from(75_000)
        ));
    }

    #[rstest]
    fn test_handle_ouo_partial_fill_cancels_sibling_filled_past_target() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let parent_id = ClientOrderId::from("O-PARENT");
        let sibling_id = ClientOrderId::from("O-SIBLING");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Ouo)
            .linked_order_ids(vec![parent_id, sibling_id])
            .submit(true)
            .build();
        let sibling = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(sibling_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(sibling.clone(), None, None, false)
            .unwrap();
        apply_accepted(&cache, &parent, "V-PARENT");
        apply_accepted(&cache, &sibling, "V-SIBLING");
        apply_fill(
            &cache,
            &sibling,
            &instrument,
            "T-SIBLING",
            Quantity::from(30_000),
        );
        let parent_fill = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-PARENT",
            Quantity::from(90_000),
        );

        let actions = manager.handle_event(&parent_fill);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::CancelLocal(order)]
                if order.client_order_id() == sibling_id
        ));
    }

    #[rstest]
    #[case(TerminalEvent::Canceled)]
    #[case(TerminalEvent::Expired)]
    #[case(TerminalEvent::Rejected)]
    fn test_terminal_non_contingent_event_clears_oto_target(#[case] terminal: TerminalEvent) {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim().id())
            .client_order_id(ClientOrderId::from("O-001"))
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        if matches!(terminal, TerminalEvent::Expired) {
            apply_accepted(&cache, &order, "V-001");
        }
        manager
            .oto_target_quantities
            .insert(order.client_order_id(), Quantity::from(40_000));
        let event = apply_terminal(&cache, &order, terminal);

        let actions = manager.handle_event(&event);

        assert!(actions.is_empty());
        assert!(manager.oto_target_quantities.is_empty());
    }

    #[rstest]
    #[case(ContingencyType::Oto, TerminalEvent::Rejected)]
    #[case(ContingencyType::Oco, TerminalEvent::Canceled)]
    #[case(ContingencyType::Oco, TerminalEvent::Expired)]
    #[case(ContingencyType::Oco, TerminalEvent::Rejected)]
    #[case(ContingencyType::Ouo, TerminalEvent::Canceled)]
    fn test_terminal_contingent_event_cancels_open_sibling(
        #[case] contingency_type: ContingencyType,
        #[case] terminal: TerminalEvent,
    ) {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let parent_id = ClientOrderId::from("O-PARENT");
        let sibling_id = ClientOrderId::from("O-SIBLING");
        let linked_order_ids = if contingency_type == ContingencyType::Oto {
            vec![sibling_id]
        } else {
            vec![parent_id, sibling_id]
        };
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim().id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(contingency_type)
            .linked_order_ids(linked_order_ids)
            .submit(true)
            .build();
        let sibling = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim().id())
            .client_order_id(sibling_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(sibling, None, None, false)
            .unwrap();

        if matches!(terminal, TerminalEvent::Expired) {
            apply_accepted(&cache, &parent, "V-PARENT");
        }
        let event = apply_terminal(&cache, &parent, terminal);

        let actions = manager.handle_event(&event);

        assert!(matches!(
            actions.as_slice(),
            [OrderManagerAction::CancelLocal(order)]
                if order.client_order_id() == sibling_id
        ));
    }

    #[rstest]
    #[case(ContingencyType::Oto)]
    #[case(ContingencyType::Ouo)]
    fn test_strategy_manager_excludes_active_local_and_closed_siblings(
        #[case] contingency_type: ContingencyType,
    ) {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache.clone(), false);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let parent_id = ClientOrderId::from("O-PARENT");
        let active_local_id = ClientOrderId::from("O-ACTIVE-LOCAL");
        let closed_id = ClientOrderId::from("O-CLOSED");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .contingency_type(contingency_type)
            .linked_order_ids(vec![active_local_id, closed_id])
            .submit(true)
            .build();
        let active_local = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(active_local_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .build();
        let closed = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(closed_id)
            .side(OrderSide::Sell)
            .price(Price::from("1.00100"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(parent.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(active_local, None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(closed.clone(), None, None, false)
            .unwrap();
        apply_terminal(&cache, &closed, TerminalEvent::Canceled);
        let event = apply_fill(
            &cache,
            &parent,
            &instrument,
            "T-EXCLUSION",
            Quantity::from(100_000),
        );

        let actions = manager.handle_event(&event);

        assert!(actions.is_empty());
    }
}
