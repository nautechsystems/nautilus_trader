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

//! Shares pending order state and contingent commands within a simulated venue.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use indexmap::{IndexMap, IndexSet};
use nautilus_common::cache::Cache;
use nautilus_model::{
    events::OrderUpdated,
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    orders::Order,
    types::Quantity,
};

/// Pending order state shared by a simulated venue's instrument engines.
///
/// The venue clones one instance into each instrument engine before processing orders. It drains
/// queued commands before the next input, after its engine calls return:
///
/// 1. Pops a command with [`Self::pop_command`].
/// 1. Calls [`process_oto_command`](super::OrderMatchingEngine::process_oto_command) on the engine
///    for [`OtoCommand::instrument_id`].
/// 1. After a [`OtoCommand::Release`] with `parent_quantity: Some(quantity)`, calls
///    [`process_oto_resize`](super::OrderMatchingEngine::process_oto_resize) on the engine for
///    `parent_instrument_id`, using `parent_order_id` and `quantity`.
///
/// Each engine keeps its own book, pending fills, and fee evaluation.
#[derive(Debug, Clone, Default)]
pub struct MatchingOrders {
    pub(super) pending_updates: Rc<RefCell<IndexMap<ClientOrderId, Vec<OrderUpdated>>>>,
    pub(super) cached_filled_qty: Rc<RefCell<IndexMap<ClientOrderId, Quantity>>>,
    pub(super) pending_oto_order_ids: Rc<RefCell<IndexSet<ClientOrderId>>>,
    pub(super) released_oto_order_ids: Rc<RefCell<IndexSet<ClientOrderId>>>,
    pub(super) rejected_oto_parent_ids: Rc<RefCell<IndexSet<ClientOrderId>>>,
    pub(super) closed_oto_order_ids: Rc<RefCell<IndexSet<ClientOrderId>>>,
    pub(super) commands: Rc<RefCell<VecDeque<OtoCommand>>>,
}

/// A contingent command delivered to the order's instrument engine.
#[derive(Debug, Clone)]
pub enum OtoCommand {
    /// Releases a child; a parent quantity requests a sibling sizing recount after delivery.
    Release {
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        account_id: AccountId,
        parent_instrument_id: InstrumentId,
        parent_order_id: ClientOrderId,
        parent_quantity: Option<Quantity>,
    },
    /// Recounts the parent's child allowance after a full-trigger child fill.
    Resize {
        instrument_id: InstrumentId,
        parent_order_id: ClientOrderId,
        quantity: Quantity,
    },
    /// Cancels a child ahead of all queued commands.
    ///
    /// `cancel_contingencies` controls cancellation of linked orders. The cascade skips the linked
    /// order IDs in `excluded`.
    Cancel {
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        cancel_contingencies: bool,
        excluded: Vec<ClientOrderId>,
    },
}

impl OtoCommand {
    /// Returns the instrument which owns this command.
    pub fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::Release { instrument_id, .. }
            | Self::Resize { instrument_id, .. }
            | Self::Cancel { instrument_id, .. } => *instrument_id,
        }
    }
}

impl MatchingOrders {
    /// Removes the next command without holding a borrow during matching.
    pub fn pop_command(&self) -> Option<OtoCommand> {
        self.commands.borrow_mut().pop_front()
    }

    pub(super) fn reset(&self, instrument_id: InstrumentId, cache: &Cache) {
        let keep = |id: &ClientOrderId| {
            cache
                .order(id)
                .is_some_and(|order| order.instrument_id() != instrument_id)
        };

        self.pending_updates.borrow_mut().retain(|id, _| keep(id));
        self.cached_filled_qty.borrow_mut().retain(|id, _| keep(id));
        self.pending_oto_order_ids.borrow_mut().retain(keep);
        self.released_oto_order_ids.borrow_mut().retain(keep);
        self.rejected_oto_parent_ids.borrow_mut().retain(keep);
        self.closed_oto_order_ids.borrow_mut().retain(keep);
        self.commands.borrow_mut().retain(|command| {
            command.instrument_id() != instrument_id && !matches!(command,
                OtoCommand::Release { parent_instrument_id, .. } if *parent_instrument_id == instrument_id)
        });
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::OrderType,
        events::order::spec::OrderUpdatedSpec,
        instruments::{CryptoPerpetual, Instrument, stubs::crypto_perpetual_ethusdt},
        orders::OrderTestBuilder,
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_reset_retains_other_instrument_state(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let first = crypto_perpetual_ethusdt.id();
        let second = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let first_id = ClientOrderId::from("FIRST");
        let second_id = ClientOrderId::from("SECOND");
        let orders = MatchingOrders::default();
        let mut cache = Cache::default();
        let update = OrderUpdatedSpec::builder()
            .instrument_id(second)
            .client_order_id(second_id)
            .quantity(Quantity::from("4.560"))
            .build();

        for (instrument, id, qty) in [(first, first_id, "1.230"), (second, second_id, "4.560")] {
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(instrument)
                .client_order_id(id)
                .quantity(Quantity::from(qty))
                .build();
            cache.add_order(order, None, None, false).unwrap();

            let pending = if id == second_id {
                update
            } else {
                OrderUpdatedSpec::builder()
                    .instrument_id(instrument)
                    .client_order_id(id)
                    .quantity(Quantity::from(qty))
                    .build()
            };

            orders
                .pending_updates
                .borrow_mut()
                .insert(id, vec![pending]);
            orders
                .cached_filled_qty
                .borrow_mut()
                .insert(id, Quantity::from(qty));
            orders.pending_oto_order_ids.borrow_mut().insert(id);
            orders.released_oto_order_ids.borrow_mut().insert(id);
            orders.rejected_oto_parent_ids.borrow_mut().insert(id);
            orders.closed_oto_order_ids.borrow_mut().insert(id);
        }

        orders.commands.borrow_mut().push_back(OtoCommand::Release {
            instrument_id: second,
            client_order_id: second_id,
            account_id: AccountId::from("SIM-001"),
            parent_instrument_id: first,
            parent_order_id: first_id,
            parent_quantity: Some(Quantity::from("1.230")),
        });

        orders.commands.borrow_mut().push_back(OtoCommand::Cancel {
            instrument_id: second,
            client_order_id: second_id,
            cancel_contingencies: true,
            excluded: vec![first_id],
        });

        for (instrument_id, parent_order_id, quantity) in [
            (first, first_id, Quantity::from("2.340")),
            (second, second_id, Quantity::from("5.670")),
        ] {
            orders.commands.borrow_mut().push_back(OtoCommand::Resize {
                instrument_id,
                parent_order_id,
                quantity,
            });
        }

        orders.reset(first, &cache);
        assert_eq!(
            *orders.pending_updates.borrow(),
            IndexMap::from([(second_id, vec![update])])
        );
        assert_eq!(
            *orders.cached_filled_qty.borrow(),
            IndexMap::from([(second_id, Quantity::from("4.560"))])
        );
        assert_eq!(
            *orders.pending_oto_order_ids.borrow(),
            IndexSet::from([second_id])
        );
        assert_eq!(
            *orders.released_oto_order_ids.borrow(),
            IndexSet::from([second_id])
        );
        assert_eq!(
            *orders.rejected_oto_parent_ids.borrow(),
            IndexSet::from([second_id])
        );
        assert_eq!(
            *orders.closed_oto_order_ids.borrow(),
            IndexSet::from([second_id])
        );

        let OtoCommand::Cancel {
            instrument_id,
            client_order_id,
            cancel_contingencies,
            excluded,
        } = orders.pop_command().unwrap()
        else {
            panic!("Expected remaining peer cancellation");
        };

        assert_eq!(instrument_id, second);
        assert_eq!(client_order_id, second_id);
        assert!(cancel_contingencies);
        assert_eq!(excluded, vec![first_id]);

        let OtoCommand::Resize {
            instrument_id,
            parent_order_id,
            quantity,
        } = orders.pop_command().unwrap()
        else {
            panic!("Expected remaining peer resize");
        };

        assert_eq!(instrument_id, second);
        assert_eq!(parent_order_id, second_id);
        assert_eq!(quantity, Quantity::from("5.670"));
        assert!(orders.pop_command().is_none());
    }
}
