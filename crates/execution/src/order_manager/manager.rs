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

use std::{
    cell::RefCell,
    fmt::{Debug, Display},
    rc::Rc,
};

use ahash::AHashMap;
use nautilus_common::{cache::Cache, clock::Clock, messages::execution::SubmitOrder};
use nautilus_core::UUID4;
use nautilus_model::{
    enums::{ContingencyType, OrderStatus},
    events::{
        OrderCanceled, OrderEvent, OrderEventAny, OrderExpired, OrderFilled, OrderRejected,
        OrderUpdated,
    },
    identifiers::{ClientId, ClientOrderId, PositionId},
    instruments::Instrument,
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

    /// Caches a submit command and returns initialization and routing actions.
    ///
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
            let event = OrderEventAny::Initialized(order.init_event().clone());
            actions.push(OrderManagerAction::PublishInitialized(event));
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

        self.cache_submit_order_command(submit.clone());

        if order.emulation_trigger().is_some() {
            actions.push(OrderManagerAction::SubmitToEmulator(submit));
        } else {
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
            OrderEventAny::Denied(event) => {
                self.submit_order_commands.remove(&event.client_order_id);
                self.oto_target_quantities.remove(&event.client_order_id);
                let order = self.cache.borrow().order_owned(&event.client_order_id);

                match order {
                    Some(order)
                        if order.previous_status() == Some(OrderStatus::Released)
                            && order.contingency_type().is_some() =>
                    {
                        self.handle_contingencies(&order)
                    }
                    _ => Vec::new(),
                }
            }
            OrderEventAny::Rejected(event) => self.handle_order_rejected(*event),
            OrderEventAny::Canceled(event) => self.handle_order_canceled(*event),
            OrderEventAny::Expired(event) => self.handle_order_expired(*event),
            OrderEventAny::Updated(event) => self.handle_order_updated(*event),
            OrderEventAny::ModifyDenied(_) | OrderEventAny::ModifyRejected(_) => {
                let client_order_id = event.client_order_id();
                if !self
                    .cache
                    .borrow()
                    .order(&client_order_id)
                    .is_some_and(|order| order.has_pending_update_command())
                {
                    self.oto_target_quantities.remove(&client_order_id);
                }

                Vec::new()
            }
            OrderEventAny::Filled(event) => self.handle_order_filled(event),
            _ => Vec::new(),
        }
    }

    /// Handles an order rejected event and manages any contingent orders.
    pub fn handle_order_rejected(&mut self, rejected: OrderRejected) -> Vec<OrderManagerAction> {
        self.handle_terminal_event(&rejected)
    }

    /// Handles an order canceled event and manages any contingent orders.
    pub fn handle_order_canceled(&mut self, canceled: OrderCanceled) -> Vec<OrderManagerAction> {
        self.handle_terminal_event(&canceled)
    }

    /// Handles an order expired event and manages any contingent orders.
    pub fn handle_order_expired(&mut self, expired: OrderExpired) -> Vec<OrderManagerAction> {
        self.handle_terminal_event(&expired)
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

    /// Returns contingency actions for a fill using the cached order state.
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
                let cached_position_id = self
                    .cache
                    .borrow()
                    .position_id(&order.client_order_id())
                    .copied();

                if let (Some(fill_position_id), Some(cached_position_id)) =
                    (filled.position_id, cached_position_id)
                    && fill_position_id != cached_position_id
                {
                    log::error!(
                        "Cannot handle OTO fill: event position {fill_position_id} does not match cached position {cached_position_id}"
                    );
                    return actions;
                }

                let position_id = filled.position_id.or(cached_position_id);
                let client_id = self
                    .cache
                    .borrow()
                    .client_id(&order.client_order_id())
                    .copied();

                let (parent_filled_qty, is_spawn_active) = match order.exec_spawn_id() {
                    Some(spawn_id) => {
                        let cache = self.cache.borrow();
                        let Some(filled_qty) = cache.exec_spawn_total_filled_qty(&spawn_id, false)
                        else {
                            log::error!("Failed to get spawn filled quantity for {spawn_id}");
                            return actions;
                        };
                        let is_spawn_active = cache
                            .exec_spawn_total_leaves_qty(&spawn_id, true)
                            .is_some_and(|leaves_qty| leaves_qty.is_positive());
                        (filled_qty, is_spawn_active)
                    }
                    None => (order.filled_qty(), false),
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

                    let Some(child_quantity) = self.position_adjusted_oto_quantity(
                        &order,
                        &child_order,
                        parent_filled_qty,
                        position_id,
                    ) else {
                        continue;
                    };

                    if child_quantity.is_zero() {
                        self.oto_target_quantities.remove(client_order_id);

                        if order.is_closed() && !is_spawn_active {
                            actions.extend(self.cancel_order(&child_order));
                        }
                        continue;
                    }

                    actions.extend(self.sync_oto_quantity(&child_order, child_quantity));

                    if child_quantity.is_positive()
                        && self.active_local
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

    /// Returns cancellation and quantity-update actions for linked orders.
    pub fn handle_contingencies(&mut self, order: &OrderAny) -> Vec<OrderManagerAction> {
        let mut actions = Vec::new();

        let (filled_qty, leaves_qty, is_spawn_active) =
            if let Some(exec_spawn_id) = order.exec_spawn_id() {
                let cache = self.cache.borrow();
                let Some(filled) = cache.exec_spawn_total_filled_qty(&exec_spawn_id, false) else {
                    log::error!("Failed to get spawn filled quantity for {exec_spawn_id}");
                    return actions;
                };
                let leaves = cache
                    .exec_spawn_total_leaves_qty(&exec_spawn_id, true)
                    .unwrap_or_else(|| Quantity::zero(filled.precision));

                (filled, leaves, leaves.is_positive())
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
                    if filled_qty.is_zero()
                        && order.is_closed()
                        && (order.exec_spawn_id().is_none() || !is_spawn_active)
                    {
                        self.oto_target_quantities.remove(client_order_id);
                        actions.extend(self.cancel_order(&contingent_order));
                        continue;
                    }

                    let position_id = self.oto_parent_position_id(order);

                    let Some(child_quantity) = self.position_adjusted_oto_quantity(
                        order,
                        &contingent_order,
                        filled_qty,
                        position_id,
                    ) else {
                        continue;
                    };

                    if child_quantity.is_zero() {
                        self.oto_target_quantities.remove(client_order_id);

                        if order.is_closed()
                            && (order.exec_spawn_id().is_none() || !is_spawn_active)
                        {
                            actions.extend(self.cancel_order(&contingent_order));
                        }
                    } else {
                        actions.extend(self.sync_oto_quantity(&contingent_order, child_quantity));
                    }
                }
                Some(ContingencyType::Oco)
                    if order.is_closed()
                        && (order.exec_spawn_id().is_none() || !is_spawn_active) =>
                {
                    actions.extend(self.cancel_order(&contingent_order));
                }
                Some(ContingencyType::Ouo) => {
                    if (leaves_qty.is_zero() && order.exec_spawn_id().is_some())
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

    /// Returns quantity-sync actions for linked OTO and OUO orders.
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

        if quantity.is_zero() {
            return actions;
        }

        let leaves_qty = self.ouo_leaves_qty(order);

        let oto_filled_qty = if contingency_type == Some(ContingencyType::Oto) {
            Some(match order.exec_spawn_id() {
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
            })
        } else {
            None
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
                    let child_quantity =
                        if let Some(filled_qty) = oto_filled_qty.filter(Quantity::is_positive) {
                            let position_id = self.oto_parent_position_id(order);

                            let Some(quantity) = self.position_adjusted_oto_quantity(
                                order,
                                &contingent_order,
                                filled_qty,
                                position_id,
                            ) else {
                                continue;
                            };
                            quantity
                        } else {
                            quantity
                        };

                    actions.extend(self.sync_oto_quantity(&contingent_order, child_quantity));
                }
                // Siblings already tracking the same remaining quantity need no action, which also
                // ends the reciprocal update a resize triggers.
                Some(ContingencyType::Ouo) if leaves_qty == contingent_order.leaves_qty() => {}
                Some(ContingencyType::Ouo) if contingent_order.filled_qty() >= quantity => {
                    actions.extend(self.cancel_order(&contingent_order));
                }
                Some(ContingencyType::Ouo) => {
                    // The modify sets the sibling's total quantity, so add back its own fills
                    let quantity = contingent_order.filled_qty() + leaves_qty;
                    actions.extend(self.modify_order_quantity(&contingent_order, quantity));
                }
                _ => {}
            }
        }

        actions
    }

    fn handle_terminal_event(
        &mut self,
        event: &(impl OrderEvent + Display),
    ) -> Vec<OrderManagerAction> {
        let client_order_id = event.client_order_id();
        self.oto_target_quantities.remove(&client_order_id);

        let Some(order) = self.cache.borrow().order_owned(&client_order_id) else {
            log::error!(
                "Cannot handle `{}`: order for client_order_id: {client_order_id} not found, {event}",
                event.type_name(),
            );
            return Vec::new();
        };

        if order.contingency_type().is_some() {
            self.handle_contingencies(&order)
        } else {
            Vec::new()
        }
    }

    // OUO siblings track the remaining quantity, as when a leg fills
    fn ouo_leaves_qty(&self, order: &OrderAny) -> Quantity {
        match order.exec_spawn_id() {
            Some(exec_spawn_id) => self
                .cache
                .borrow()
                .exec_spawn_total_leaves_qty(&exec_spawn_id, true)
                .unwrap_or_else(|| Quantity::zero(order.quantity().precision)),
            None => order.leaves_qty(),
        }
    }

    fn oto_parent_position_id(&self, order: &OrderAny) -> Option<PositionId> {
        if let Some(position_id) = order.position_id() {
            return Some(position_id);
        }

        let cache = self.cache.borrow();

        cache
            .position_id(&order.client_order_id())
            .copied()
            .or_else(|| {
                order.exec_spawn_id().and_then(|spawn_id| {
                    cache
                        .orders_for_exec_spawn(&spawn_id)
                        .into_iter()
                        .find_map(|spawn_order| spawn_order.position_id())
                })
            })
    }

    fn position_adjusted_oto_quantity(
        &self,
        parent_order: &OrderAny,
        child_order: &OrderAny,
        filled_qty: Quantity,
        position_id: Option<PositionId>,
    ) -> Option<Quantity> {
        let Some(position_id) = position_id else {
            return Some(filled_qty);
        };

        let cache = self.cache.borrow();
        let Some(parent_instrument) = cache.instrument(&parent_order.instrument_id()) else {
            log::error!(
                "Cannot size OTO orders: instrument {} not found in cache",
                parent_order.instrument_id()
            );
            return None;
        };

        let Some(child_instrument) = cache.instrument(&child_order.instrument_id()) else {
            log::error!(
                "Cannot size OTO order {}: instrument {} not found in cache",
                child_order.client_order_id(),
                child_order.instrument_id()
            );
            return None;
        };

        let increment = child_instrument.size_increment();
        if increment.is_zero() {
            log::error!(
                "Cannot size OTO orders: size increment is zero for {}",
                child_order.instrument_id()
            );
            return None;
        }

        let capped = if parent_instrument.is_spread() || !child_order.is_reduce_only() {
            filled_qty
        } else {
            let Some(position) = cache.position(&position_id) else {
                log::error!("Cannot size OTO orders: position {position_id} not found in cache");
                return None;
            };

            let child_filled_qty = child_order.filled_qty();
            let precision = child_filled_qty.precision.max(position.quantity.precision);

            let (Some(child_filled_qty), Some(position_qty)) = (
                child_filled_qty.checked_rescale(precision),
                position.quantity.checked_rescale(precision),
            ) else {
                log::error!("Cannot size OTO orders: position quantity scale alignment overflow");
                return None;
            };

            let position_cap = child_filled_qty.saturating_add(position_qty);
            filled_qty.min(position_cap)
        };

        let Some(target) = capped.checked_floor_to_increment(increment) else {
            log::error!("Cannot size OTO orders: quantity scale alignment overflow");
            return None;
        };

        if child_instrument
            .min_quantity()
            .is_some_and(|min_quantity| target < min_quantity)
        {
            return Some(Quantity::zero(child_instrument.size_precision()));
        }

        Some(target)
    }

    fn sync_oto_quantity(
        &mut self,
        order: &OrderAny,
        quantity: Quantity,
    ) -> Vec<OrderManagerAction> {
        let client_order_id = order.client_order_id();

        if quantity.is_zero() {
            self.oto_target_quantities.remove(&client_order_id);
            return Vec::new();
        }

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
