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

use std::fmt::Display;

use derive_builder::Builder;
use nautilus_core::{time::UnixNanos, uuid::UUID4};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::identifiers::{
    account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
    strategy_id::StrategyId, trader_id::TraderId, venue_order_id::VenueOrderId,
};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Builder)]
#[builder(default)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderCancelRejected {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub reason: Ustr,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: u8,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
}

impl OrderCancelRejected {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: Ustr,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reconciliation: bool,
        venue_order_id: Option<VenueOrderId>,
        account_id: Option<AccountId>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            event_id,
            ts_event,
            ts_init,
            reconciliation: u8::from(reconciliation),
            venue_order_id,
            account_id,
        })
    }
}

impl Display for OrderCancelRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OrderCancelRejected(instrument_id={}, client_order_id={}, venue_order_id={}, account_id={}, reason={}, ts_event={})",
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id.map_or_else(|| "None".to_string(), |venue_order_id| format!("{venue_order_id}")),
            self.account_id.map_or_else(|| "None".to_string(), |account_id| format!("{account_id}")),
            self.reason,
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

    use crate::events::order::{cancel_rejected::OrderCancelRejected, stubs::*};

    #[rstest]
    fn test_order_cancel_rejected(order_cancel_rejected: OrderCancelRejected) {
        let display = format!("{order_cancel_rejected}");
        assert_eq!(
            display,
            "OrderCancelRejected(instrument_id=BTCUSDT.COINBASE, client_order_id=O-20200814-102234-001-001-1, venue_order_id=001, account_id=SIM-001, reason=ORDER_DOES_NOT_EXISTS, ts_event=0)"
        );
    }
}
