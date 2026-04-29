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

use crate::{
    events::OrderExpired,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    stubs::{TestDefault, test_uuid},
};

/// Test-only fluent spec for [`OrderExpired`].
///
/// All fields carry sensible defaults so callers only set what differs.
/// `build()` constructs the event through [`OrderExpired::new`] so any future invariants
/// added to the production constructor are exercised by tests built on this spec.
#[derive(Debug, Clone, bon::Builder)]
#[builder(finish_fn = into_spec)]
pub struct OrderExpiredSpec {
    #[builder(default = TraderId::test_default())]
    pub trader_id: TraderId,
    #[builder(default = StrategyId::test_default())]
    pub strategy_id: StrategyId,
    #[builder(default = InstrumentId::test_default())]
    pub instrument_id: InstrumentId,
    #[builder(default = ClientOrderId::test_default())]
    pub client_order_id: ClientOrderId,
    #[builder(default = test_uuid())]
    pub event_id: UUID4,
    #[builder(default = UnixNanos::default())]
    pub ts_event: UnixNanos,
    #[builder(default = UnixNanos::default())]
    pub ts_init: UnixNanos,
    #[builder(default = false)]
    pub reconciliation: bool,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: Option<AccountId>,
}

impl<S: order_expired_spec_builder::IsComplete> OrderExpiredSpecBuilder<S> {
    /// Builds the spec and constructs an [`OrderExpired`] through its production constructor.
    #[must_use]
    pub fn build(self) -> OrderExpired {
        let spec = self.into_spec();
        OrderExpired::new(
            spec.trader_id,
            spec.strategy_id,
            spec.instrument_id,
            spec.client_order_id,
            spec.event_id,
            spec.ts_event,
            spec.ts_init,
            spec.reconciliation,
            spec.venue_order_id,
            spec.account_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::stubs::reset_test_uuid_rng;

    #[rstest]
    fn defaults_are_sensible() {
        // Pin the spec's no-arg defaults so accidental drift in any individual default surfaces here,
        // rather than as silent behavior change in downstream tests.
        let event = OrderExpiredSpec::builder().build();
        assert_eq!(event.trader_id, TraderId::test_default());
        assert_eq!(event.strategy_id, StrategyId::test_default());
        assert_eq!(event.instrument_id, InstrumentId::test_default());
        assert_eq!(event.client_order_id, ClientOrderId::test_default());
        assert_eq!(event.ts_event, UnixNanos::default());
        assert_eq!(event.ts_init, UnixNanos::default());
        assert_eq!(event.reconciliation, 0);
        assert_eq!(event.venue_order_id, None);
        assert_eq!(event.account_id, None);
    }

    #[rstest]
    fn overrides_apply_through_constructor() {
        let event = OrderExpiredSpec::builder()
            .venue_order_id(VenueOrderId::from("V-1"))
            .account_id(AccountId::from("SIM-002"))
            .reconciliation(true)
            .build();

        assert_eq!(event.venue_order_id, Some(VenueOrderId::from("V-1")));
        assert_eq!(event.account_id, Some(AccountId::from("SIM-002")));
        // Production constructor stores the bool as a u8; assert against the encoded value.
        assert_eq!(event.reconciliation, 1);
        assert_eq!(event.trader_id, TraderId::test_default());
    }

    #[rstest]
    fn event_ids_are_unique_within_a_run() {
        reset_test_uuid_rng();
        let a = OrderExpiredSpec::builder().build();
        let b = OrderExpiredSpec::builder().build();
        let c = OrderExpiredSpec::builder().build();
        assert_ne!(a.event_id, b.event_id);
        assert_ne!(b.event_id, c.event_id);
        assert_ne!(a.event_id, c.event_id);
    }

    #[rstest]
    fn event_id_sequence_is_reproducible() {
        // Reset before each draw so the comparison is run-order independent.
        reset_test_uuid_rng();
        let first_run: Vec<_> = (0..3)
            .map(|_| OrderExpiredSpec::builder().build().event_id)
            .collect();

        reset_test_uuid_rng();
        let second_run: Vec<_> = (0..3)
            .map(|_| OrderExpiredSpec::builder().build().event_id)
            .collect();

        assert_eq!(first_run, second_run);
    }
}
