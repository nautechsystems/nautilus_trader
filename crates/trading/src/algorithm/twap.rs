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

use std::{
    ops::{Deref, DerefMut},
    time::Duration,
};

use ahash::AHashMap;
use nautilus_common::{
    actor::{DataActor, DataActorCore},
    timer::TimeEvent,
};
use nautilus_model::{
    enums::OrderType,
    identifiers::ClientOrderId,
    instruments::Instrument,
    orders::{Order, OrderAny},
    types::{Quantity, quantity::QuantityRaw},
};
use ustr::Ustr;

use super::{ExecutionAlgorithm, ExecutionAlgorithmConfig, ExecutionAlgorithmCore};

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
    fn complete_sequence(&mut self, primary_id: &ClientOrderId) {
        let timer_name = primary_id.as_str();
        if self.core.clock().timer_names().contains(&timer_name) {
            self.core.clock().cancel_timer(timer_name);
        }
        self.scheduled_sizes.remove(primary_id);
        log::info!("Completed TWAP execution for {primary_id}");
    }
}

impl Deref for TwapAlgorithm {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core.actor
    }
}

impl DerefMut for TwapAlgorithm {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core.actor
    }
}

impl DataActor for TwapAlgorithm {}

impl ExecutionAlgorithm for TwapAlgorithm {
    fn core_mut(&mut self) -> &mut ExecutionAlgorithmCore {
        &mut self.core
    }

    fn on_order(&mut self, order: OrderAny) -> anyhow::Result<()> {
        let primary_id = order.client_order_id();

        if self.scheduled_sizes.contains_key(&primary_id) {
            anyhow::bail!("Order {primary_id} already being executed");
        }

        log::info!("Received order for TWAP execution: {order:?}");

        // Only market orders supported
        if order.order_type() != OrderType::Market {
            log::error!(
                "Cannot execute order: only implemented for market orders, order_type={:?}",
                order.order_type()
            );
            return Ok(());
        }

        let instrument = {
            let cache = self.core.cache();
            cache.instrument(&order.instrument_id()).cloned()
        };

        let Some(instrument) = instrument else {
            log::error!(
                "Cannot execute order: instrument {} not found",
                order.instrument_id()
            );
            return Ok(());
        };

        let Some(exec_params) = order.exec_algorithm_params() else {
            log::error!(
                "Cannot execute order: exec_algorithm_params not found for primary order {primary_id}"
            );
            return Ok(());
        };

        let Some(horizon_secs_str) = exec_params.get(&Ustr::from("horizon_secs")) else {
            log::error!("Cannot execute order: horizon_secs not found in exec_algorithm_params");
            return Ok(());
        };

        let horizon_secs: f64 = horizon_secs_str.parse().map_err(|e| {
            log::error!("Cannot parse horizon_secs: {e}");
            anyhow::anyhow!("Invalid horizon_secs")
        })?;

        let Some(interval_secs_str) = exec_params.get(&Ustr::from("interval_secs")) else {
            log::error!("Cannot execute order: interval_secs not found in exec_algorithm_params");
            return Ok(());
        };

        let interval_secs: f64 = interval_secs_str.parse().map_err(|e| {
            log::error!("Cannot parse interval_secs: {e}");
            anyhow::anyhow!("Invalid interval_secs")
        })?;

        if !horizon_secs.is_finite() || horizon_secs <= 0.0 {
            log::error!(
                "Cannot execute order: horizon_secs={horizon_secs} must be finite and positive"
            );
            return Ok(());
        }

        if !interval_secs.is_finite() || interval_secs <= 0.0 {
            log::error!(
                "Cannot execute order: interval_secs={interval_secs} must be finite and positive"
            );
            return Ok(());
        }

        if horizon_secs < interval_secs {
            log::error!(
                "Cannot execute order: horizon_secs={horizon_secs} was less than interval_secs={interval_secs}"
            );
            return Ok(());
        }

        let num_intervals = (horizon_secs / interval_secs).floor() as u64;
        if num_intervals == 0 {
            log::error!("Cannot execute order: num_intervals is 0");
            return Ok(());
        }

        let total_qty = order.quantity();
        let total_raw = total_qty.raw;
        let precision = total_qty.precision;

        let qty_per_interval_raw = total_raw / (num_intervals as QuantityRaw);
        let qty_per_interval = Quantity::from_raw(qty_per_interval_raw, precision);

        if qty_per_interval == total_qty || qty_per_interval < instrument.size_increment() {
            log::warn!(
                "Submitting for entire size: qty_per_interval={qty_per_interval}, order_quantity={total_qty}"
            );
            self.submit_order(order, None, None)?;
            return Ok(());
        }

        if let Some(min_qty) = instrument.min_quantity()
            && qty_per_interval < min_qty
        {
            log::warn!(
                "Submitting for entire size: qty_per_interval={qty_per_interval} < min_quantity={min_qty}"
            );
            self.submit_order(order, None, None)?;
            return Ok(());
        }

        let mut scheduled_sizes: Vec<Quantity> = vec![qty_per_interval; num_intervals as usize];

        // Remainder goes in the last slice
        let scheduled_total = qty_per_interval_raw * (num_intervals as QuantityRaw);
        let remainder_raw = total_raw - scheduled_total;
        if remainder_raw > 0 {
            let remainder = Quantity::from_raw(remainder_raw, total_qty.precision);
            scheduled_sizes.push(remainder);
        }

        log::info!("Order execution size schedule: {scheduled_sizes:?}");

        // Add primary order to cache so on_time_event can retrieve it
        {
            let cache_rc = self.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(order.clone(), None, None, false)?;
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
            self.complete_sequence(&primary_id);
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

        {
            let cache_rc = self.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&order)?;
        }

        self.core.clock().set_timer(
            primary_id.as_str(),
            Duration::from_secs_f64(interval_secs),
            None,
            None,
            None,
            None,
            None,
        )?;

        log::info!(
            "Started TWAP execution for {primary_id}: horizon_secs={horizon_secs}, interval_secs={interval_secs}"
        );

        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        log::info!("Received time event: {event:?}");

        let primary_id = ClientOrderId::new(event.name.as_str());

        let primary = {
            let cache = self.core.cache();
            cache.order(&primary_id).cloned()
        };

        let Some(primary) = primary else {
            log::error!("Cannot find primary order for exec_spawn_id={primary_id}");
            return Ok(());
        };

        if primary.is_closed() {
            self.complete_sequence(&primary_id);
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
            self.complete_sequence(&primary_id);
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

        {
            let cache_rc = self.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&primary)?;
        }

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.core.clock().cancel_timers();
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.unsubscribe_all_strategy_events();
        self.core.reset();
        self.scheduled_sizes.clear();
        Ok(())
    }
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
    };
    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::{OrderSide, TimeInForce},
        events::OrderEventAny,
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

    fn add_instrument_to_cache(algo: &mut TwapAlgorithm) {
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
        OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            ClientOrderId::from("O-001"),
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
            None,
            None,
        ))
    }

    #[rstest]
    fn test_twap_creation() {
        let algo = create_twap_algorithm();
        assert!(algo.core.exec_algorithm_id.inner().starts_with("TWAP"));
        assert!(algo.scheduled_sizes.is_empty());
    }

    #[rstest]
    fn test_twap_registration() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        assert!(algo.core.trader_id().is_some());
    }

    #[rstest]
    fn test_twap_reset_clears_scheduled_sizes() {
        let mut algo = create_twap_algorithm();
        let primary_id = ClientOrderId::new("O-001");

        algo.scheduled_sizes
            .insert(primary_id, vec![Quantity::from("1.0")]);

        assert!(!algo.scheduled_sizes.is_empty());

        ExecutionAlgorithm::on_reset(&mut algo).unwrap();

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

        // Should not error, just log and return
        let result = algo.on_order(order);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_twap_rejects_missing_params() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
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

        // Should not error, just log and return
        let result = algo.on_order(order);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_twap_rejects_horizon_less_than_interval() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("30"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("60"));

        let order = create_market_order_with_params(params);
        let result = algo.on_order(order);

        assert!(result.is_ok());
        assert!(algo.scheduled_sizes.is_empty());
    }

    #[rstest]
    fn test_twap_rejects_duplicate_order() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

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

        add_instrument_to_cache(&mut algo);

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
    fn test_twap_calculates_size_schedule_with_remainder() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        // 1.0 qty over 60s with 20s intervals = 3 intervals
        // Raw is scaled to FIXED_PRECISION: 9 (standard) or 16 (high-precision)
        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params(params);
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();

        // First slice spawned, 3 remaining (2 regular + 1 remainder)
        let remaining = algo.scheduled_sizes.get(&primary_id).unwrap();
        assert_eq!(remaining.len(), 3);

        // Expected raw values depend on FIXED_PRECISION
        // Standard (9):  1_000_000_000 / 3 = 333_333_333, remainder = 1
        // High (16): 10_000_000_000_000_000 / 3 = 3_333_333_333_333_333, remainder = 1
        #[cfg(feature = "high-precision")]
        {
            assert_eq!(remaining[0].raw, 3_333_333_333_333_333);
            assert_eq!(remaining[1].raw, 3_333_333_333_333_333);
            assert_eq!(remaining[2].raw, 1);
        }
        #[cfg(not(feature = "high-precision"))]
        {
            assert_eq!(remaining[0].raw, 333_333_333);
            assert_eq!(remaining[1].raw, 333_333_333);
            assert_eq!(remaining[2].raw, 1);
        }
    }

    #[rstest]
    fn test_twap_on_time_event_spawns_next_slice() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        // Use qty that divides evenly: 1.2 / 3 = 0.4 each
        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params_and_qty(params, Quantity::from("1.2"));
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();

        // Verify 2 slices remain after first spawn (no remainder)
        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 2);

        // Simulate timer firing
        let event = TimeEvent::new(
            Ustr::from(primary_id.as_str()),
            UUID4::new(),
            0.into(),
            0.into(),
        );
        ExecutionAlgorithm::on_time_event(&mut algo, &event).unwrap();

        // One slice consumed
        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 1);
    }

    #[rstest]
    fn test_twap_on_time_event_completes_on_final_slice() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        // 2 intervals: first spawned immediately, one in scheduled_sizes
        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("30"));

        let order = create_market_order_with_params(params);
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();
        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 1);

        // Simulate timer firing for final slice
        let event = TimeEvent::new(
            Ustr::from(primary_id.as_str()),
            UUID4::new(),
            0.into(),
            0.into(),
        );
        ExecutionAlgorithm::on_time_event(&mut algo, &event).unwrap();

        // Sequence completed, scheduled_sizes removed
        assert!(algo.scheduled_sizes.get(&primary_id).is_none());
    }

    #[rstest]
    fn test_twap_on_time_event_completes_when_primary_closed() {
        use nautilus_model::events::OrderCanceled;

        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params_and_qty(params, Quantity::from("1.2"));
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();
        assert_eq!(algo.scheduled_sizes.get(&primary_id).unwrap().len(), 2);

        // Mark primary order as closed (canceled)
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            let mut primary = cache.order(&primary_id).cloned().unwrap();

            let canceled = OrderCanceled::new(
                primary.trader_id(),
                primary.strategy_id(),
                primary.instrument_id(),
                primary.client_order_id(),
                UUID4::new(),
                0.into(),
                0.into(),
                false,
                None,
                None,
            );
            primary.apply(OrderEventAny::Canceled(canceled)).unwrap();
            cache.update_order(&primary).unwrap();
        }

        // Timer fires but primary is closed
        let event = TimeEvent::new(
            Ustr::from(primary_id.as_str()),
            UUID4::new(),
            0.into(),
            0.into(),
        );
        ExecutionAlgorithm::on_time_event(&mut algo, &event).unwrap();

        // Sequence should complete early since primary is closed
        assert!(algo.scheduled_sizes.get(&primary_id).is_none());
    }

    #[rstest]
    fn test_twap_on_stop_cancels_timers() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("20"));

        let order = create_market_order_with_params(params);
        let primary_id = order.client_order_id();

        algo.on_order(order).unwrap();

        // Verify timer is set
        assert!(
            algo.core
                .clock()
                .timer_names()
                .contains(&primary_id.as_str())
        );

        // Stop the algorithm
        ExecutionAlgorithm::on_stop(&mut algo).unwrap();

        // Timer should be canceled
        assert!(algo.core.clock().timer_names().is_empty());
    }

    #[rstest]
    fn test_twap_fractional_interval_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

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

        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            ClientOrderId::from("O-002"),
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
            None,
            None,
        ));

        let primary_id = order.client_order_id();
        algo.on_order(order).unwrap();

        // Should submit entire size directly (no scheduling)
        assert!(algo.scheduled_sizes.get(&primary_id).is_none());
    }

    #[rstest]
    fn test_twap_rejects_negative_interval_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("-0.5"));

        let order = create_market_order_with_params(params);

        // Should not error but should reject the order (no scheduling)
        let result = algo.on_order(order);
        assert!(result.is_ok());
        assert!(algo.scheduled_sizes.is_empty());
    }

    #[rstest]
    fn test_twap_rejects_negative_horizon_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("-10"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("1"));

        let order = create_market_order_with_params(params);

        // Should not error but should reject the order (no scheduling)
        let result = algo.on_order(order);
        assert!(result.is_ok());
        assert!(algo.scheduled_sizes.is_empty());
    }

    #[rstest]
    fn test_twap_rejects_zero_interval_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("0"));

        let order = create_market_order_with_params(params);

        // Should not error but should reject the order (no scheduling)
        let result = algo.on_order(order);
        assert!(result.is_ok());
        assert!(algo.scheduled_sizes.is_empty());
    }

    #[rstest]
    fn test_twap_rejects_nan_interval_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("60"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("NaN"));

        let order = create_market_order_with_params(params);

        let result = algo.on_order(order);
        assert!(result.is_ok());
        assert!(algo.scheduled_sizes.is_empty());
    }

    #[rstest]
    fn test_twap_rejects_infinity_horizon_secs() {
        let mut algo = create_twap_algorithm();
        register_algorithm(&mut algo);

        add_instrument_to_cache(&mut algo);

        let mut params = IndexMap::new();
        params.insert(Ustr::from("horizon_secs"), Ustr::from("inf"));
        params.insert(Ustr::from("interval_secs"), Ustr::from("10"));

        let order = create_market_order_with_params(params);

        let result = algo.on_order(order);
        assert!(result.is_ok());
        assert!(algo.scheduled_sizes.is_empty());
    }
}
