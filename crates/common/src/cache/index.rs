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

use std::collections::{HashMap, HashSet};

use nautilus_model::identifiers::{
    AccountId, ClientId, ClientOrderId, ComponentId, ExecAlgorithmId, InstrumentId, PositionId,
    StrategyId, Venue, VenueOrderId,
};

/// A key-value lookup index for a `Cache`.
#[derive(Debug)]
pub struct CacheIndex {
    pub(crate) venue_account: HashMap<Venue, AccountId>,
    pub(crate) venue_orders: HashMap<Venue, HashSet<ClientOrderId>>,
    pub(crate) venue_positions: HashMap<Venue, HashSet<PositionId>>,
    pub(crate) venue_order_ids: HashMap<VenueOrderId, ClientOrderId>,
    pub(crate) client_order_ids: HashMap<ClientOrderId, VenueOrderId>,
    pub(crate) order_position: HashMap<ClientOrderId, PositionId>,
    pub(crate) order_strategy: HashMap<ClientOrderId, StrategyId>,
    pub(crate) order_client: HashMap<ClientOrderId, ClientId>,
    pub(crate) position_strategy: HashMap<PositionId, StrategyId>,
    pub(crate) position_orders: HashMap<PositionId, HashSet<ClientOrderId>>,
    pub(crate) instrument_orders: HashMap<InstrumentId, HashSet<ClientOrderId>>,
    pub(crate) instrument_positions: HashMap<InstrumentId, HashSet<PositionId>>,
    pub(crate) strategy_orders: HashMap<StrategyId, HashSet<ClientOrderId>>,
    pub(crate) strategy_positions: HashMap<StrategyId, HashSet<PositionId>>,
    pub(crate) exec_algorithm_orders: HashMap<ExecAlgorithmId, HashSet<ClientOrderId>>,
    pub(crate) exec_spawn_orders: HashMap<ClientOrderId, HashSet<ClientOrderId>>,
    pub(crate) orders: HashSet<ClientOrderId>,
    pub(crate) orders_open: HashSet<ClientOrderId>,
    pub(crate) orders_closed: HashSet<ClientOrderId>,
    pub(crate) orders_emulated: HashSet<ClientOrderId>,
    pub(crate) orders_inflight: HashSet<ClientOrderId>,
    pub(crate) orders_pending_cancel: HashSet<ClientOrderId>,
    pub(crate) positions: HashSet<PositionId>,
    pub(crate) positions_open: HashSet<PositionId>,
    pub(crate) positions_closed: HashSet<PositionId>,
    pub(crate) actors: HashSet<ComponentId>,
    pub(crate) strategies: HashSet<StrategyId>,
    pub(crate) exec_algorithms: HashSet<ExecAlgorithmId>,
}

impl Default for CacheIndex {
    /// Creates a new default [`CacheIndex`] instance.
    fn default() -> Self {
        Self {
            venue_account: HashMap::new(),
            venue_orders: HashMap::new(),
            venue_positions: HashMap::new(),
            venue_order_ids: HashMap::new(),
            client_order_ids: HashMap::new(),
            order_position: HashMap::new(),
            order_strategy: HashMap::new(),
            order_client: HashMap::new(),
            position_strategy: HashMap::new(),
            position_orders: HashMap::new(),
            instrument_orders: HashMap::new(),
            instrument_positions: HashMap::new(),
            strategy_orders: HashMap::new(),
            strategy_positions: HashMap::new(),
            exec_algorithm_orders: HashMap::new(),
            exec_spawn_orders: HashMap::new(),
            orders: HashSet::new(),
            orders_open: HashSet::new(),
            orders_closed: HashSet::new(),
            orders_emulated: HashSet::new(),
            orders_inflight: HashSet::new(),
            orders_pending_cancel: HashSet::new(),
            positions: HashSet::new(),
            positions_open: HashSet::new(),
            positions_closed: HashSet::new(),
            actors: HashSet::new(),
            strategies: HashSet::new(),
            exec_algorithms: HashSet::new(),
        }
    }
}

impl CacheIndex {
    /// Clears the index which will clear/reset all internal state.
    pub fn clear(&mut self) {
        self.venue_account.clear();
        self.venue_orders.clear();
        self.venue_positions.clear();
        self.venue_order_ids.clear();
        self.client_order_ids.clear();
        self.order_position.clear();
        self.order_strategy.clear();
        self.order_client.clear();
        self.position_strategy.clear();
        self.position_orders.clear();
        self.instrument_orders.clear();
        self.instrument_positions.clear();
        self.strategy_orders.clear();
        self.strategy_positions.clear();
        self.exec_algorithm_orders.clear();
        self.exec_spawn_orders.clear();
        self.orders.clear();
        self.orders_open.clear();
        self.orders_closed.clear();
        self.orders_emulated.clear();
        self.orders_inflight.clear();
        self.orders_pending_cancel.clear();
        self.positions.clear();
        self.positions_open.clear();
        self.positions_closed.clear();
        self.actors.clear();
        self.strategies.clear();
        self.exec_algorithms.clear();
    }
}
