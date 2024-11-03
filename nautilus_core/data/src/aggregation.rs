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

//! Bar aggregation machinery.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, ops::Add, rc::Rc};

use chrono::TimeDelta;
use nautilus_common::{
    clock::Clock,
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    correctness::{self, FAILED},
    nanos::UnixNanos,
};
use nautilus_model::{
    data::{
        bar::{get_bar_interval, get_bar_interval_ns, get_time_bar_start, Bar, BarType},
        quote::QuoteTick,
        trade::TradeTick,
    },
    enums::AggregationSource,
    instruments::any::InstrumentAny,
    types::{fixed::FIXED_SCALAR, price::Price, quantity::Quantity},
};

pub trait BarAggregator {
    /// The [`BarType`] to be aggregated.
    fn bar_type(&self) -> BarType;
    /// Updates theaggregator  with the given price and size.
    fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos);
    /// Updates the aggregator with the given quote.
    fn handle_quote(&mut self, quote: QuoteTick) {
        let spec = self.bar_type().spec();
        self.update(
            quote.extract_price(spec.price_type),
            quote.extract_size(spec.price_type),
            quote.ts_event,
        );
    }
    /// Updates the aggregator with the given trade.
    fn handle_trade(&mut self, trade: TradeTick) {
        self.update(trade.price, trade.size, trade.ts_event);
    }
}

/// Provides a generic bar builder for aggregation.
pub struct BarBuilder {
    bar_type: BarType,
    size_precision: u8,
    initialized: bool,
    ts_last: UnixNanos,
    count: usize,
    partial_set: bool,
    last_close: Option<Price>,
    open: Option<Price>,
    high: Option<Price>,
    low: Option<Price>,
    close: Option<Price>,
    volume: Quantity,
}

impl BarBuilder {
    /// Creates a new [`BarBuilder`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - If `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    #[must_use]
    pub fn new(instrument: &InstrumentAny, bar_type: BarType) -> Self {
        correctness::check_equal(
            instrument.id(),
            bar_type.instrument_id(),
            "instrument.id",
            "bar_type.instrument_id",
        )
        .expect(FAILED);
        correctness::check_equal(
            bar_type.aggregation_source(),
            AggregationSource::Internal,
            "bar_type.aggregation_source",
            "AggregationSource::Internal",
        )
        .expect(FAILED);

        Self {
            bar_type,
            size_precision: instrument.size_precision(),
            initialized: false,
            ts_last: UnixNanos::default(),
            count: 0,
            partial_set: false,
            last_close: None,
            open: None,
            high: None,
            low: None,
            close: None,
            volume: Quantity::zero(instrument.size_precision()),
        }
    }

    /// Set the initial values for a partially completed bar.
    pub fn set_partial(&mut self, partial_bar: Bar) {
        if self.partial_set {
            return; // Already updated
        }

        self.open = Some(partial_bar.open);

        if self.high.is_none() || partial_bar.high > self.high.unwrap() {
            self.high = Some(partial_bar.high);
        }

        if self.low.is_none() || partial_bar.low < self.low.unwrap() {
            self.low = Some(partial_bar.low);
        }

        if self.close.is_none() {
            self.close = Some(partial_bar.close);
        }

        self.volume = partial_bar.volume;

        if self.ts_last == 0 {
            self.ts_last = partial_bar.ts_init;
        }

        self.partial_set = true;
        self.initialized = true;
    }

    /// Update the bar builder.
    pub fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        if ts_event < self.ts_last {
            return; // Not applicable
        }

        if self.open.is_none() {
            self.open = Some(price);
            self.high = Some(price);
            self.low = Some(price);
            self.initialized = true;
        } else {
            if price > self.high.unwrap() {
                self.high = Some(price);
            }
            if price < self.low.unwrap() {
                self.low = Some(price);
            }
        }

        self.close = Some(price);
        self.volume = self.volume.add(size);
        self.count += 1;
        self.ts_last = ts_event;
    }

    /// Reset the bar builder.
    ///
    /// All stateful fields are reset to their initial value.
    pub fn reset(&mut self) {
        self.open = None;
        self.high = None;
        self.low = None;
        self.volume = Quantity::zero(self.size_precision);
        self.count = 0;
    }

    /// Return the aggregated bar and reset.
    pub fn build_now(&mut self) -> Bar {
        self.build(self.ts_last, self.ts_last)
    }

    /// Return the aggregated bar with the given closing timestamp, and reset.
    pub fn build(&mut self, ts_event: UnixNanos, ts_init: UnixNanos) -> Bar {
        if self.open.is_none() {
            self.open = self.last_close;
            self.high = self.last_close;
            self.low = self.last_close;
            self.close = self.last_close;
        }

        // SAFETY: The open was checked, so we can assume all prices are Some
        let bar = Bar::new(
            self.bar_type,
            self.open.unwrap(),
            self.high.unwrap(),
            self.low.unwrap(),
            self.close.unwrap(),
            self.volume,
            ts_event,
            ts_init,
        );

        self.last_close = self.close;
        self.reset();
        bar
    }
}

/// Provides a means of aggregating specified bar types and sending to a registered handler.
pub struct BarAggregatorCore<H>
where
    H: FnMut(Bar),
{
    bar_type: BarType,
    builder: BarBuilder,
    handler: H,
    await_partial: bool,
}

impl<H> BarAggregatorCore<H>
where
    H: FnMut(Bar),
{
    /// Creates a new [`BarAggregatorCore`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - If `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new(
        instrument: &InstrumentAny,
        bar_type: BarType,
        handler: H,
        await_partial: bool,
    ) -> Self {
        Self {
            bar_type,
            builder: BarBuilder::new(instrument, bar_type),
            handler,
            await_partial,
        }
    }

    pub fn set_await_partial(&mut self, value: bool) {
        self.await_partial = value;
    }

    /// Set the initial values for a partially completed bar.
    pub fn set_partial(&mut self, partial_bar: Bar) {
        self.builder.set_partial(partial_bar);
    }

    fn apply_update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        self.builder.update(price, size, ts_event);
    }

    fn build_now_and_send(&mut self) {
        let bar = self.builder.build_now();
        (self.handler)(bar);
    }

    fn build_and_send(&mut self, ts_event: UnixNanos, ts_init: UnixNanos) {
        let bar = self.builder.build(ts_event, ts_init);
        (self.handler)(bar);
    }
}

/// Provides a means of building tick bars aggregated from quote and trade ticks.
///
/// When received tick count reaches the step threshold of the bar
/// specification, then a bar is created and sent to the handler.
pub struct TickBarAggregator<H>
where
    H: FnMut(Bar),
{
    core: BarAggregatorCore<H>,
}

impl<H> TickBarAggregator<H>
where
    H: FnMut(Bar),
{
    /// Creates a new [`TickBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - If `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new(
        instrument: &InstrumentAny,
        bar_type: BarType,
        handler: H,
        await_partial: bool,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(instrument, bar_type, handler, await_partial),
        }
    }
}

impl<H> BarAggregator for TickBarAggregator<H>
where
    H: FnMut(Bar),
{
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        self.core.apply_update(price, size, ts_event);
        let spec = self.core.bar_type.spec();

        if self.core.builder.count >= spec.step {
            self.core.build_now_and_send();
        }
    }
}

/// Provides a means of building volume bars aggregated from quote and trade ticks.
pub struct VolumeBarAggregator<H>
where
    H: FnMut(Bar),
{
    core: BarAggregatorCore<H>,
}

impl<H> VolumeBarAggregator<H>
where
    H: FnMut(Bar),
{
    /// Creates a new [`VolumeBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - If `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new(
        instrument: &InstrumentAny,
        bar_type: BarType,
        handler: H,
        await_partial: bool,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(instrument, bar_type, handler, await_partial),
        }
    }
}

impl<H> BarAggregator for VolumeBarAggregator<H>
where
    H: FnMut(Bar),
{
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    /// Apply the given update to the aggregator.
    #[allow(unused_assignments)] // Temp for development
    fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        let mut raw_size_update = size.raw;
        let spec = self.core.bar_type.spec();
        let raw_step = (spec.step as f64 * FIXED_SCALAR) as u64;
        let mut raw_size_diff = 0;

        while raw_size_update > 0 {
            if self.core.builder.volume.raw + raw_size_update < raw_step {
                self.core.apply_update(
                    price,
                    Quantity::from_raw(raw_size_update, size.precision),
                    ts_event,
                );
                break;
            }

            raw_size_diff = raw_step - self.core.builder.volume.raw;
            self.core.apply_update(
                price,
                Quantity::from_raw(raw_size_update, size.precision),
                ts_event,
            );

            self.core.build_now_and_send();
            raw_size_update -= raw_size_diff;
        }
    }
}

/// Provides a means of building value bars aggregated from quote and trade ticks.
///
/// When received value reaches the step threshold of the bar
/// specification, then a bar is created and sent to the handler.
pub struct ValueBarAggregator<H>
where
    H: FnMut(Bar),
{
    core: BarAggregatorCore<H>,
    cum_value: f64,
}

impl<H> ValueBarAggregator<H>
where
    H: FnMut(Bar),
{
    /// Creates a new [`ValueBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - If `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new(
        instrument: &InstrumentAny,
        bar_type: BarType,
        handler: H,
        await_partial: bool,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(instrument, bar_type, handler, await_partial),
            cum_value: 0.0,
        }
    }

    #[must_use]
    /// Returns the cumulative value for the aggregator.
    pub const fn get_cumulative_value(&self) -> f64 {
        self.cum_value
    }
}

impl<H> BarAggregator for ValueBarAggregator<H>
where
    H: FnMut(Bar),
{
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        let mut size_update = size.as_f64();
        let spec = self.core.bar_type.spec();

        while size_update > 0.0 {
            let value_update = price.as_f64() * size_update;
            if self.cum_value + value_update < spec.step as f64 {
                self.cum_value += value_update;
                self.core
                    .apply_update(price, Quantity::new(size_update, size.precision), ts_event);
                break;
            }

            let value_diff = spec.step as f64 - self.cum_value;
            let size_diff = size_update * (value_diff / value_update);
            self.core
                .apply_update(price, Quantity::new(size_diff, size.precision), ts_event);

            self.core.build_now_and_send();
            self.cum_value = 0.0;
            size_update -= size_diff;
        }
    }
}

/// Provides a means of building time bars aggregated from quote and trade ticks.
///
/// At each aggregation time interval, a bar is created and sent to the handler.
pub struct TimeBarAggregator<C, H>
where
    C: Clock,
    H: FnMut(Bar),
{
    core: BarAggregatorCore<H>,
    clock: C,
    build_with_no_updates: bool,
    timestamp_on_close: bool,
    is_left_open: bool,
    build_on_next_tick: bool,
    stored_open_ns: UnixNanos,
    stored_close_ns: UnixNanos,
    cached_update: Option<(Price, Quantity, u64)>,
    timer_name: String,
    interval: TimeDelta,
    interval_ns: UnixNanos,
    next_close_ns: UnixNanos,
}

#[derive(Clone)]
pub struct NewBarCallback<C: Clock, H: FnMut(Bar)> {
    aggregator: Rc<RefCell<TimeBarAggregator<C, H>>>,
}

impl<C: Clock, H: FnMut(Bar)> NewBarCallback<C, H> {
    pub const fn new(aggregator: Rc<RefCell<TimeBarAggregator<C, H>>>) -> Self {
        Self { aggregator }
    }
}

impl<C: Clock + 'static, H: FnMut(Bar) + 'static> From<NewBarCallback<C, H>> for TimeEventCallback {
    fn from(value: NewBarCallback<C, H>) -> Self {
        Self::Rust(Rc::new(move |event: TimeEvent| {
            value.aggregator.borrow_mut().build_bar(event);
        }))
    }
}

impl<C, H> TimeBarAggregator<C, H>
where
    C: Clock + 'static,
    H: FnMut(Bar) + 'static,
{
    /// Creates a new [`TimeBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - If `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument: &InstrumentAny,
        bar_type: BarType,
        handler: H,
        await_partial: bool,
        clock: C,
        build_with_no_updates: bool,
        timestamp_on_close: bool,
        interval_type: &str, // TODO: Make this an enum
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(instrument, bar_type, handler, await_partial),
            clock,
            build_with_no_updates,
            timestamp_on_close,
            is_left_open: false,
            build_on_next_tick: false,
            stored_open_ns: UnixNanos::default(),
            stored_close_ns: UnixNanos::default(),
            cached_update: None,
            timer_name: bar_type.to_string(),
            interval: get_bar_interval(&bar_type),
            interval_ns: get_bar_interval_ns(&bar_type),
            next_close_ns: UnixNanos::default(),
        }
    }

    /// Starts the time bar aggregator.
    pub fn start(&mut self, callback: NewBarCallback<C, H>) -> anyhow::Result<()> {
        let now = self.clock.utc_now();
        let start_time = get_time_bar_start(now, &self.bar_type());
        let start_time_ns = UnixNanos::from(start_time.timestamp_nanos_opt().unwrap() as u64);

        self.clock.set_timer_ns(
            &self.timer_name,
            self.interval_ns.as_u64(),
            start_time_ns,
            None,
            Some(callback.into()),
        );

        log::debug!("Started timer {}", self.timer_name);
        Ok(())
    }

    /// Stops the time bar aggregator.
    pub fn stop(&mut self) {
        self.clock.cancel_timer(&self.timer_name);
    }

    fn build_bar(&mut self, event: TimeEvent) {
        if !self.core.builder.initialized {
            self.build_on_next_tick = true;
            self.stored_close_ns = self.next_close_ns;
            return;
        }

        if !self.build_with_no_updates && self.core.builder.count == 0 {
            return;
        }

        let ts_init = event.ts_event;
        let ts_event = if self.is_left_open {
            if self.timestamp_on_close {
                event.ts_event
            } else {
                self.stored_open_ns
            }
        } else {
            self.stored_open_ns
        };

        self.core.build_and_send(ts_event, ts_init);
        self.stored_open_ns = event.ts_event;
        self.next_close_ns = self.clock.next_time_ns(&self.timer_name);
    }
}

impl<C, H> BarAggregator for TimeBarAggregator<C, H>
where
    C: Clock,
    H: FnMut(Bar),
{
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        self.core.apply_update(price, size, ts_event);
        if self.build_on_next_tick {
            let ts_init = ts_event;

            let ts_event = if self.is_left_open {
                if self.timestamp_on_close {
                    self.stored_close_ns
                } else {
                    self.stored_open_ns
                }
            } else {
                self.stored_open_ns
            };

            self.core.build_and_send(ts_event, ts_init);
            self.build_on_next_tick = false;
            self.stored_close_ns = UnixNanos::default();
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nautilus_model::{
        data::bar::{BarSpecification, BarType},
        enums::{AggregationSource, BarAggregation, PriceType},
        instruments::{any::InstrumentAny, equity::Equity, stubs::*},
        types::{price::Price, quantity::Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_bar_builder_instantiate(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let builder = BarBuilder::new(&instrument, bar_type);
        assert!(!builder.initialized);
        assert_eq!(builder.ts_last, 0);
        assert_eq!(builder.count, 0);
    }

    #[rstest]
    fn test_bar_builder_set_partial_updates_bar_to_expected_properties(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(&instrument, bar_type);

        let partial_bar = Bar::new(
            bar_type,
            Price::new(1.00001, 8),
            Price::new(1.00010, 8),
            Price::new(1.00000, 8),
            Price::new(1.00002, 8),
            Quantity::new(1.0, 0),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        builder.set_partial(partial_bar);
        let bar = builder.build_now();

        assert_eq!(bar.open, Price::new(1.00001, 8));
        assert_eq!(bar.high, Price::new(1.00010, 8));
        assert_eq!(bar.low, Price::new(1.00000, 8));
        assert_eq!(bar.close, Price::new(1.00002, 8));
        assert_eq!(bar.volume, Quantity::new(1.0, 0));
        assert_eq!(bar.ts_init, 2_000_000_000);
        assert_eq!(builder.ts_last, 2_000_000_000);
    }

    #[rstest]
    fn test_bar_builder_set_partial_when_already_set_does_not_update(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(&instrument, bar_type);

        let partial_bar1 = Bar::new(
            bar_type,
            Price::new(1.00001, 8),
            Price::new(1.00010, 8),
            Price::new(1.00000, 8),
            Price::new(1.00002, 8),
            Quantity::new(1.0, 0),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_000_000_000),
        );

        let partial_bar2 = Bar::new(
            bar_type,
            Price::new(2.00001, 8),
            Price::new(2.00010, 8),
            Price::new(2.00000, 8),
            Price::new(2.00002, 8),
            Quantity::new(2.0, 0),
            UnixNanos::from(3_000_000_000),
            UnixNanos::from(3_000_000_000),
        );

        builder.set_partial(partial_bar1);
        builder.set_partial(partial_bar2);
        let bar = builder.build(
            UnixNanos::from(4_000_000_000),
            UnixNanos::from(4_000_000_000),
        );

        assert_eq!(bar.open, Price::new(1.00001, 8));
        assert_eq!(bar.high, Price::new(1.00010, 8));
        assert_eq!(bar.low, Price::new(1.00000, 8));
        assert_eq!(bar.close, Price::new(1.00002, 8));
        assert_eq!(bar.volume, Quantity::new(1.0, 0));
        assert_eq!(bar.ts_init, 4_000_000_000);
        assert_eq!(builder.ts_last, 1_000_000_000);
    }

    #[rstest]
    fn test_bar_builder_single_update_results_in_expected_properties(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(&instrument, bar_type);

        builder.update(
            Price::new(1.00000, 8),
            Quantity::new(1.0, 0),
            UnixNanos::from(0),
        );

        assert!(builder.initialized);
        assert_eq!(builder.ts_last, 0);
        assert_eq!(builder.count, 1);
    }

    #[rstest]
    fn test_bar_builder_single_update_when_timestamp_less_than_last_update_ignores(
        equity_aapl: Equity,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(&instrument, bar_type);

        builder.update(
            Price::new(1.00000, 8),
            Quantity::new(1.0, 0),
            UnixNanos::from(1_000),
        );
        builder.update(
            Price::new(1.00001, 8),
            Quantity::new(1.0, 0),
            UnixNanos::from(500),
        );

        assert!(builder.initialized);
        assert_eq!(builder.ts_last, 1_000);
        assert_eq!(builder.count, 1);
    }

    #[rstest]
    fn test_bar_builder_multiple_updates_correctly_increments_count(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(&instrument, bar_type);

        for _ in 0..5 {
            builder.update(
                Price::new(1.00000, 8),
                Quantity::new(1.0, 0),
                UnixNanos::from(1_000),
            );
        }

        assert_eq!(builder.count, 5);
    }

    #[rstest]
    #[should_panic]
    fn test_bar_builder_build_when_no_updates_panics(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(&instrument, bar_type);
        let _ = builder.build_now();
    }

    #[rstest]
    fn test_bar_builder_build_when_received_updates_returns_expected_bar(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(&instrument, bar_type);

        builder.update(
            Price::new(1.00001, 8),
            Quantity::new(2.0, 0),
            UnixNanos::from(0),
        );
        builder.update(
            Price::new(1.00002, 8),
            Quantity::new(2.0, 0),
            UnixNanos::from(0),
        );
        builder.update(
            Price::new(1.00000, 8),
            Quantity::new(1.0, 0),
            UnixNanos::from(1_000_000_000),
        );

        let bar = builder.build_now();

        assert_eq!(bar.open, Price::new(1.00001, 8));
        assert_eq!(bar.high, Price::new(1.00002, 8));
        assert_eq!(bar.low, Price::new(1.00000, 8));
        assert_eq!(bar.close, Price::new(1.00000, 8));
        assert_eq!(bar.volume, Quantity::new(5.0, 0));
        assert_eq!(bar.ts_init, 1_000_000_000);
        assert_eq!(builder.ts_last, 1_000_000_000);
        assert_eq!(builder.count, 0);
    }

    #[rstest]
    fn test_bar_builder_build_with_previous_close(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(&instrument, bar_type);

        builder.update(
            Price::new(1.00001, 8),
            Quantity::new(1.0, 0),
            UnixNanos::from(0),
        );
        builder.build_now(); // This close should become the next open

        builder.update(
            Price::new(1.00000, 8),
            Quantity::new(1.0, 0),
            UnixNanos::from(0),
        );
        builder.update(
            Price::new(1.00003, 8),
            Quantity::new(1.0, 0),
            UnixNanos::from(0),
        );
        builder.update(
            Price::new(1.00002, 8),
            Quantity::new(1.0, 0),
            UnixNanos::from(0),
        );

        let bar = builder.build_now();

        assert_eq!(bar.open, Price::new(1.00000, 8));
        assert_eq!(bar.high, Price::new(1.00003, 8));
        assert_eq!(bar.low, Price::new(1.00000, 8));
        assert_eq!(bar.close, Price::new(1.00002, 8));
        assert_eq!(bar.volume, Quantity::new(3.0, 0));
    }

    #[rstest]
    fn test_tick_bar_aggregator_handle_trade_when_step_count_below_threshold(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(3, BarAggregation::Tick, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickBarAggregator::new(
            &instrument,
            bar_type,
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false,
        );

        let trade = TradeTick::default();
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 0);
    }

    #[rstest]
    fn test_tick_bar_aggregator_handle_trade_when_step_count_reached(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(3, BarAggregation::Tick, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickBarAggregator::new(
            &instrument,
            bar_type,
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false,
        );

        let trade = TradeTick::default();
        aggregator.handle_trade(trade);
        aggregator.handle_trade(trade);
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().unwrap();
        let bar = handler_guard.first().unwrap();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(bar.open, trade.price);
        assert_eq!(bar.high, trade.price);
        assert_eq!(bar.low, trade.price);
        assert_eq!(bar.close, trade.price);
        assert_eq!(bar.volume, Quantity::from(300000));
        assert_eq!(bar.ts_event, trade.ts_event);
        assert_eq!(bar.ts_init, trade.ts_init);
    }
}
