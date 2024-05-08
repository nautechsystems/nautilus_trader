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

use std::fmt::{Debug, Display};

use derive_builder::Builder;
use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use serde::{Deserialize, Serialize};

use crate::{
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
        strategy_id::StrategyId, trader_id::TraderId, venue_order_id::VenueOrderId,
    },
    types::{price::Price, quantity::Quantity},
};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Builder)]
#[builder(default)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderUpdated {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub quantity: Quantity,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: u8, // TODO: Change to bool once Cython removed
}

impl OrderUpdated {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        quantity: Quantity,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reconciliation: bool,
        venue_order_id: Option<VenueOrderId>,
        account_id: Option<AccountId>,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            quantity,
            event_id,
            ts_event,
            ts_init,
            reconciliation: u8::from(reconciliation),
            venue_order_id,
            account_id,
            price,
            trigger_price,
        })
    }
}

impl Debug for OrderUpdated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,
            "{}(trader_id={}, strategy_id={}, instrument_id={}, client_order_id={}, \
            venue_order_id={}, account_id={}, quantity={}, price={}, trigger_price={}, event_id={}, ts_event={}, ts_init={})",
            stringify!(OrderUpdated),
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id.map_or("None".to_string(), |venue_order_id| format!("{venue_order_id}")),
            self.account_id.map_or("None".to_string(), |account_id| format!("{account_id}")),
            self.quantity,
            self.price.map_or("None".to_string(), |price| price.to_formatted_string()),
            self.trigger_price.map_or("None".to_string(), |trigger_price| trigger_price.to_formatted_string()),
            self.event_id,
            self.ts_event,
            self.ts_init
        )
    }
}

impl Display for OrderUpdated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={}, client_order_id={}, venue_order_id={}, account_id={}, quantity={}, price={}, trigger_price={}, ts_event={})",
            stringify!(OrderUpdated),
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id.map_or("None".to_string(), |venue_order_id| format!("{venue_order_id}")),
            self.account_id.map_or("None".to_string(), |account_id| format!("{account_id}")),
            self.quantity.to_formatted_string(),
            self.price.map_or("None".to_string(), |price| price.to_formatted_string()),
            self.trigger_price.map_or("None".to_string(), |trigger_price| trigger_price.to_formatted_string()),
            self.ts_event
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::events::order::{stubs::*, updated::OrderUpdated};

    #[rstest]
    fn test_order_updated_display(order_updated: OrderUpdated) {
        let display = format!("{order_updated}");
        assert_eq!(
            display,
            "OrderUpdated(instrument_id=BTCUSDT.COINBASE, client_order_id=O-19700101-0000-000-001-1, venue_order_id=001, account_id=SIM-001, quantity=100, price=22_000, trigger_price=None, ts_event=0)"
        );
    }
}
