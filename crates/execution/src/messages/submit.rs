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

use std::{cell::RefCell, fmt::Display, rc::Rc};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    identifiers::{
        ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId, StrategyId, TraderId,
        VenueOrderId,
    },
    orders::OrderAny,
};
use serde::{Deserialize, Serialize};

use crate::order_emulator::emulator::OrderEmulator;

// Fix: equality and default and builder
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
// #[builder(default)]
#[serde(tag = "type")]
pub struct SubmitOrder {
    pub trader_id: TraderId,
    pub client_id: ClientId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub order: OrderAny,
    pub exec_algorith_id: Option<ExecAlgorithmId>,
    pub position_id: Option<PositionId>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl SubmitOrder {
    /// Creates a new [`SubmitOrder`] instance.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        trader_id: TraderId,
        client_id: ClientId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        order: OrderAny,
        exec_algorith_id: Option<ExecAlgorithmId>,
        position_id: Option<PositionId>,
        command_id: UUID4,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            order,
            exec_algorith_id,
            position_id,
            command_id,
            ts_init,
        })
    }
}

impl Display for SubmitOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SubmitOrder(instrument_id={}, order=TBD, position_id={})",
            self.instrument_id,
            self.position_id
                .map_or("None".to_string(), |position_id| format!("{position_id}")),
        )
    }
}

pub trait SubmitOrderHandler {
    fn handle_submit_order(&self, command: SubmitOrder);
}

pub enum SubmitOrderHandlerAny {
    OrderEmulator(Rc<RefCell<OrderEmulator>>),
}

impl SubmitOrderHandler for SubmitOrderHandlerAny {
    fn handle_submit_order(&self, command: SubmitOrder) {
        match self {
            Self::OrderEmulator(emulator) => {
                emulator.borrow_mut().handle_submit_order(command);
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {}
