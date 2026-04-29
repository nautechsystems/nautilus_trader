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

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    events::OrderInitialized,
    identifiers::{
        ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, StrategyId, TraderId,
    },
    stubs::{TestDefault, test_uuid},
    types::{Price, Quantity},
};

/// Test-only fluent spec for [`OrderInitialized`].
///
/// All fields carry sensible defaults so callers only set what differs.
/// `build()` constructs the event through [`OrderInitialized::new`] so any future invariants
/// added to the production constructor are exercised by tests built on this spec.
#[derive(Debug, Clone, bon::Builder)]
#[builder(finish_fn = into_spec)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "spec mirrors `OrderInitialized` field set; bool count is fixed by the event"
)]
pub struct OrderInitializedSpec {
    #[builder(default = TraderId::test_default())]
    pub trader_id: TraderId,
    #[builder(default = StrategyId::test_default())]
    pub strategy_id: StrategyId,
    #[builder(default = InstrumentId::test_default())]
    pub instrument_id: InstrumentId,
    #[builder(default = ClientOrderId::test_default())]
    pub client_order_id: ClientOrderId,
    #[builder(default = OrderSide::Buy)]
    pub order_side: OrderSide,
    #[builder(default = OrderType::Market)]
    pub order_type: OrderType,
    #[builder(default = Quantity::new(100_000.0, 0))]
    pub quantity: Quantity,
    #[builder(default = TimeInForce::Day)]
    pub time_in_force: TimeInForce,
    #[builder(default = false)]
    pub post_only: bool,
    #[builder(default = false)]
    pub reduce_only: bool,
    #[builder(default = false)]
    pub quote_quantity: bool,
    #[builder(default = false)]
    pub reconciliation: bool,
    #[builder(default = test_uuid())]
    pub event_id: UUID4,
    #[builder(default = UnixNanos::default())]
    pub ts_event: UnixNanos,
    #[builder(default = UnixNanos::default())]
    pub ts_init: UnixNanos,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub trigger_type: Option<TriggerType>,
    pub limit_offset: Option<Decimal>,
    pub trailing_offset: Option<Decimal>,
    pub trailing_offset_type: Option<TrailingOffsetType>,
    pub expire_time: Option<UnixNanos>,
    pub display_qty: Option<Quantity>,
    pub emulation_trigger: Option<TriggerType>,
    pub trigger_instrument_id: Option<InstrumentId>,
    pub contingency_type: Option<ContingencyType>,
    pub order_list_id: Option<OrderListId>,
    pub linked_order_ids: Option<Vec<ClientOrderId>>,
    pub parent_order_id: Option<ClientOrderId>,
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    pub exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
    pub exec_spawn_id: Option<ClientOrderId>,
    pub tags: Option<Vec<Ustr>>,
}

impl<S: order_initialized_spec_builder::IsComplete> OrderInitializedSpecBuilder<S> {
    /// Builds the spec and constructs an [`OrderInitialized`] through its production constructor.
    #[must_use]
    pub fn build(self) -> OrderInitialized {
        let spec = self.into_spec();
        OrderInitialized::new(
            spec.trader_id,
            spec.strategy_id,
            spec.instrument_id,
            spec.client_order_id,
            spec.order_side,
            spec.order_type,
            spec.quantity,
            spec.time_in_force,
            spec.post_only,
            spec.reduce_only,
            spec.quote_quantity,
            spec.reconciliation,
            spec.event_id,
            spec.ts_event,
            spec.ts_init,
            spec.price,
            spec.trigger_price,
            spec.trigger_type,
            spec.limit_offset,
            spec.trailing_offset,
            spec.trailing_offset_type,
            spec.expire_time,
            spec.display_qty,
            spec.emulation_trigger,
            spec.trigger_instrument_id,
            spec.contingency_type,
            spec.order_list_id,
            spec.linked_order_ids,
            spec.parent_order_id,
            spec.exec_algorithm_id,
            spec.exec_algorithm_params,
            spec.exec_spawn_id,
            spec.tags,
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
        let order = OrderInitializedSpec::builder().build();
        assert_eq!(order.trader_id, TraderId::test_default());
        assert_eq!(order.strategy_id, StrategyId::test_default());
        assert_eq!(order.instrument_id, InstrumentId::test_default());
        assert_eq!(order.client_order_id, ClientOrderId::test_default());
        assert_eq!(order.order_side, OrderSide::Buy);
        assert_eq!(order.order_type, OrderType::Market);
        assert_eq!(order.quantity, Quantity::new(100_000.0, 0));
        assert_eq!(order.time_in_force, TimeInForce::Day);
        assert!(!order.post_only);
        assert!(!order.reduce_only);
        assert!(!order.quote_quantity);
        assert!(!order.reconciliation);
        assert_eq!(order.ts_event, UnixNanos::default());
        assert_eq!(order.ts_init, UnixNanos::default());
        assert_eq!(order.price, None);
        assert_eq!(order.trigger_price, None);
        assert_eq!(order.trigger_type, None);
        assert_eq!(order.limit_offset, None);
        assert_eq!(order.trailing_offset, None);
        assert_eq!(order.trailing_offset_type, None);
        assert_eq!(order.expire_time, None);
        assert_eq!(order.display_qty, None);
        assert_eq!(order.emulation_trigger, None);
        assert_eq!(order.trigger_instrument_id, None);
        assert_eq!(order.contingency_type, None);
        assert_eq!(order.order_list_id, None);
        assert_eq!(order.linked_order_ids, None);
        assert_eq!(order.parent_order_id, None);
        assert_eq!(order.exec_algorithm_id, None);
        assert_eq!(order.exec_algorithm_params, None);
        assert_eq!(order.exec_spawn_id, None);
        assert_eq!(order.tags, None);
    }

    #[rstest]
    fn overrides_apply_through_constructor() {
        let order = OrderInitializedSpec::builder()
            .order_type(OrderType::Limit)
            .order_side(OrderSide::Sell)
            .quantity(Quantity::from("50"))
            .price(Price::from("1.25000"))
            .post_only(true)
            .build();

        assert_eq!(order.order_type, OrderType::Limit);
        assert_eq!(order.order_side, OrderSide::Sell);
        assert_eq!(order.quantity, Quantity::from("50"));
        assert_eq!(order.price, Some(Price::from("1.25000")));
        assert!(order.post_only);
        assert_eq!(order.trader_id, TraderId::test_default());
    }

    #[rstest]
    fn event_ids_are_unique_within_a_run() {
        reset_test_uuid_rng();
        let a = OrderInitializedSpec::builder().build();
        let b = OrderInitializedSpec::builder().build();
        let c = OrderInitializedSpec::builder().build();
        assert_ne!(a.event_id, b.event_id);
        assert_ne!(b.event_id, c.event_id);
        assert_ne!(a.event_id, c.event_id);
    }

    #[rstest]
    fn event_id_sequence_is_reproducible() {
        // Reset before each draw so the comparison is run-order independent.
        reset_test_uuid_rng();
        let first_run: Vec<_> = (0..3)
            .map(|_| OrderInitializedSpec::builder().build().event_id)
            .collect();

        reset_test_uuid_rng();
        let second_run: Vec<_> = (0..3)
            .map(|_| OrderInitializedSpec::builder().build().event_id)
            .collect();

        assert_eq!(first_run, second_run);
    }
}
