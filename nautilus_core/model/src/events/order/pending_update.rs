// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt::{Display, Formatter};

use anyhow::Result;
use derive_builder::{self, Builder};
use nautilus_core::{time::UnixNanos, uuid::UUID4};
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};

use crate::identifiers::{
    account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
    strategy_id::StrategyId, trader_id::TraderId, venue_order_id::VenueOrderId,
};

#[repr(C)]
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
#[builder(default)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderPendingUpdate {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub account_id: AccountId,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: u8,
    pub venue_order_id: Option<VenueOrderId>,
}

impl OrderPendingUpdate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        account_id: AccountId,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reconciliation: bool,
        venue_order_id: Option<VenueOrderId>,
    ) -> Result<Self> {
        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation: reconciliation as u8,
            venue_order_id,
        })
    }
}

impl Display for OrderPendingUpdate {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OrderPendingUpdate(instrument_id={}, client_order_id={}, venue_order_id={}, account_id={}, ts_event={})",
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id
                .map(|venue_order_id| format!("{}", venue_order_id))
                .unwrap_or_else(|| "None".to_string()),
            self.account_id,
            self.ts_event
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod test {
    use rstest::rstest;

    use crate::events::order::{pending_update::OrderPendingUpdate, stubs::order_pending_update};

    #[rstest]
    fn test_order_pending_update_display(order_pending_update: OrderPendingUpdate) {
        let display = format!("{}", order_pending_update);
        assert_eq!(
            display,
            "OrderPendingUpdate(instrument_id=BTCUSDT.COINBASE, client_order_id=O-20200814-102234-001-001-1, venue_order_id=001, account_id=SIM-001, ts_event=0)"
        );
    }
}
