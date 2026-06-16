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
    enums::{ContingencyType, TriggerType},
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

    /// Resets the order manager by clearing all cached commands.
    pub fn reset(&mut self) {
        self.submit_order_commands.clear();
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

        if matches!(order.emulation_trigger(), Some(trigger) if trigger != TriggerType::NoTrigger) {
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
        self.active_local && order.is_active_local()
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
            OrderEventAny::Filled(event) => self.handle_order_filled(*event),
            _ => Vec::new(),
        }
    }

    /// Handles an order rejected event and manages any contingent orders.
    pub fn handle_order_rejected(&mut self, rejected: OrderRejected) -> Vec<OrderManagerAction> {
        let cloned_order = self
            .cache
            .borrow()
            .order(&rejected.client_order_id)
            .map(|o| o.clone());

        if let Some(order) = cloned_order {
            if order.contingency_type() != Some(ContingencyType::NoContingency) {
                return self.handle_contingencies(&order);
            }
        } else {
            log::error!(
                "Cannot handle `OrderRejected`: order for client_order_id: {} not found, {}",
                rejected.client_order_id,
                rejected
            );
        }

        Vec::new()
    }

    pub fn handle_order_canceled(&mut self, canceled: OrderCanceled) -> Vec<OrderManagerAction> {
        let cloned_order = self
            .cache
            .borrow()
            .order(&canceled.client_order_id)
            .map(|o| o.clone());

        if let Some(order) = cloned_order {
            if order.contingency_type() != Some(ContingencyType::NoContingency) {
                return self.handle_contingencies(&order);
            }
        } else {
            log::error!(
                "Cannot handle `OrderCanceled`: order for client_order_id: {} not found, {}",
                canceled.client_order_id,
                canceled
            );
        }

        Vec::new()
    }

    pub fn handle_order_expired(&mut self, expired: OrderExpired) -> Vec<OrderManagerAction> {
        let cloned_order = self
            .cache
            .borrow()
            .order(&expired.client_order_id)
            .map(|o| o.clone());
        if let Some(order) = cloned_order {
            if order.contingency_type() != Some(ContingencyType::NoContingency) {
                return self.handle_contingencies(&order);
            }
        } else {
            log::error!(
                "Cannot handle `OrderExpired`: order for client_order_id: {} not found, {}",
                expired.client_order_id,
                expired
            );
        }

        Vec::new()
    }

    pub fn handle_order_updated(&mut self, updated: OrderUpdated) -> Vec<OrderManagerAction> {
        let cloned_order = self
            .cache
            .borrow()
            .order(&updated.client_order_id)
            .map(|o| o.clone());
        if let Some(order) = cloned_order {
            if order.contingency_type() != Some(ContingencyType::NoContingency) {
                return self.handle_contingencies_update(&order);
            }
        } else {
            log::error!(
                "Cannot handle `OrderUpdated`: order for client_order_id: {} not found, {}",
                updated.client_order_id,
                updated
            );
        }

        Vec::new()
    }

    pub fn handle_order_filled(&mut self, filled: OrderFilled) -> Vec<OrderManagerAction> {
        let order = if let Some(order) = self
            .cache
            .borrow()
            .order(&filled.client_order_id)
            .map(|o| o.clone())
        {
            order
        } else {
            log::error!(
                "Cannot handle `OrderFilled`: order for client_order_id: {} not found, {}",
                filled.client_order_id,
                filled
            );
            return Vec::new();
        };

        let mut actions = Vec::new();

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
                            .exec_spawn_total_filled_qty(&spawn_id, true)
                        {
                            qty
                        } else {
                            log::error!("Failed to get spawn filled quantity for {spawn_id}");
                            return actions;
                        }
                    }
                    None => order.filled_qty(),
                };

                let linked_orders = if let Some(orders) = order.linked_order_ids() {
                    orders
                } else {
                    log::error!("No linked orders found for OTO order");
                    return actions;
                };

                for client_order_id in linked_orders {
                    let mut child_order = if let Some(order) = self
                        .cache
                        .borrow()
                        .order(client_order_id)
                        .map(|o| o.clone())
                    {
                        order
                    } else {
                        log::error!(
                            "Cannot find OTO child order for client_order_id: {client_order_id}"
                        );
                        continue;
                    };

                    if !self.should_manage_order(&child_order) {
                        continue;
                    }

                    if child_order.position_id().is_none() {
                        child_order.set_position_id(position_id);
                    }

                    if parent_filled_qty != child_order.leaves_qty() {
                        actions.extend(self.modify_order_quantity(&child_order, parent_filled_qty));
                    }

                    if !self
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
                let linked_orders = if let Some(orders) = order.linked_order_ids() {
                    orders
                } else {
                    log::error!("No linked orders found for OCO order");
                    return actions;
                };

                for client_order_id in linked_orders {
                    let contingent_order = match self
                        .cache
                        .borrow()
                        .order(client_order_id)
                        .map(|o| o.clone())
                    {
                        Some(contingent_order) => contingent_order,
                        None => {
                            log::error!(
                                "Cannot find OCO contingent order for client_order_id: {client_order_id}"
                            );
                            continue;
                        }
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
                        .exec_spawn_total_filled_qty(&exec_spawn_id, true),
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

        let linked_orders = if let Some(orders) = order.linked_order_ids() {
            orders
        } else {
            log::error!("No linked orders found");
            return actions;
        };

        for client_order_id in linked_orders {
            let contingent_order = if let Some(order) = self
                .cache
                .borrow()
                .order(client_order_id)
                .map(|o| o.clone())
            {
                order
            } else {
                log::error!("Cannot find contingent order for client_order_id: {client_order_id}");
                continue;
            };

            if !self.should_manage_order(&contingent_order)
                || client_order_id == &order.client_order_id()
            {
                continue;
            }

            if contingent_order.is_closed() {
                self.submit_order_commands.remove(&order.client_order_id());
                continue;
            }

            match order.contingency_type() {
                Some(ContingencyType::Oto) => {
                    if order.is_closed()
                        && filled_qty.raw == 0
                        && (order.exec_spawn_id().is_none() || !is_spawn_active)
                    {
                        actions.extend(self.cancel_order(&contingent_order));
                    } else if filled_qty.raw > 0 && filled_qty != contingent_order.quantity() {
                        actions.extend(self.modify_order_quantity(&contingent_order, filled_qty));
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

        let linked_orders = if let Some(orders) = order.linked_order_ids() {
            orders
        } else {
            log::error!("No linked orders found for contingent order");
            return actions;
        };

        for client_order_id in linked_orders {
            let contingent_order = match self
                .cache
                .borrow()
                .order(client_order_id)
                .map(|o| o.clone())
            {
                Some(contingent_order) => contingent_order,
                None => {
                    log::error!(
                        "Cannot find OCO contingent order for client_order_id: {client_order_id}"
                    );
                    continue;
                }
            };

            if !self.should_manage_order(&contingent_order)
                || client_order_id == &order.client_order_id()
                || contingent_order.is_closed()
            {
                continue;
            }

            if let Some(contingency_type) = order.contingency_type()
                && matches!(
                    contingency_type,
                    ContingencyType::Oto | ContingencyType::Oco
                )
                && quantity != contingent_order.quantity()
            {
                actions.extend(self.modify_order_quantity(&contingent_order, quantity));
            }
        }

        actions
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
        events::{OrderAccepted, OrderSubmitted},
        identifiers::{
            AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, StrategyId, TraderId,
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
        let submitted = OrderEventAny::Submitted(OrderSubmitted {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            instrument_id: InstrumentId::from("BTC-USDT.OKX"),
            client_order_id: ClientOrderId::from("O-001"),
            account_id: AccountId::from("ACCOUNT-001"),
            event_id: UUID4::new(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            causation_id: None,
        });
        let accepted = OrderEventAny::Accepted(OrderAccepted {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            instrument_id: InstrumentId::from("BTC-USDT.OKX"),
            client_order_id: ClientOrderId::from("O-001"),
            venue_order_id: VenueOrderId::from("V-001"),
            account_id: AccountId::from("ACCOUNT-001"),
            event_id: UUID4::new(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: false,
            causation_id: None,
        });

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
    fn test_create_new_submit_order_returns_risk_submit_action() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim().id())
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::NoTrigger)
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
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim().id())
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::NoTrigger)
            .exec_algorithm_id(exec_algorithm_id)
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
    fn test_handle_event_unhandled_events_return_no_actions() {
        let (clock, cache) = create_test_components();
        let mut manager = OrderManager::new(clock, cache, true);
        let order = create_test_stop_order();
        let event = OrderEventAny::Submitted(OrderSubmitted {
            trader_id: order.trader_id(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id: order.client_order_id(),
            account_id: AccountId::from("ACCOUNT-001"),
            event_id: UUID4::new(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            causation_id: None,
        });

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

        let actions = manager.handle_order_filled(filled);

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
    fn test_handle_event_inactive_manager_returns_no_local_actions() {
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
            .build();
        let child_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(child_id)
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

        assert!(actions.is_empty());
        assert!(
            manager.submit_order_commands.contains_key(&child_id),
            "inactive manager must not start local contingency actions",
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
            .build();
        let child_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(valid_client_order_id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::NoTrigger)
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(child_order, None, None, false)
            .unwrap();
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

        let actions = manager.handle_order_filled(filled);

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            &actions[0],
            OrderManagerAction::ModifyLocalQuantity { order, quantity }
                if order.client_order_id() == valid_client_order_id
                    && *quantity == Quantity::zero(0)
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
}
