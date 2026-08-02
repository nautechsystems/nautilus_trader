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

//! Time-Weighted Average Price (TWAP) execution algorithm.
//!
//! The TWAP algorithm executes orders by evenly spreading them over a specified
//! time horizon at regular intervals. This helps reduce market impact by avoiding
//! concentration of trade size at any given time.
//!
//! # Parameters
//!
//! Orders submitted to this algorithm must include `exec_algorithm_params` with:
//! - `horizon_secs`: Total execution horizon in seconds.
//! - `interval_secs`: Interval between child orders in seconds.
//!
//! # Example
//!
//! An order with `horizon_secs=60` and `interval_secs=10` will spawn 6 child
//! orders over 60 seconds, one every 10 seconds.

use std::time::Duration;

use ahash::AHashMap;
use nautilus_common::{
    actor::{DataActor, DataActorNative},
    timer::TimeEvent,
};
use nautilus_model::{
    enums::OrderType,
    events::OrderDeniedReason,
    identifiers::ClientOrderId,
    instruments::Instrument,
    orders::{Order, OrderAny},
    types::Quantity,
};
use rust_decimal::{Decimal, RoundingStrategy};
use ustr::Ustr;

use super::{
    ExecutionAlgorithm, ExecutionAlgorithmConfig, ExecutionAlgorithmCore, ExecutionAlgorithmNative,
};
use crate::nautilus_execution_algorithm;

/// Configuration for [`TwapAlgorithm`].
pub type TwapAlgorithmConfig = ExecutionAlgorithmConfig;

/// Time-Weighted Average Price (TWAP) execution algorithm.
///
/// Executes orders by evenly spreading them over a specified time horizon,
/// at regular intervals. The algorithm receives a primary order and spawns
/// smaller child orders that are executed at regular intervals.
#[derive(Debug)]
pub struct TwapAlgorithm {
    /// The algorithm core.
    pub core: ExecutionAlgorithmCore,
    /// Scheduled sizes for each primary order.
    scheduled_sizes: AHashMap<ClientOrderId, Vec<Quantity>>,
}

impl TwapAlgorithm {
    /// Creates a new [`TwapAlgorithm`] instance.
    #[must_use]
    pub fn new(config: TwapAlgorithmConfig) -> Self {
        Self {
            core: ExecutionAlgorithmCore::new(config),
            scheduled_sizes: AHashMap::new(),
        }
    }

    /// Completes the execution sequence for a primary order.
    fn complete_sequence(&mut self, primary_id: ClientOrderId) {
        let timer_name = primary_id.as_str();
        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        if core.clock_mut().timer_names().contains(&timer_name) {
            core.clock_mut().cancel_timer(timer_name);
        }
        core.remove_submit_params(&primary_id);
        self.scheduled_sizes.remove(&primary_id);
        log::info!("Completed TWAP execution for {primary_id}");
    }
}

// The clock and component lifecycle dispatch through the `DataActor` hooks,
// so forward them to the `ExecutionAlgorithm` implementations.
impl DataActor for TwapAlgorithm {
    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        ExecutionAlgorithm::on_time_event(self, event)
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        ExecutionAlgorithm::on_stop(self)
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        ExecutionAlgorithm::on_reset(self)
    }
}

nautilus_execution_algorithm!(TwapAlgorithm, {
    fn on_order(&mut self, order: OrderAny) -> anyhow::Result<()> {
        let primary_id = order.client_order_id();

        if self.scheduled_sizes.contains_key(&primary_id) {
            anyhow::bail!("Order {primary_id} already being executed");
        }

        log::info!("Received order for TWAP execution: {order:?}");

        // Only market orders supported
        if order.order_type() != OrderType::Market {
            let reason = OrderDeniedReason::UnsupportedOrderType {
                order_type: order.order_type(),
            }
            .to_string();
            return self.deny_order(&order, Ustr::from(&reason));
        }

        let instrument = {
            let cache = ExecutionAlgorithmNative::exec_algorithm_core(self).cache_ref();
            cache.instrument(&order.instrument_id()).cloned()
        };

        let Some(instrument) = instrument else {
            let reason = OrderDeniedReason::InstrumentNotFound {
                instrument_id: order.instrument_id(),
            }
            .to_string();
            return self.deny_order(&order, Ustr::from(&reason));
        };

        let Some(exec_params) = order.exec_algorithm_params() else {
            return self.deny_order(&order, validation_failed("exec_algorithm_params not found"));
        };

        let Some(horizon_secs_str) = exec_params.get(&Ustr::from("horizon_secs")) else {
            return self.deny_order(
                &order,
                validation_failed("horizon_secs not found in exec_algorithm_params"),
            );
        };

        let horizon_secs: f64 = match horizon_secs_str.parse() {
            Ok(value) => value,
            Err(_) => {
                return self.deny_order(
                    &order,
                    validation_failed(format!(
                        "horizon_secs={horizon_secs_str} is not a valid number"
                    )),
                );
            }
        };

        let Some(interval_secs_str) = exec_params.get(&Ustr::from("interval_secs")) else {
            return self.deny_order(
                &order,
                validation_failed("interval_secs not found in exec_algorithm_params"),
            );
        };

        let interval_secs: f64 = match interval_secs_str.parse() {
            Ok(value) => value,
            Err(_) => {
                return self.deny_order(
                    &order,
                    validation_failed(format!(
                        "interval_secs={interval_secs_str} is not a valid number"
                    )),
                );
            }
        };

        if !horizon_secs.is_finite() || horizon_secs <= 0.0 {
            return self.deny_order(
                &order,
                validation_failed(format!(
                    "horizon_secs={horizon_secs} must be finite and positive"
                )),
            );
        }

        if !interval_secs.is_finite() || interval_secs <= 0.0 {
            return self.deny_order(
                &order,
                validation_failed(format!(
                    "interval_secs={interval_secs} must be finite and positive"
                )),
            );
        }

        if horizon_secs < interval_secs {
            return self.deny_order(
                &order,
                validation_failed(format!(
                    "horizon_secs={horizon_secs} must be greater than or equal to interval_secs={interval_secs}"
                )),
            );
        }

        let num_intervals = (horizon_secs / interval_secs).floor() as u64;
        if num_intervals == 0 {
            return self.deny_order(&order, validation_failed("num_intervals is 0"));
        }

        let total_qty = order.quantity();
        let interval_count = Decimal::from(num_intervals);
        let quotient = total_qty.as_decimal() / interval_count;
        let floored = quotient.round_dp_with_strategy(
            u32::from(instrument.size_precision()),
            RoundingStrategy::ToZero,
        );
        let qty_per_interval = match instrument.try_make_qty_from_decimal(floored, None) {
            Ok(quantity) => quantity,
            Err(e) => {
                return self.deny_order(
                    &order,
                    validation_failed(format!("invalid qty_per_interval={floored}: {e}")),
                );
            }
        };
        let remainder = total_qty.as_decimal() - floored * interval_count;

        if qty_per_interval == total_qty || qty_per_interval < instrument.size_increment() {
            log::warn!(
                "Submitting for entire size: qty_per_interval={qty_per_interval}, order_quantity={total_qty}"
            );
            self.submit_order(order, None, None)?;
            self.complete_sequence(primary_id);
            return Ok(());
        }

        if let Some(min_qty) = instrument.min_quantity()
            && qty_per_interval < min_qty
        {
            log::warn!(
                "Submitting for entire size: qty_per_interval={qty_per_interval} < min_quantity={min_qty}"
            );
            self.submit_order(order, None, None)?;
            self.complete_sequence(primary_id);
            return Ok(());
        }

        let interval = match Duration::try_from_secs_f64(interval_secs) {
            Ok(interval) => interval,
            Err(e) => {
                return self.deny_order(
                    &order,
                    validation_failed(format!(
                        "interval_secs={interval_secs} is not a valid duration: {e}"
                    )),
                );
            }
        };

        if interval == Duration::ZERO {
            return self.deny_order(
                &order,
                validation_failed(format!(
                    "interval_secs={interval_secs} rounds to a zero duration"
                )),
            );
        }
        let Ok(interval_ns) = u64::try_from(interval.as_nanos()) else {
            return self.deny_order(
                &order,
                validation_failed(format!(
                    "interval_secs={interval_secs} exceeds the clock nanosecond range"
                )),
            );
        };
        let timestamp_ns = ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .clock_mut()
            .timestamp_ns()
            .as_u64();

        if timestamp_ns.checked_add(interval_ns).is_none() {
            return self.deny_order(
                &order,
                validation_failed(format!(
                    "interval_secs={interval_secs} exceeds the clock timestamp headroom"
                )),
            );
        }

        let mut scheduled_sizes: Vec<Quantity> = vec![qty_per_interval; num_intervals as usize];

        if remainder > Decimal::ZERO {
            let remainder_qty = match instrument.try_make_qty_from_decimal(remainder, None) {
                Ok(quantity) => quantity,
                Err(e) => {
                    return self.deny_order(
                        &order,
                        validation_failed(format!("invalid qty_remainder={remainder}: {e}")),
                    );
                }
            };
            scheduled_sizes.push(remainder_qty);
        }

        let scheduled_total = scheduled_sizes
            .iter()
            .fold(Decimal::ZERO, |total, quantity| {
                total + quantity.as_decimal()
            });

        if scheduled_total != total_qty.as_decimal() {
            return self.deny_order(
                &order,
                validation_failed(format!(
                    "scheduled quantity {scheduled_total} does not equal order quantity {total_qty}"
                )),
            );
        }

        log::info!("Order execution size schedule: {scheduled_sizes:?}");

        // Add primary order to cache so on_time_event can retrieve it,
        // it is already present when routed through the engine's submit path.
        {
            let cache_rc = ExecutionAlgorithmNative::exec_algorithm_core(self).cache_rc();
            let mut cache = cache_rc.borrow_mut();
            if !cache.order_exists(&primary_id) {
                cache.add_order(order.clone(), None, None, false)?;
            }
        }

        self.scheduled_sizes
            .insert(primary_id, scheduled_sizes.clone());

        let first_qty = self.scheduled_sizes.get_mut(&primary_id).unwrap().remove(0);
        let is_single_slice = self
            .scheduled_sizes
            .get(&primary_id)
            .is_some_and(|s| s.is_empty());

        // Single slice: submit the primary order directly
        if is_single_slice {
            self.submit_order(order, None, None)?;
            self.complete_sequence(primary_id);
            return Ok(());
        }

        // Multiple slices: spawn first child order and reduce primary
        let tags = order.tags().map(|t| t.to_vec());
        let time_in_force = order.time_in_force();
        let reduce_only = order.is_reduce_only();
        let mut order = order;
        let spawned = self.spawn_market(
            &mut order,
            first_qty,
            time_in_force,
            reduce_only,
            tags,
            true,
        );
        self.submit_order(spawned.into(), None, None)?;

        ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .clock_mut()
            .set_timer(primary_id.as_str(), interval, None, None, None, None, None)?;

        log::info!(
            "Started TWAP execution for {primary_id}: horizon_secs={horizon_secs}, interval_secs={interval_secs}"
        );

        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        log::info!("Received time event: {event:?}");

        let primary_id = ClientOrderId::new(event.name.as_str());

        let primary = {
            let cache = ExecutionAlgorithmNative::exec_algorithm_core(self).cache_ref();
            cache.order(&primary_id).map(|o| o.clone())
        };

        let Some(primary) = primary else {
            log::error!("Cannot find primary order for exec_spawn_id={primary_id}");
            return Ok(());
        };

        if primary.is_closed() {
            self.complete_sequence(primary_id);
            return Ok(());
        }

        let Some(scheduled_sizes) = self.scheduled_sizes.get_mut(&primary_id) else {
            log::error!("Cannot find scheduled sizes for exec_spawn_id={primary_id}");
            return Ok(());
        };

        if scheduled_sizes.is_empty() {
            log::warn!("No more size to execute for exec_spawn_id={primary_id}");
            return Ok(());
        }

        let quantity = scheduled_sizes.remove(0);
        let is_final_slice = scheduled_sizes.is_empty();

        // Final slice: submit the primary order (already reduced to remaining quantity)
        if is_final_slice {
            self.submit_order(primary, None, None)?;
            self.complete_sequence(primary_id);
            return Ok(());
        }

        // Intermediate slice: spawn child order and reduce primary
        let tags = primary.tags().map(|t| t.to_vec());
        let time_in_force = primary.time_in_force();
        let reduce_only = primary.is_reduce_only();
        let mut primary = primary;
        let spawned = self.spawn_market(
            &mut primary,
            quantity,
            time_in_force,
            reduce_only,
            tags,
            true,
        );
        self.submit_order(spawned.into(), None, None)?;

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .clock_mut()
            .cancel_timers();
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.unsubscribe_all_strategy_events();
        ExecutionAlgorithmNative::exec_algorithm_core_mut(self).reset();
        self.scheduled_sizes.clear();
        Ok(())
    }
});

fn validation_failed(detail: impl Into<String>) -> Ustr {
    let reason = OrderDeniedReason::ValidationFailed {
        detail: detail.into(),
    }
    .to_string();
    Ustr::from(&reason)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use indexmap::IndexMap;
    use nautilus_common::{
        cache::Cache,
        clock::{Clock, TestClock},
        component::Component,
        enums::ComponentTrigger,
        messages::execution::{SubmitOrder, TradingCommand},
        msgbus::{self, MessagingSwitchboard, TypedHandler},
    };
    use nautilus_core::{Params, UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, OrderStatus, TimeInForce},
        events::{OrderDeniedReason, OrderEventAny, order::spec::OrderCanceledSpec},
        identifiers::{ExecAlgorithmId, InstrumentId, StrategyId, TraderId},
        orders::{LimitOrder, MarketOrder},
        types::Price,
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    fn create_twap_algorithm() -> TwapAlgorithm {
        // Use unique ID to avoid thread-local registry/msgbus conflicts in parallel tests
        let unique_id = format!("TWAP-{}", UUID4::new());
        let config = TwapAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::new(&unique_id)),
            ..Default::default()
        };
        TwapAlgorithm::new(config)
    }

    fn register_algorithm(algo: &mut TwapAlgorithm) {
        use nautilus_common::timer::TimeEventCallback;

        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        // Register a no-op default handler for timer callbacks
        clock
            .borrow_mut()
            .register_default_handler(TimeEventCallback::Rust(std::sync::Arc::new(|_| {})));

        algo.core.register(trader_id, clock, cache).unwrap();

        // Transition to Running state for tests
        algo.transition_state(ComponentTrigger::Initialize).unwrap();
        algo.transition_state(ComponentTrigger::Start).unwrap();
        algo.transition_state(ComponentTrigger::StartCompleted)
            .unwrap();
    }

    fn add_instrument_to_cache(algo: &TwapAlgorithm) {
        use nautilus_model::instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt};

        let instrument = crypto_perpetual_ethusdt();
        let cache_rc = algo.core.cache_rc();
        let mut cache = cache_rc.borrow_mut();
        cache
            .add_instrument(InstrumentAny::CryptoPerpetual(instrument))
            .unwrap();
    }

    fn create_market_order_with_params(params: IndexMap<Ustr, Ustr>) -> OrderAny {
        create_market_order_with_params_and_qty(params, Quantity::from("1.0"))
    }

    fn create_market_order_with_params_and_qty(
        params: IndexMap<Ustr, Ustr>,
        quantity: Quantity,
    ) -> OrderAny {
        let client_order_id = ClientOrderId::from("O-001");
        OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            client_order_id,
            OrderSide::Buy,
            quantity,
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false,
            false,
            None,
            None,
            None,
            None,
            Some(ExecAlgorithmId::new("TWAP")),
            Some(params),
            Some(client_order_id),
            None,
        ))
    }

    fn assert_twap_denied(algo: &mut TwapAlgorithm, order: &OrderAny, expected_reason: &str) {
        let strategy_id = order.strategy_id();
        {
            let cache_rc = algo.core.cache_rc();
            cache_rc
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
        }
        let events = Rc::new(RefCell::new(Vec::new()));
        let handler = TypedHandler::from({
            let events = events.clone();
            move |event: &OrderEventAny| events.borrow_mut().push(event.clone())
        });
        let topic = format!("events.order.{strategy_id}");
        msgbus::subscribe_order_events(topic.clone().into(), handler.clone(), None);

        algo.on_order(order.clone()).unwrap();
        algo.on_order(order.clone()).unwrap();

        msgbus::unsubscribe_order_events(topic.into(), &handler);
        let cached_order = algo.cache().order(&order.client_order_id()).unwrap();
        let events = events.borrow();

        assert_eq!(cached_order.status(), OrderStatus::Denied);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            OrderEventAny::Denied(event)
                if event.reason.as_str() == expected_reason
                    && event.strategy_id == strategy_id
                    && event.client_order_id == order.client_order_id()
        ));
        assert!(algo.scheduled_sizes.is_empty());
        assert!(algo.clock().timer_names().is_empty());
    }

    #[rstest]
    fn test_twap_creation() {
        let algo = create_twap_algorithm();
        assert!(algo.id().inner().starts_with("TWAP"));
        assert!(algo.scheduled_sizes.is_empty());
    }

    #[rstest]
    fn test_twap_registration() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        assert_eq!(algo.trader_id(), Some(TraderId::from("TRADER-001")));
    }

    #[rstest]
    fn test_twap_reset_clears_scheduled_sizes() {
        let mut algo = create_twap_algorithm();
        let primary_id = ClientOrderId::new("O-001");

        algo.scheduled_sizes
            .insert(primary_id, vec![Quantity::from("1.0")]);

        assert!(!algo.scheduled_sizes.is_empty());

        // Dispatch through the DataActor entry point the component lifecycle uses
        DataActor::on_reset(&mut algo).unwrap();

        assert!(algo.scheduled_sizes.is_empty());
    }

    #[rstest]
    fn test_twap_rejects_non_market_orders() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        let order = OrderAny::Limit(LimitOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            Price::from("50000.0"),
            TimeInForce::Gtc,
            None,  // expire_time
            false, // post_only
            false, // reduce_only
            false, // quote_quantity
            None,  // display_qty
            None,  // emulation_trigger
            None,  // trigger_instrument_id
            None,  // contingency_type
            None,  // order_list_id
            None,  // linked_order_ids
            None,  // parent_order_id
            None,  // exec_algorithm_id
            None,  // exec_algorithm_params
            None,  // exec_spawn_id
            None,  // tags
            UUID4::new(),
            0.into(),
        ));

        let reason = OrderDeniedReason::UnsupportedOrderType {
            order_type: OrderType::Limit,
        }
        .to_string();
        assert_twap_denied(&mut algo, &order, &reason);
    }

    #[rstest]
    fn test_twap_denies_missing_instrument() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("10"));
        let order = create_market_order_with_params(params);
        let reason = OrderDeniedReason::InstrumentNotFound {
            instrument_id: order.instrument_id(),
        }
        .to_string();

        assert_twap_denied(&mut algo, &order, &reason);
    }

    #[rstest]
    fn test_twap_rejects_missing_params() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None, // No exec_algorithm_params
            None,
            None,
        ));

        assert_twap_denied(
            &mut algo,
            &order,
            "VALIDATION_FAILED: exec_algorithm_params not found",
        );
    }

    #[rstest]
    #[case(
        None,
        Some("10"),
        "VALIDATION_FAILED: horizon_secs not found in exec_algorithm_params"
    )]
    #[case(
        Some("60"),
        None,
        "VALIDATION_FAILED: interval_secs not found in exec_algorithm_params"
    )]
    #[case(
        Some("not-a-number"),
        Some("10"),
        "VALIDATION_FAILED: horizon_secs=not-a-number is not a valid number"
    )]
    #[case(
        Some("60"),
        Some("not-a-number"),
        "VALIDATION_FAILED: interval_secs=not-a-number is not a valid number"
    )]
    fn test_twap_denies_missing_or_malformed_schedule_parameter(
        #[case] horizon_secs: Option<&str>,
        #[case] interval_secs: Option<&str>,
        #[case] expected_reason: &str,
    ) {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);
        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();

        if let Some(horizon_secs) = horizon_secs {
            params.insert(Ustr::from("horizon_secs"), Ustr::from(horizon_secs));
        }

        if let Some(interval_secs) = interval_secs {
            params.insert(Ustr::from("interval_secs"), Ustr::from(interval_secs));
        }

        assert_twap_denied(
            &mut algo,
            &create_market_order_with_params(params),
            expected_reason,
        );
    }

    #[rstest]
    fn test_twap_rejects_horizon_less_than_interval() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("30"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("60"));

        let order = create_market_order_with_params(params);
        assert_twap_denied(
            &mut algo,
            &order,
            "VALIDATION_FAILED: horizon_secs=30 must be greater than or equal to interval_secs=60",
        );
    }

    #[rstest]
    fn test_twap_rejects_duplicate_order() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("10"));

        let order1 = create_market_order_with_params(params.clone());
        let order2 = create_market_order_with_params(params);

        algo.on_order(order1).unwrap();
        let result = algo.on_order(order2);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("already being executed")
        );
    }

    #[rstest]
    fn test_twap_calculates_size_schedule_evenly() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        // 1.2 qty over 60s with 20s intervals = 3 intervals of 0.4 each (divides evenly)
        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params_and_qty(params, Quantity::from("1.2"));
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();

        // First slice spawned immediately, remaining 2 slices scheduled (no remainder)
        let remaining = algo.scheduled_sizes.get(&primary_id).unwrap();
        assert_eq!(remaining.len(), 2);

        for qty in remaining {
            assert_eq!(*qty, Quantity::from("0.4"));
        }
    }

    #[rstest]
    fn test_twap_reduces_cached_primary_after_first_child_spawn() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("30"));

        let order = create_market_order_with_params_and_qty(params, Quantity::from("1.2"));
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();

        let cache = algo.cache();
        let primary = cache.order(&primary_id).unwrap();
        let spawned = cache.order(&ClientOrderId::from("O-001-E1")).unwrap();

        assert_eq!(primary.quantity(), Quantity::from("0.6"));
        assert_eq!(spawned.quantity(), Quantity::from("0.6"));
        assert_eq!(spawned.exec_spawn_id(), Some(primary_id));
    }

    #[rstest]
    fn test_twap_on_order_accepts_already_cached_primary() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params_and_qty(params, Quantity::from("1.2"));
        let primary_id = order.client_order_id();

        // The engine submit path caches the primary before routing to the algorithm
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(order.clone(), None, None, false).unwrap();
        }

        algo.on_order(order).unwrap();

        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 2);
    }

    #[rstest]
    fn test_twap_calculates_size_schedule_with_remainder() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);
        let instrument = nautilus_model::instruments::stubs::crypto_perpetual_ethusdt();

        // 1.0 qty over 60s with 20s intervals = 3 intervals
        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params(params);
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();

        // First slice spawned, 3 remaining (2 regular + 1 remainder)
        let remaining = algo.scheduled_sizes.get(&primary_id).unwrap();
        assert_eq!(remaining.len(), 3);
        assert_eq!(
            remaining,
            &[
                Quantity::from("0.333"),
                Quantity::from("0.333"),
                Quantity::from("0.001"),
            ]
        );

        for quantity in remaining {
            assert_eq!(quantity.precision, instrument.size_precision());
            assert_eq!(quantity.raw % instrument.size_increment().raw, 0);
        }

        let first = algo
            .cache()
            .order(&ClientOrderId::from("O-001-E1"))
            .unwrap()
            .quantity();
        let total = remaining
            .iter()
            .fold(first.as_decimal(), |sum, qty| sum + qty.as_decimal());
        assert_eq!(total, Quantity::from("1.0").as_decimal());
    }

    #[rstest]
    fn test_twap_children_use_instrument_size_precision() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);
        let instrument = nautilus_model::instruments::stubs::crypto_perpetual_ethusdt();

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params_and_qty(params, Quantity::from("1.000000000"));
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();

        let spawned = algo
            .cache()
            .order(&ClientOrderId::from("O-001-E1"))
            .unwrap()
            .quantity();
        assert_eq!(spawned.precision, instrument.size_precision());
        assert!(
            algo.scheduled_sizes[&primary_id]
                .iter()
                .all(|quantity| quantity.precision == instrument.size_precision())
        );
    }

    #[rstest]
    fn test_twap_on_time_event_spawns_next_slice() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        // Use qty that divides evenly: 1.2 / 3 = 0.4 each
        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params_and_qty(params, Quantity::from("1.2"));
        let primary_id = order.client_order_id();
        let mut submit_params = Params::new();
        submit_params.insert(
            "routing_profile".to_string(),
            serde_json::Value::String("intermediate-slice".to_string()),
        );
        algo.core
            .remember_submit_params(primary_id, Some(submit_params.clone()));

        algo.on_order(order).unwrap();

        // Verify 2 slices remain after first spawn (no remainder)
        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 2);

        // Simulate timer firing
        let event = TimeEvent::new(primary_id.inner(), UUID4::new(), 0.into(), 0.into());
        ExecutionAlgorithm::on_time_event(&mut algo, &event).unwrap();

        // One slice consumed
        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 1);
        assert_eq!(algo.core.submit_params(&primary_id), Some(submit_params));
    }

    #[rstest]
    fn test_twap_data_actor_dispatch_spawns_next_slice() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params_and_qty(params, Quantity::from("1.2"));
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();
        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 2);

        // Dispatch through the DataActor entry point the clock callback uses
        let event = TimeEvent::new(primary_id.inner(), UUID4::new(), 0.into(), 0.into());
        algo.handle_time_event(&event);

        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 1);
    }

    #[rstest]
    fn test_twap_on_time_event_completes_on_final_slice() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        // 2 intervals: first spawned immediately, one in scheduled_sizes
        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("30"));

        let order = create_market_order_with_params(params);
        let primary_id = order.client_order_id();
        let mut submit_params = Params::new();
        submit_params.insert(
            "routing_profile".to_string(),
            serde_json::Value::String("final-slice".to_string()),
        );
        algo.core
            .remember_submit_params(primary_id, Some(submit_params.clone()));

        algo.on_order(order).unwrap();
        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 1);
        assert_eq!(
            algo.core.submit_params(&primary_id),
            Some(submit_params.clone())
        );

        let received = Rc::new(RefCell::new(None::<SubmitOrder>));
        let handler = msgbus::TypedIntoHandler::from({
            let captured = received.clone();
            move |cmd: TradingCommand| {
                if let TradingCommand::SubmitOrder(cmd) = cmd {
                    *captured.borrow_mut() = Some(cmd);
                }
            }
        });
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_execute(),
            handler,
        );

        // Simulate timer firing for final slice
        let event = TimeEvent::new(primary_id.inner(), UUID4::new(), 0.into(), 0.into());
        ExecutionAlgorithm::on_time_event(&mut algo, &event).unwrap();

        // Sequence completed, scheduled_sizes removed
        assert!(algo.scheduled_sizes.get(&primary_id).is_none());
        assert_eq!(
            received
                .borrow()
                .as_ref()
                .and_then(|cmd| cmd.params.clone()),
            Some(submit_params),
        );
        assert_eq!(algo.core.submit_params(&primary_id), None);
    }

    #[rstest]
    fn test_twap_on_time_event_completes_when_primary_closed() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params_and_qty(params, Quantity::from("1.2"));
        let primary_id = order.client_order_id();
        let mut submit_params = Params::new();
        submit_params.insert(
            "routing_profile".to_string(),
            serde_json::Value::String("closed-primary".to_string()),
        );
        algo.core
            .remember_submit_params(primary_id, Some(submit_params));

        algo.on_order(order).unwrap();
        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 2);

        // Mark primary order as closed (canceled)
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            let primary = cache.order(&primary_id).map(|o| o.clone()).unwrap();

            let canceled = OrderCanceledSpec::builder()
                .trader_id(primary.trader_id())
                .strategy_id(primary.strategy_id())
                .instrument_id(primary.instrument_id())
                .client_order_id(primary.client_order_id())
                .build();
            cache
                .update_order(&OrderEventAny::Canceled(canceled))
                .unwrap();
        }

        // Timer fires but primary is closed
        let event = TimeEvent::new(primary_id.inner(), UUID4::new(), 0.into(), 0.into());
        ExecutionAlgorithm::on_time_event(&mut algo, &event).unwrap();

        // Sequence should complete early since primary is closed
        assert!(algo.scheduled_sizes.get(&primary_id).is_none());
        assert_eq!(algo.core.submit_params(&primary_id), None);
    }

    #[rstest]
    fn test_twap_on_stop_cancels_timers() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params(params);
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();

        // Verify timer is set
        assert!(
            algo.clock()
                .timer_names()
                .iter()
                .any(|name| name.as_str() == primary_id.as_str())
        );

        // Stop through the DataActor entry point the component lifecycle uses
        DataActor::on_stop(&mut algo).unwrap();

        // Timer should be canceled
        assert!(algo.clock().timer_names().is_empty());
    }

    #[rstest]
    fn test_twap_fractional_interval_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        // Use fractional interval like Python tests: 3 second horizon, 0.5 second interval
        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("3"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("0.5"));

        let order = create_market_order_with_params(params);
        let primary_id = order.client_order_id();

        // Should not error - fractional seconds should parse correctly
        algo.on_order(order).unwrap();

        // 3 / 0.5 = 6 intervals, first spawned immediately, 5 remaining (plus possible remainder)
        let remaining = algo.scheduled_sizes.get(&primary_id).unwrap();
        assert!(remaining.len() >= 5);
    }

    #[rstest]
    fn test_twap_submits_entire_size_when_qty_per_interval_below_size_increment() {
        use nautilus_model::instruments::{InstrumentAny, stubs::equity_aapl};

        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        // Use equity with size_increment of 1 (whole shares only)
        let instrument = equity_aapl();
        let instrument_id = instrument.id();
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_instrument(InstrumentAny::Equity(instrument))
                .unwrap();
        }

        // 2 shares over 60s with 10s intervals = 6 intervals
        // 2 / 6 = 0.333... which is less than size_increment of 1
        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("10"));

        let client_order_id = ClientOrderId::from("O-002");
        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            Quantity::from("2"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false,
            false,
            None,
            None,
            None,
            None,
            Some(ExecAlgorithmId::new("TWAP")),
            Some(params),
            Some(client_order_id),
            None,
        ));

        let primary_id = order.client_order_id();
        let mut submit_params = Params::new();
        submit_params.insert(
            "routing_profile".to_string(),
            serde_json::Value::String("whole-size".to_string()),
        );
        algo.core
            .remember_submit_params(primary_id, Some(submit_params));
        algo.on_order(order).unwrap();

        // Should submit entire size directly (no scheduling)
        assert!(algo.scheduled_sizes.get(&primary_id).is_none());
        assert_eq!(algo.core.submit_params(&primary_id), None);
    }

    #[rstest]
    fn test_twap_submits_entire_size_when_qty_per_interval_below_min_quantity() {
        use nautilus_model::instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt};

        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        let mut instrument = crypto_perpetual_ethusdt();
        instrument.min_quantity = Some(Quantity::from("0.5"));
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_instrument(InstrumentAny::CryptoPerpetual(instrument))
                .unwrap();
        }

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params_and_qty(params, Quantity::from("1.2"));
        let primary_id = order.client_order_id();
        let mut submit_params = Params::new();
        submit_params.insert(
            "routing_profile".to_string(),
            serde_json::Value::String("minimum-quantity".to_string()),
        );
        algo.core
            .remember_submit_params(primary_id, Some(submit_params));

        algo.on_order(order).unwrap();

        assert!(algo.scheduled_sizes.get(&primary_id).is_none());
        assert_eq!(algo.core.submit_params(&primary_id), None);
    }

    #[rstest]
    fn test_twap_rejects_negative_interval_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("-0.5"));

        let order = create_market_order_with_params(params);
        assert_twap_denied(
            &mut algo,
            &order,
            "VALIDATION_FAILED: interval_secs=-0.5 must be finite and positive",
        );
    }

    #[rstest]
    fn test_twap_rejects_negative_horizon_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("-10"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("1"));

        let order = create_market_order_with_params(params);
        assert_twap_denied(
            &mut algo,
            &order,
            "VALIDATION_FAILED: horizon_secs=-10 must be finite and positive",
        );
    }

    #[rstest]
    fn test_twap_rejects_zero_interval_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("0"));

        let order = create_market_order_with_params(params);
        assert_twap_denied(
            &mut algo,
            &order,
            "VALIDATION_FAILED: interval_secs=0 must be finite and positive",
        );
    }

    #[rstest]
    fn test_twap_rejects_huge_finite_interval_before_submission() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let received = Rc::new(RefCell::new(None::<SubmitOrder>));
        let handler = msgbus::TypedIntoHandler::from({
            let captured = received.clone();
            move |cmd: TradingCommand| {
                if let TradingCommand::SubmitOrder(cmd) = cmd {
                    *captured.borrow_mut() = Some(cmd);
                }
            }
        });
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_execute(),
            handler,
        );

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("2e20"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("1e20"));

        let duration_error = Duration::try_from_secs_f64(1e20).unwrap_err();
        let reason = OrderDeniedReason::ValidationFailed {
            detail: format!(
                "interval_secs=100000000000000000000 is not a valid duration: {duration_error}"
            ),
        }
        .to_string();
        assert_twap_denied(&mut algo, &create_market_order_with_params(params), &reason);

        assert!(received.borrow().is_none());
    }

    #[rstest]
    fn test_twap_rejects_subnanosecond_interval_before_submission() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let received = Rc::new(RefCell::new(None::<SubmitOrder>));
        let handler = msgbus::TypedIntoHandler::from({
            let captured = received.clone();
            move |cmd: TradingCommand| {
                if let TradingCommand::SubmitOrder(cmd) = cmd {
                    *captured.borrow_mut() = Some(cmd);
                }
            }
        });
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_execute(),
            handler,
        );

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("2e-10"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("1e-10"));

        assert_twap_denied(
            &mut algo,
            &create_market_order_with_params(params),
            "VALIDATION_FAILED: interval_secs=0.0000000001 rounds to a zero duration",
        );

        assert!(received.borrow().is_none());
    }

    #[rstest]
    fn test_twap_rejects_interval_exceeding_timestamp_headroom_before_submission() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);
        DataActorNative::clock_mut(&mut algo)
            .as_any_mut()
            .downcast_mut::<TestClock>()
            .unwrap()
            .set_time(UnixNanos::new(u64::MAX - 500_000_000));

        let received = Rc::new(RefCell::new(None::<SubmitOrder>));
        let handler = msgbus::TypedIntoHandler::from({
            let captured = received.clone();
            move |cmd: TradingCommand| {
                if let TradingCommand::SubmitOrder(cmd) = cmd {
                    *captured.borrow_mut() = Some(cmd);
                }
            }
        });
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_execute(),
            handler,
        );

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("2"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("1"));

        assert_twap_denied(
            &mut algo,
            &create_market_order_with_params(params),
            "VALIDATION_FAILED: interval_secs=1 exceeds the clock timestamp headroom",
        );

        assert!(received.borrow().is_none());
    }

    #[rstest]
    fn test_twap_rejects_nan_interval_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("NaN"));

        let order = create_market_order_with_params(params);
        assert_twap_denied(
            &mut algo,
            &order,
            "VALIDATION_FAILED: interval_secs=NaN must be finite and positive",
        );
    }

    #[rstest]
    fn test_twap_rejects_infinity_horizon_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("inf"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("10"));

        let order = create_market_order_with_params(params);
        assert_twap_denied(
            &mut algo,
            &order,
            "VALIDATION_FAILED: horizon_secs=inf must be finite and positive",
        );
    }
}
