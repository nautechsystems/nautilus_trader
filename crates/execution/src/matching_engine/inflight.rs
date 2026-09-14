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

//! Tracks submitted orders which have not yet reached the simulated venue.

use std::{cell::RefCell, rc::Rc};

use ahash::AHashSet;
use nautilus_common::messages::execution::TradingCommand;
use nautilus_model::identifiers::ClientOrderId;

/// Shared receipt state for a venue's command queues and matching engines.
#[derive(Debug, Clone, Default)]
pub struct InflightOrders {
    orders: Rc<RefCell<AHashSet<ClientOrderId>>>,
}

impl InflightOrders {
    /// Marks a queued submit's orders as awaiting venue receipt.
    ///
    /// Non-submit commands are ignored.
    pub fn insert(&self, command: &TradingCommand) {
        self.orders.borrow_mut().extend(submit_ids(command));
    }

    /// Releases all orders in a submit immediately before the venue processes it.
    ///
    /// The first receipt releases the order even if a duplicate submit remains queued.
    /// Non-submit commands are ignored.
    pub fn remove(&self, command: &TradingCommand) {
        let mut orders = self.orders.borrow_mut();
        for id in submit_ids(command) {
            orders.remove(id);
        }
    }

    /// Returns whether the order is still awaiting venue receipt.
    pub fn contains(&self, id: ClientOrderId) -> bool {
        self.orders.borrow().contains(&id)
    }

    /// Clears receipt state when the venue discards its queues.
    pub fn clear(&self) {
        self.orders.borrow_mut().clear();
    }
}

fn submit_ids(command: &TradingCommand) -> &[ClientOrderId] {
    match command {
        TradingCommand::SubmitOrder(command) => std::slice::from_ref(&command.client_order_id),
        TradingCommand::SubmitOrderList(command) => &command.order_list.client_order_ids,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::execution::{SubmitOrder, SubmitOrderList};
    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::OrderType,
        identifiers::{InstrumentId, OrderListId, StrategyId, TraderId},
        orders::{Order, OrderList, OrderTestBuilder},
        types::Quantity,
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::single(false)]
    #[case::list(true)]
    fn test_first_receipt_releases_duplicate_submits(#[case] list: bool) {
        let orders: Vec<_> = ["O-1", "O-2"]
            .iter()
            .map(|id| {
                OrderTestBuilder::new(OrderType::Market)
                    .instrument_id(InstrumentId::from("ETHUSDT.BINANCE"))
                    .quantity(Quantity::from("1.000"))
                    .client_order_id(ClientOrderId::from(*id))
                    .order_list_id(OrderListId::from("OL-1"))
                    .build()
            })
            .collect();

        let trader_id = TraderId::from("TRADER-001");

        let command = if list {
            TradingCommand::SubmitOrderList(SubmitOrderList::new(
                trader_id,
                None,
                StrategyId::from("STRATEGY-001"),
                OrderList::from_orders(&orders, 0.into()),
                orders
                    .iter()
                    .map(|order| order.init_event().clone())
                    .collect(),
                None,
                None,
                None,
                UUID4::new(),
                0.into(),
                None,
            ))
        } else {
            TradingCommand::SubmitOrder(SubmitOrder::from_order(
                &orders[0],
                trader_id,
                None,
                None,
                UUID4::new(),
                0.into(),
            ))
        };

        let queue = InflightOrders::default();
        let engine = queue.clone();
        queue.insert(&command);
        queue.insert(&command);
        assert!(engine.contains(orders[0].client_order_id()));
        assert_eq!(engine.contains(orders[1].client_order_id()), list);

        queue.remove(&command);
        assert!(!engine.contains(orders[0].client_order_id()));
        assert!(!engine.contains(orders[1].client_order_id()));
        queue.remove(&command);
        assert!(!engine.contains(orders[0].client_order_id()));

        queue.insert(&command);
        queue.clear();
        assert!(!engine.contains(orders[0].client_order_id()));
        assert!(!engine.contains(orders[1].client_order_id()));
    }
}
