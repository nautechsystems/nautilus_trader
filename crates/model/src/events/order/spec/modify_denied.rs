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

use nautilus_core::{UUID4, UnixNanos};
use ustr::Ustr;

use crate::{
    events::OrderModifyDenied,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    stubs::{TestDefault, test_uuid},
};

/// Test-only fluent spec for [`OrderModifyDenied`].
///
/// All fields carry sensible defaults so callers only set what differs.
/// `build()` constructs the event through [`OrderModifyDenied::new`] so any future invariants
/// added to the production constructor are exercised by tests built on this spec.
#[derive(Debug, Clone, bon::Builder)]
#[builder(finish_fn = into_spec)]
pub struct OrderModifyDeniedSpec {
    #[builder(default = TraderId::test_default())]
    pub trader_id: TraderId,
    #[builder(default = StrategyId::test_default())]
    pub strategy_id: StrategyId,
    #[builder(default = InstrumentId::test_default())]
    pub instrument_id: InstrumentId,
    #[builder(default = ClientOrderId::test_default())]
    pub client_order_id: ClientOrderId,
    #[builder(default = Ustr::from("TEST"))]
    pub reason: Ustr,
    #[builder(default = test_uuid())]
    pub event_id: UUID4,
    #[builder(default = UnixNanos::default())]
    pub ts_event: UnixNanos,
    #[builder(default = UnixNanos::default())]
    pub ts_init: UnixNanos,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
    pub causation_id: Option<UUID4>,
}

impl<S: order_modify_denied_spec_builder::IsComplete> OrderModifyDeniedSpecBuilder<S> {
    /// Builds the spec and constructs an [`OrderModifyDenied`] through its production constructor.
    #[must_use]
    pub fn build(self) -> OrderModifyDenied {
        let spec = self.into_spec();

        let mut event = OrderModifyDenied::new(
            spec.trader_id,
            spec.strategy_id,
            spec.instrument_id,
            spec.client_order_id,
            spec.reason,
            spec.event_id,
            spec.ts_event,
            spec.ts_init,
            spec.venue_order_id,
            spec.account_id,
        );
        event.causation_id = spec.causation_id;
        event
    }
}
