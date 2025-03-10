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

use derive_builder::Builder;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    identifiers::{ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    orders::OrderAny,
    types::{Price, Quantity},
};
use serde::{Deserialize, Serialize};

use crate::order_emulator::emulator::OrderEmulator;

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
#[builder(default)]
#[serde(tag = "type")]
pub struct ModifyOrder {
    pub trader_id: TraderId,
    pub client_id: ClientId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub quantity: Option<Quantity>,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl ModifyOrder {
    /// Creates a new [`ModifyOrder`] instance.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        trader_id: TraderId,
        client_id: ClientId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
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
            quantity,
            price,
            trigger_price,
            command_id,
            ts_init,
        })
    }
}

impl Display for ModifyOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ModifyOrder(instrument_id={}, client_order_id={}, venue_order_id={}, quantity={}, price={}, trigger_price={})",
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.quantity
                .map_or("None".to_string(), |quantity| format!("{quantity}")),
            self.price
                .map_or("None".to_string(), |price| format!("{price}")),
            self.trigger_price
                .map_or("None".to_string(), |trigger_price| format!(
                    "{trigger_price}"
                )),
        )
    }
}

pub trait ModifyOrderHandler {
    fn handle_modify_order(&self, order: &mut OrderAny, quantity: Quantity);
}

pub enum ModifyOrderHandlerAny {
    OrderEmulator(Rc<RefCell<OrderEmulator>>),
}

impl ModifyOrderHandler for ModifyOrderHandlerAny {
    fn handle_modify_order(&self, order: &mut OrderAny, quantity: Quantity) {
        match self {
            Self::OrderEmulator(order_emulator) => {
                order_emulator.borrow_mut().update_order(order, quantity);
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {}
