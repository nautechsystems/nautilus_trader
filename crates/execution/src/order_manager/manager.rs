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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, collections::HashMap, fmt::Debug, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, SEND},
    messages::execution::{SubmitOrder, TradingCommand},
    msgbus,
};
use nautilus_core::UUID4;
use nautilus_model::{
    enums::{ContingencyType, TriggerType},
    events::{
        OrderCanceled, OrderEventAny, OrderExpired, OrderFilled, OrderRejected, OrderUpdated,
    },
    identifiers::{ClientId, ClientOrderId, ExecAlgorithmId, PositionId},
    orders::{Order, OrderAny},
    types::Quantity,
};

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
    // submit_order_handler: Option<SubmitOrderHandlerAny>,
    // cancel_order_handler: Option<CancelOrderHandlerAny>,
    // modify_order_handler: Option<ModifyOrderHandlerAny>,
    submit_order_commands: HashMap<ClientOrderId, SubmitOrder>,
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
        // submit_order_handler: Option<SubmitOrderHandlerAny>,
        // cancel_order_handler: Option<CancelOrderHandlerAny>,
        // modify_order_handler: Option<ModifyOrderHandlerAny>,
    ) -> Self {
        Self {
            clock,
            cache,
            active_local,
            // submit_order_handler,
            // cancel_order_handler,
            // modify_order_handler,
            submit_order_commands: HashMap::new(),
        }
    }

    // pub fn set_submit_order_handler(&mut self, handler: SubmitOrderHandlerAny) {
    //     self.submit_order_handler = Some(handler);
    // }
    //
    // pub fn set_cancel_order_handler(&mut self, handler: CancelOrderHandlerAny) {
    //     self.cancel_order_handler = Some(handler);
    // }
    //
    // pub fn set_modify_order_handler(&mut self, handler: ModifyOrderHandlerAny) {
    //     self.modify_order_handler = Some(handler);
    // }

    #[must_use]
    /// Returns a copy of all cached submit order commands.
    pub fn get_submit_order_commands(&self) -> HashMap<ClientOrderId, SubmitOrder> {
        self.submit_order_commands.clone()
    }

    /// Caches a submit order command for later processing.
    pub fn cache_submit_order_command(&mut self, command: SubmitOrder) {
        self.submit_order_commands
            .insert(command.order.client_order_id(), command);
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
    pub fn cancel_order(&mut self, order: &OrderAny) {
        if self
            .cache
            .borrow()
            .is_order_pending_cancel_local(&order.client_order_id())
        {
            return;
        }

        if order.is_closed() {
            log::warn!("Cannot cancel order: already closed");
            return;
        }

        self.submit_order_commands.remove(&order.client_order_id());

        // if let Some(handler) = &self.cancel_order_handler {
        //     handler.handle_cancel_order(order);
        // }
    }

    /// Modifies the quantity of an existing order.
    pub const fn modify_order_quantity(&mut self, order: &mut OrderAny, new_quantity: Quantity) {
        // if let Some(handler) = &self.modify_order_handler {
        //     handler.handle_modify_order(order, new_quantity);
        // }
    }

    /// # Errors
    ///
    /// Returns an error if creating a new submit order fails.
    pub fn create_new_submit_order(
        &mut self,
        order: &OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        let client_id = client_id.ok_or_else(|| anyhow::anyhow!("Client ID is required"))?;
        let venue_order_id = order
            .venue_order_id()
            .ok_or_else(|| anyhow::anyhow!("Venue order ID is required"))?;

        let submit = SubmitOrder::new(
            order.trader_id(),
            client_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            order.clone(),
            order.exec_algorithm_id(),
            position_id,
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
        )?;

        if order.emulation_trigger() == Some(TriggerType::NoTrigger) {
            self.cache_submit_order_command(submit.clone());

            match order.exec_algorithm_id() {
                Some(exec_algorithm_id) => {
                    self.send_algo_command(submit, exec_algorithm_id);
                }
                None => self.send_risk_command(TradingCommand::SubmitOrder(submit)),
            }
        } // else if let Some(handler) = &self.submit_order_handler {
        //     handler.handle_submit_order(submit);
        // }

        Ok(())
    }

    #[must_use]
    /// Returns true if the order manager should manage the given order.
    pub fn should_manage_order(&self, order: &OrderAny) -> bool {
        self.active_local && order.is_active_local()
    }

    // Event Handlers
    /// Handles an order event by routing it to the appropriate handler method.
    pub fn handle_event(&mut self, event: OrderEventAny) {
        match event {
            OrderEventAny::Rejected(event) => self.handle_order_rejected(event),
            OrderEventAny::Canceled(event) => self.handle_order_canceled(event),
            OrderEventAny::Expired(event) => self.handle_order_expired(event),
            OrderEventAny::Updated(event) => self.handle_order_updated(event),
            OrderEventAny::Filled(event) => self.handle_order_filled(event),
            _ => self.handle_position_event(event),
        }
    }

    /// Handles an order rejected event and manages any contingent orders.
    pub fn handle_order_rejected(&mut self, rejected: OrderRejected) {
        let cloned_order = self
            .cache
            .borrow()
            .order(&rejected.client_order_id)
            .cloned();
        if let Some(order) = cloned_order {
            if order.contingency_type() != Some(ContingencyType::NoContingency) {
                self.handle_contingencies(order);
            }
        } else {
            log::error!(
                "Cannot handle `OrderRejected`: order for client_order_id: {} not found, {}",
                rejected.client_order_id,
                rejected
            );
        }
    }

    pub fn handle_order_canceled(&mut self, canceled: OrderCanceled) {
        let cloned_order = self
            .cache
            .borrow()
            .order(&canceled.client_order_id)
            .cloned();
        if let Some(order) = cloned_order {
            if order.contingency_type() != Some(ContingencyType::NoContingency) {
                self.handle_contingencies(order);
            }
        } else {
            log::error!(
                "Cannot handle `OrderCanceled`: order for client_order_id: {} not found, {}",
                canceled.client_order_id,
                canceled
            );
        }
    }

    pub fn handle_order_expired(&mut self, expired: OrderExpired) {
        let cloned_order = self.cache.borrow().order(&expired.client_order_id).cloned();
        if let Some(order) = cloned_order {
            if order.contingency_type() != Some(ContingencyType::NoContingency) {
                self.handle_contingencies(order);
            }
        } else {
            log::error!(
                "Cannot handle `OrderExpired`: order for client_order_id: {} not found, {}",
                expired.client_order_id,
                expired
            );
        }
    }

    pub fn handle_order_updated(&mut self, updated: OrderUpdated) {
        let cloned_order = self.cache.borrow().order(&updated.client_order_id).cloned();
        if let Some(order) = cloned_order {
            if order.contingency_type() != Some(ContingencyType::NoContingency) {
                self.handle_contingencies_update(order);
            }
        } else {
            log::error!(
                "Cannot handle `OrderUpdated`: order for client_order_id: {} not found, {}",
                updated.client_order_id,
                updated
            );
        }
    }

    /// # Panics
    ///
    /// Panics if the OTO child order cannot be found for the given client order ID.
    pub fn handle_order_filled(&mut self, filled: OrderFilled) {
        let order = if let Some(order) = self.cache.borrow().order(&filled.client_order_id).cloned()
        {
            order
        } else {
            log::error!(
                "Cannot handle `OrderFilled`: order for client_order_id: {} not found, {}",
                filled.client_order_id,
                filled
            );
            return;
        };

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
                            return;
                        }
                    }
                    None => order.filled_qty(),
                };

                let linked_orders = if let Some(orders) = order.linked_order_ids() {
                    orders
                } else {
                    log::error!("No linked orders found for OTO order");
                    return;
                };

                for client_order_id in linked_orders {
                    let mut child_order =
                        if let Some(order) = self.cache.borrow().order(client_order_id).cloned() {
                            order
                        } else {
                            panic!(
                                "Cannot find OTO child order for client_order_id: {client_order_id}"
                            );
                        };

                    if !self.should_manage_order(&child_order) {
                        continue;
                    }

                    if child_order.position_id().is_none() {
                        child_order.set_position_id(position_id);
                    }

                    if parent_filled_qty != child_order.leaves_qty() {
                        self.modify_order_quantity(&mut child_order, parent_filled_qty);
                    }

                    // if self.submit_order_handler.is_none() {
                    //     return;
                    // }

                    if !self
                        .submit_order_commands
                        .contains_key(&child_order.client_order_id())
                        && let Err(e) =
                            self.create_new_submit_order(&child_order, position_id, client_id)
                    {
                        log::error!("Failed to create new submit order: {e}");
                    }
                }
            }
            Some(ContingencyType::Oco) => {
                let linked_orders = if let Some(orders) = order.linked_order_ids() {
                    orders
                } else {
                    log::error!("No linked orders found for OCO order");
                    return;
                };

                for client_order_id in linked_orders {
                    let contingent_order = match self.cache.borrow().order(client_order_id).cloned()
                    {
                        Some(contingent_order) => contingent_order,
                        None => {
                            panic!(
                                "Cannot find OCO contingent order for client_order_id: {client_order_id}"
                            );
                        }
                    };

                    // Not being managed || Already completed
                    if !self.should_manage_order(&contingent_order) || contingent_order.is_closed()
                    {
                        continue;
                    }
                    if contingent_order.client_order_id() != order.client_order_id() {
                        self.cancel_order(&contingent_order);
                    }
                }
            }
            Some(ContingencyType::Ouo) => self.handle_contingencies(order),
            _ => {}
        }
    }

    /// # Panics
    ///
    /// Panics if a contingent order cannot be found for the given client order ID.
    pub fn handle_contingencies(&mut self, order: OrderAny) {
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
                    return;
                }
            } else {
                (order.filled_qty(), order.leaves_qty(), false)
            };

        let linked_orders = if let Some(orders) = order.linked_order_ids() {
            orders
        } else {
            log::error!("No linked orders found");
            return;
        };

        for client_order_id in linked_orders {
            let mut contingent_order =
                if let Some(order) = self.cache.borrow().order(client_order_id).cloned() {
                    order
                } else {
                    panic!("Cannot find contingent order for client_order_id: {client_order_id}");
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
                        self.cancel_order(&contingent_order);
                    } else if filled_qty.raw > 0 && filled_qty != contingent_order.quantity() {
                        self.modify_order_quantity(&mut contingent_order, filled_qty);
                    }
                }
                Some(ContingencyType::Oco) => {
                    if order.is_closed() && (order.exec_spawn_id().is_none() || !is_spawn_active) {
                        self.cancel_order(&contingent_order);
                    }
                }
                Some(ContingencyType::Ouo) => {
                    if (leaves_qty.raw == 0 && order.exec_spawn_id().is_some())
                        || (order.is_closed()
                            && (order.exec_spawn_id().is_none() || !is_spawn_active))
                    {
                        self.cancel_order(&contingent_order);
                    } else if leaves_qty != contingent_order.leaves_qty() {
                        self.modify_order_quantity(&mut contingent_order, leaves_qty);
                    }
                }
                _ => {}
            }
        }
    }

    /// # Panics
    ///
    /// Panics if an OCO contingent order cannot be found for the given client order ID.
    pub fn handle_contingencies_update(&mut self, order: OrderAny) {
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
                    return;
                }
            }
            None => order.quantity(),
        };

        if quantity.raw == 0 {
            return;
        }

        let linked_orders = if let Some(orders) = order.linked_order_ids() {
            orders
        } else {
            log::error!("No linked orders found for contingent order");
            return;
        };

        for client_order_id in linked_orders {
            let mut contingent_order = match self.cache.borrow().order(client_order_id).cloned() {
                Some(contingent_order) => contingent_order,
                None => panic!(
                    "Cannot find OCO contingent order for client_order_id: {client_order_id}"
                ),
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
                self.modify_order_quantity(&mut contingent_order, quantity);
            }
        }
    }

    pub fn handle_position_event(&mut self, _event: OrderEventAny) {
        todo!()
    }

    // Message sending methods
    pub fn send_emulator_command(&self, command: TradingCommand) {
        log::info!("{CMD}{SEND} {command}");

        msgbus::send_any("OrderEmulator.execute".into(), &command);
    }

    pub fn send_algo_command(&self, command: SubmitOrder, exec_algorithm_id: ExecAlgorithmId) {
        log::info!("{CMD}{SEND} {command}");

        let endpoint = format!("{exec_algorithm_id}.execute");
        msgbus::send_any(endpoint.into(), &TradingCommand::SubmitOrder(command));
    }

    pub fn send_risk_command(&self, command: TradingCommand) {
        log::info!("{CMD}{SEND} {command}");
        msgbus::send_any("RiskEngine.execute".into(), &command);
    }

    pub fn send_exec_command(&self, command: TradingCommand) {
        log::info!("{CMD}{SEND} {command}");
        msgbus::send_any("ExecEngine.execute".into(), &command);
    }

    pub fn send_risk_event(&self, event: OrderEventAny) {
        log::info!("{EVT}{SEND} {event}");
        msgbus::send_any("RiskEngine.process".into(), &event);
    }

    pub fn send_exec_event(&self, event: OrderEventAny) {
        log::info!("{EVT}{SEND} {event}");
        msgbus::send_any("ExecEngine.process".into(), &event);
    }
}
