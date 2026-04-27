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
    events::OrderRejected,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    stubs::{TestDefault, test_uuid},
};

/// Test-only fluent spec for [`OrderRejected`].
///
/// All fields carry sensible defaults so callers only set what differs.
/// `build()` constructs the event through [`OrderRejected::new`] so any future invariants
/// added to the production constructor are exercised by tests built on this spec.
#[derive(Debug, Clone, bon::Builder)]
#[builder(finish_fn = into_spec)]
pub struct OrderRejectedSpec {
    #[builder(default = TraderId::test_default())]
    pub trader_id: TraderId,
    #[builder(default = StrategyId::test_default())]
    pub strategy_id: StrategyId,
    #[builder(default = InstrumentId::test_default())]
    pub instrument_id: InstrumentId,
    #[builder(default = ClientOrderId::test_default())]
    pub client_order_id: ClientOrderId,
    #[builder(default = AccountId::test_default())]
    pub account_id: AccountId,
    #[builder(default = Ustr::from("TEST"))]
    pub reason: Ustr,
    #[builder(default = test_uuid())]
    pub event_id: UUID4,
    #[builder(default = UnixNanos::default())]
    pub ts_event: UnixNanos,
    #[builder(default = UnixNanos::default())]
    pub ts_init: UnixNanos,
    #[builder(default = false)]
    pub reconciliation: bool,
    #[builder(default = false)]
    pub due_post_only: bool,
}

impl<S: order_rejected_spec_builder::IsComplete> OrderRejectedSpecBuilder<S> {
    /// Builds the spec and constructs an [`OrderRejected`] through its production constructor.
    #[must_use]
    pub fn build(self) -> OrderRejected {
        let spec = self.into_spec();
        OrderRejected::new(
            spec.trader_id,
            spec.strategy_id,
            spec.instrument_id,
            spec.client_order_id,
            spec.account_id,
            spec.reason,
            spec.event_id,
            spec.ts_event,
            spec.ts_init,
            spec.reconciliation,
            spec.due_post_only,
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
        let event = OrderRejectedSpec::builder().build();
        assert_eq!(event.trader_id, TraderId::test_default());
        assert_eq!(event.strategy_id, StrategyId::test_default());
        assert_eq!(event.instrument_id, InstrumentId::test_default());
        assert_eq!(event.client_order_id, ClientOrderId::test_default());
        assert_eq!(event.account_id, AccountId::test_default());
        assert_eq!(event.reason, Ustr::from("TEST"));
        assert_eq!(event.ts_event, UnixNanos::default());
        assert_eq!(event.ts_init, UnixNanos::default());
        assert_eq!(event.reconciliation, 0);
        assert_eq!(event.due_post_only, 0);
    }

    #[rstest]
    fn overrides_apply_through_constructor() {
        let event = OrderRejectedSpec::builder()
            .reason(Ustr::from("INSUFFICIENT_MARGIN"))
            .reconciliation(true)
            .due_post_only(true)
            .build();

        assert_eq!(event.reason, Ustr::from("INSUFFICIENT_MARGIN"));
        // Production constructor stores the bools as u8; assert against encoded values.
        assert_eq!(event.reconciliation, 1);
        assert_eq!(event.due_post_only, 1);
        assert_eq!(event.trader_id, TraderId::test_default());
    }

    #[rstest]
    fn event_ids_are_unique_within_a_run() {
        reset_test_uuid_rng();
        let a = OrderRejectedSpec::builder().build();
        let b = OrderRejectedSpec::builder().build();
        let c = OrderRejectedSpec::builder().build();
        assert_ne!(a.event_id, b.event_id);
        assert_ne!(b.event_id, c.event_id);
        assert_ne!(a.event_id, c.event_id);
    }

    #[rstest]
    fn event_id_sequence_is_reproducible() {
        // Reset before each draw so the comparison is run-order independent.
        reset_test_uuid_rng();
        let first_run: Vec<_> = (0..3)
            .map(|_| OrderRejectedSpec::builder().build().event_id)
            .collect();

        reset_test_uuid_rng();
        let second_run: Vec<_> = (0..3)
            .map(|_| OrderRejectedSpec::builder().build().event_id)
            .collect();

        assert_eq!(first_run, second_run);
    }
}
