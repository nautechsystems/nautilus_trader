// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{cell::RefCell, ops::Add, rc::Rc};

use chrono::TimeDelta;
use nautilus_common::{
    clock::Clock,
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    UnixNanos,
    correctness::{self, FAILED},
    datetime::{add_n_months_nanos, subtract_n_months_nanos},
};
use nautilus_model::{
    data::{
        QuoteTick, TradeTick,
        bar::{Bar, BarType, get_bar_interval_ns, get_time_bar_start},
    },
    enums::{AggregationSource, BarAggregation, BarIntervalType},
    types::{Price, Quantity, fixed::FIXED_SCALAR, quantity::QuantityRaw},
};

pub trait BarAggregator {
    /// The [`BarType`] to be aggregated.
    fn bar_type(&self) -> BarType;
    /// If the aggregator is running and will receive data from the message bus.
    fn is_running(&self) -> bool;
    fn set_await_partial(&mut self, value: bool);
    fn set_is_running(&mut self, value: bool);
    /// Updates the aggregator  with the given price and size.
    fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos);
    /// Updates the aggregator with the given quote.
    fn handle_quote(&mut self, quote: QuoteTick) {
        let spec = self.bar_type().spec();
        if !self.await_partial() {
            self.update(
                quote.extract_price(spec.price_type),
                quote.extract_size(spec.price_type),
                quote.ts_event,
            );
        }
    }
    /// Updates the aggregator with the given trade.
    fn handle_trade(&mut self, trade: TradeTick) {
        if !self.await_partial() {
            self.update(trade.price, trade.size, trade.ts_event);
        }
    }
    /// Updates the aggregator with the given bar.
    fn handle_bar(&mut self, bar: Bar) {
        if !self.await_partial() {
            self.update_bar(bar, bar.volume, bar.ts_init);
        }
    }
    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos);
    fn start_batch_update(&mut self, handler: Box<dyn FnMut(Bar)>, time_ns: UnixNanos);
    fn stop_batch_update(&mut self);
    fn await_partial(&self) -> bool;
    fn set_partial(&mut self, partial_bar: Bar);
}

/// Provides a generic bar builder for aggregation.
pub struct BarBuilder {
    bar_type: BarType,
    price_precision: u8,
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
    pub fn new(bar_type: BarType, price_precision: u8, size_precision: u8) -> Self {
        correctness::check_equal(
            bar_type.aggregation_source(),
            AggregationSource::Internal,
            "bar_type.aggregation_source",
            "AggregationSource::Internal",
        )
        .expect(FAILED);

        Self {
            bar_type,
            price_precision,
            size_precision,
            initialized: false,
            ts_last: UnixNanos::default(),
            count: 0,
            partial_set: false,
            last_close: None,
            open: None,
            high: None,
            low: None,
            close: None,
            volume: Quantity::zero(size_precision),
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

    pub fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        if ts_init < self.ts_last {
            return; // Not applicable
        }

        if self.open.is_none() {
            self.open = Some(bar.open);
            self.high = Some(bar.high);
            self.low = Some(bar.low);
            self.initialized = true;
        } else {
            if bar.high > self.high.unwrap() {
                self.high = Some(bar.high);
            }
            if bar.low < self.low.unwrap() {
                self.low = Some(bar.low);
            }
        }

        self.close = Some(bar.close);
        self.volume = self.volume.add(volume);
        self.count += 1;
        self.ts_last = ts_init;
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

        if let (Some(close), Some(low)) = (self.close, self.low) {
            if close < low {
                self.low = Some(close);
            }
        }

        if let (Some(close), Some(high)) = (self.close, self.high) {
            if close > high {
                self.high = Some(close);
            }
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
    handler_backup: Option<H>,
    batch_handler: Option<Box<dyn FnMut(Bar)>>,
    await_partial: bool,
    is_running: bool,
    batch_mode: bool,
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
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
        await_partial: bool,
    ) -> Self {
        Self {
            bar_type,
            builder: BarBuilder::new(bar_type, price_precision, size_precision),
            handler,
            handler_backup: None,
            batch_handler: None,
            await_partial,
            is_running: false,
            batch_mode: false,
        }
    }

    pub const fn set_await_partial(&mut self, value: bool) {
        self.await_partial = value;
    }

    pub const fn set_is_running(&mut self, value: bool) {
        self.is_running = value;
    }

    pub const fn await_partial(&self) -> bool {
        self.await_partial
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

        if self.batch_mode {
            if let Some(handler) = &mut self.batch_handler {
                handler(bar);
            }
        } else {
            (self.handler)(bar);
        }
    }

    pub fn start_batch_update(&mut self, handler: Box<dyn FnMut(Bar)>) {
        self.batch_mode = true;
        self.batch_handler = Some(handler);
    }

    pub fn stop_batch_update(&mut self) {
        self.batch_mode = false;

        if let Some(handler) = self.handler_backup.take() {
            self.handler = handler;
        }
    }
}

/// Provides a means of building tick bars aggregated from quote and trades.
///
/// When received tick count reaches the step threshold of the bar
/// specification, then a bar is created and sent to the handler.
pub struct TickBarAggregator<H>
where
    H: FnMut(Bar),
{
    core: BarAggregatorCore<H>,
    cum_value: f64,
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
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
        await_partial: bool,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(
                bar_type,
                price_precision,
                size_precision,
                handler,
                await_partial,
            ),
            cum_value: 0.0,
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

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_await_partial(&mut self, value: bool) {
        self.core.set_await_partial(value);
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    fn await_partial(&self) -> bool {
        self.core.await_partial()
    }

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        self.core.apply_update(price, size, ts_event);
        let spec = self.core.bar_type.spec();

        if self.core.builder.count >= spec.step.get() {
            self.core.build_now_and_send();
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        let mut volume_update = volume;
        let average_price = Price::new(
            (bar.high.as_f64() + bar.low.as_f64() + bar.close.as_f64()) / 3.0,
            self.core.builder.price_precision,
        );

        while volume_update.as_f64() > 0.0 {
            let value_update = average_price.as_f64() * volume_update.as_f64();
            if self.cum_value + value_update < self.core.bar_type.spec().step.get() as f64 {
                self.cum_value += value_update;
                self.core.builder.update_bar(bar, volume_update, ts_init);
                break;
            }

            let value_diff = self.core.bar_type.spec().step.get() as f64 - self.cum_value;
            let volume_diff = volume_update.as_f64() * (value_diff / value_update);
            self.core.builder.update_bar(
                bar,
                Quantity::new(volume_diff, volume_update.precision),
                ts_init,
            );

            self.core.build_now_and_send();
            self.cum_value = 0.0;
            volume_update = Quantity::new(
                volume_update.as_f64() - volume_diff,
                volume_update.precision,
            );
        }
    }

    fn start_batch_update(&mut self, handler: Box<dyn FnMut(Bar)>, _: UnixNanos) {
        self.core.start_batch_update(handler);
    }

    fn stop_batch_update(&mut self) {
        self.core.stop_batch_update();
    }

    fn set_partial(&mut self, partial_bar: Bar) {
        self.core.set_partial(partial_bar);
    }
}

/// Provides a means of building volume bars aggregated from quote and trades.
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
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
        await_partial: bool,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(
                bar_type.standard(),
                price_precision,
                size_precision,
                handler,
                await_partial,
            ),
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

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_await_partial(&mut self, value: bool) {
        self.core.set_await_partial(value);
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    fn await_partial(&self) -> bool {
        self.core.await_partial()
    }

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        let mut raw_size_update = size.raw;
        let spec = self.core.bar_type.spec();
        let raw_step = (spec.step.get() as f64 * FIXED_SCALAR) as QuantityRaw;

        while raw_size_update > 0 {
            if self.core.builder.volume.raw + raw_size_update < raw_step {
                self.core.apply_update(
                    price,
                    Quantity::from_raw(raw_size_update, size.precision),
                    ts_event,
                );
                break;
            }

            let raw_size_diff = raw_step - self.core.builder.volume.raw;
            self.core.apply_update(
                price,
                Quantity::from_raw(raw_size_diff, size.precision),
                ts_event,
            );

            self.core.build_now_and_send();
            raw_size_update -= raw_size_diff;
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        let mut raw_volume_update = volume.raw;
        let spec = self.core.bar_type.spec();
        let raw_step = (spec.step.get() as f64 * FIXED_SCALAR) as QuantityRaw;

        while raw_volume_update > 0 {
            if self.core.builder.volume.raw + raw_volume_update < raw_step {
                self.core.builder.update_bar(
                    bar,
                    Quantity::from_raw(raw_volume_update, volume.precision),
                    ts_init,
                );
                break;
            }

            let raw_volume_diff = raw_step - self.core.builder.volume.raw;
            self.core.builder.update_bar(
                bar,
                Quantity::from_raw(raw_volume_diff, volume.precision),
                ts_init,
            );

            self.core.build_now_and_send();
            raw_volume_update -= raw_volume_diff;
        }
    }

    fn start_batch_update(&mut self, handler: Box<dyn FnMut(Bar)>, _: UnixNanos) {
        self.core.start_batch_update(handler);
    }

    fn stop_batch_update(&mut self) {
        self.core.stop_batch_update();
    }

    fn set_partial(&mut self, partial_bar: Bar) {
        self.core.set_partial(partial_bar);
    }
}

/// Provides a means of building value bars aggregated from quote and trades.
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
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
        await_partial: bool,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(
                bar_type.standard(),
                price_precision,
                size_precision,
                handler,
                await_partial,
            ),
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

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_await_partial(&mut self, value: bool) {
        self.core.set_await_partial(value);
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    fn await_partial(&self) -> bool {
        self.core.await_partial()
    }

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        let mut size_update = size.as_f64();
        let spec = self.core.bar_type.spec();

        while size_update > 0.0 {
            let value_update = price.as_f64() * size_update;
            if self.cum_value + value_update < spec.step.get() as f64 {
                self.cum_value += value_update;
                self.core
                    .apply_update(price, Quantity::new(size_update, size.precision), ts_event);
                break;
            }

            let value_diff = spec.step.get() as f64 - self.cum_value;
            let size_diff = size_update * (value_diff / value_update);
            self.core
                .apply_update(price, Quantity::new(size_diff, size.precision), ts_event);

            self.core.build_now_and_send();
            self.cum_value = 0.0;
            size_update -= size_diff;
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        let mut volume_update = volume;
        let average_price = Price::new(
            (bar.high.as_f64() + bar.low.as_f64() + bar.close.as_f64()) / 3.0,
            self.core.builder.price_precision,
        );

        while volume_update.as_f64() > 0.0 {
            let value_update = average_price.as_f64() * volume_update.as_f64();
            if self.cum_value + value_update < self.core.bar_type.spec().step.get() as f64 {
                self.cum_value += value_update;
                self.core.builder.update_bar(bar, volume_update, ts_init);
                break;
            }

            let value_diff = self.core.bar_type.spec().step.get() as f64 - self.cum_value;
            let volume_diff = volume_update.as_f64() * (value_diff / value_update);
            self.core.builder.update_bar(
                bar,
                Quantity::new(volume_diff, volume_update.precision),
                ts_init,
            );

            self.core.build_now_and_send();
            self.cum_value = 0.0;
            volume_update = Quantity::new(
                volume_update.as_f64() - volume_diff,
                volume_update.precision,
            );
        }
    }

    fn start_batch_update(&mut self, handler: Box<dyn FnMut(Bar)>, _: UnixNanos) {
        self.core.start_batch_update(handler);
    }

    fn stop_batch_update(&mut self) {
        self.core.stop_batch_update();
    }

    fn set_partial(&mut self, partial_bar: Bar) {
        self.core.set_partial(partial_bar);
    }
}

/// Provides a means of building time bars aggregated from quote and trades.
///
/// At each aggregation time interval, a bar is created and sent to the handler.
pub struct TimeBarAggregator<H>
where
    H: FnMut(Bar),
{
    core: BarAggregatorCore<H>,
    clock: Rc<RefCell<dyn Clock>>,
    build_with_no_updates: bool,
    timestamp_on_close: bool,
    is_left_open: bool,
    build_on_next_tick: bool,
    stored_open_ns: UnixNanos,
    stored_close_ns: UnixNanos,
    timer_name: String,
    interval_ns: UnixNanos,
    next_close_ns: UnixNanos,
    composite_bar_build_delay: i64,
    add_delay: bool,
    batch_open_ns: UnixNanos,
    batch_next_close_ns: UnixNanos,
    time_bars_origin: Option<TimeDelta>,
    skip_first_non_full_bar: bool,
}

#[derive(Clone)]
pub struct NewBarCallback<H: FnMut(Bar)> {
    aggregator: Rc<RefCell<TimeBarAggregator<H>>>,
}

impl<H: FnMut(Bar)> NewBarCallback<H> {
    pub const fn new(aggregator: Rc<RefCell<TimeBarAggregator<H>>>) -> Self {
        Self { aggregator }
    }
}

impl<H: FnMut(Bar) + 'static> From<NewBarCallback<H>> for TimeEventCallback {
    fn from(value: NewBarCallback<H>) -> Self {
        Self::Rust(Rc::new(move |event: TimeEvent| {
            value.aggregator.borrow_mut().build_bar(event);
        }))
    }
}

impl<H> TimeBarAggregator<H>
where
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
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        clock: Rc<RefCell<dyn Clock>>,
        handler: H,
        await_partial: bool,
        build_with_no_updates: bool,
        timestamp_on_close: bool,
        interval_type: BarIntervalType,
        time_bars_origin: Option<TimeDelta>,
        composite_bar_build_delay: i64,
        skip_first_non_full_bar: bool,
    ) -> Self {
        let is_left_open = match interval_type {
            BarIntervalType::LeftOpen => true,
            BarIntervalType::RightOpen => false,
        };

        let add_delay = bar_type.is_composite()
            && bar_type.composite().aggregation_source() == AggregationSource::Internal;

        let core = BarAggregatorCore::new(
            bar_type.standard(),
            price_precision,
            size_precision,
            handler,
            await_partial,
        );

        Self {
            core,
            clock,
            build_with_no_updates,
            timestamp_on_close,
            is_left_open,
            build_on_next_tick: false,
            stored_open_ns: UnixNanos::default(),
            stored_close_ns: UnixNanos::default(),
            timer_name: bar_type.to_string(),
            interval_ns: get_bar_interval_ns(&bar_type),
            next_close_ns: UnixNanos::default(),
            composite_bar_build_delay,
            add_delay,
            batch_open_ns: UnixNanos::default(),
            batch_next_close_ns: UnixNanos::default(),
            time_bars_origin,
            skip_first_non_full_bar,
        }
    }

    /// Starts the time bar aggregator.
    pub fn start(&mut self, callback: NewBarCallback<H>) -> anyhow::Result<()> {
        let now = self.clock.borrow().utc_now();
        let mut start_time = get_time_bar_start(now, &self.bar_type(), self.time_bars_origin);

        if start_time == now {
            self.skip_first_non_full_bar = false;
        }

        if self.add_delay {
            start_time += TimeDelta::microseconds(self.composite_bar_build_delay);
        }

        let spec = &self.bar_type().spec();
        let start_time_ns = UnixNanos::from(start_time);

        if spec.aggregation == BarAggregation::Month {
            let step = spec.step.get() as u32;
            let alert_time_ns = add_n_months_nanos(start_time_ns, step);

            self.clock
                .borrow_mut()
                .set_time_alert_ns(&self.timer_name, alert_time_ns, Some(callback.into()), None)
                .expect(FAILED);
        } else {
            self.clock
                .borrow_mut()
                .set_timer_ns(
                    &self.timer_name,
                    self.interval_ns.as_u64(),
                    start_time_ns,
                    None,
                    Some(callback.into()),
                    None,
                )
                .expect(FAILED);
        }

        log::debug!("Started timer {}", self.timer_name);
        Ok(())
    }

    /// Stops the time bar aggregator.
    pub fn stop(&mut self) {
        self.clock.borrow_mut().cancel_timer(&self.timer_name);
    }

    pub fn start_batch_time(&mut self, time_ns: UnixNanos) {
        let spec = self.bar_type().spec();
        self.core.batch_mode = true;

        let time = time_ns.to_datetime_utc();
        let start_time = get_time_bar_start(time, &self.bar_type(), self.time_bars_origin);
        self.batch_open_ns = UnixNanos::from(start_time);

        if spec.aggregation == BarAggregation::Month {
            let step = spec.step.get() as u32;

            if self.batch_open_ns == time_ns {
                self.batch_open_ns = subtract_n_months_nanos(self.batch_open_ns, step);
            }

            self.batch_next_close_ns = add_n_months_nanos(self.batch_open_ns, step);
        } else {
            if self.batch_open_ns == time_ns {
                self.batch_open_ns -= self.interval_ns;
            }

            self.batch_next_close_ns = self.batch_open_ns + self.interval_ns;
        }
    }

    const fn bar_ts_event(&self, open_ns: UnixNanos, close_ns: UnixNanos) -> UnixNanos {
        if self.is_left_open {
            if self.timestamp_on_close {
                close_ns
            } else {
                open_ns
            }
        } else {
            open_ns
        }
    }

    fn build_and_send(&mut self, ts_event: UnixNanos, ts_init: UnixNanos) {
        if self.skip_first_non_full_bar {
            self.core.builder.reset();
            self.skip_first_non_full_bar = false;
        } else {
            self.core.build_and_send(ts_event, ts_init);
        }
    }

    fn batch_pre_update(&mut self, time_ns: UnixNanos) {
        if time_ns > self.batch_next_close_ns && self.core.builder.initialized {
            let ts_init = self.batch_next_close_ns;
            let ts_event = self.bar_ts_event(self.batch_open_ns, ts_init);
            self.build_and_send(ts_event, ts_init);
        }
    }

    fn batch_post_update(&mut self, time_ns: UnixNanos) {
        let step = self.bar_type().spec().step.get() as u32;

        // If not in batch mode and time matches next close, reset batch close
        if !self.core.batch_mode
            && time_ns == self.batch_next_close_ns
            && time_ns > self.stored_open_ns
        {
            self.batch_next_close_ns = UnixNanos::default();
            return;
        }

        if time_ns > self.batch_next_close_ns {
            // Ensure batch times are coherent with last builder update
            if self.bar_type().spec().aggregation == BarAggregation::Month {
                while self.batch_next_close_ns < time_ns {
                    self.batch_next_close_ns = add_n_months_nanos(self.batch_next_close_ns, step);
                }

                self.batch_open_ns = subtract_n_months_nanos(self.batch_next_close_ns, step);
            } else {
                while self.batch_next_close_ns < time_ns {
                    self.batch_next_close_ns += self.interval_ns;
                }

                self.batch_open_ns = self.batch_next_close_ns - self.interval_ns;
            }
        }

        if time_ns == self.batch_next_close_ns {
            let ts_event = self.bar_ts_event(self.batch_open_ns, self.batch_next_close_ns);
            self.build_and_send(ts_event, time_ns);
            self.batch_open_ns = self.batch_next_close_ns;

            if self.bar_type().spec().aggregation == BarAggregation::Month {
                self.batch_next_close_ns = add_n_months_nanos(self.batch_next_close_ns, step);
            } else {
                self.batch_next_close_ns += self.interval_ns;
            }
        }

        // Delay resetting batch_next_close_ns to allow creating a last historical bar when transitioning to regular bars
        if !self.core.batch_mode {
            self.batch_next_close_ns = UnixNanos::default();
        }
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
        let ts_event = self.bar_ts_event(self.stored_open_ns, ts_init);
        self.build_and_send(ts_event, ts_init);

        self.stored_open_ns = ts_init;

        if self.bar_type().spec().aggregation == BarAggregation::Month {
            let step = self.bar_type().spec().step.get() as u32;
            let next_alert_ns = add_n_months_nanos(ts_init, step);

            self.clock
                .borrow_mut()
                .set_time_alert_ns(&self.timer_name, next_alert_ns, None, None)
                .expect(FAILED);

            self.next_close_ns = next_alert_ns;
        } else {
            self.next_close_ns = self.clock.borrow().next_time_ns(&self.timer_name);
        }
    }
}

impl<H: FnMut(Bar)> BarAggregator for TimeBarAggregator<H>
where
    H: FnMut(Bar) + 'static,
{
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_await_partial(&mut self, value: bool) {
        self.core.set_await_partial(value);
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    fn await_partial(&self) -> bool {
        self.core.await_partial()
    }

    fn update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        if self.batch_next_close_ns != UnixNanos::default() {
            self.batch_pre_update(ts_event);
        }

        self.core.apply_update(price, size, ts_event);

        if self.build_on_next_tick {
            if ts_event <= self.stored_close_ns {
                let ts_init = ts_event;
                let ts_event = self.bar_ts_event(self.stored_open_ns, self.stored_close_ns);
                self.build_and_send(ts_event, ts_init);
            }

            self.build_on_next_tick = false;
            self.stored_close_ns = UnixNanos::default();
        }

        if self.batch_next_close_ns != UnixNanos::default() {
            self.batch_post_update(ts_event);
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        if self.batch_next_close_ns != UnixNanos::default() {
            self.batch_pre_update(ts_init);
        }

        self.core.builder.update_bar(bar, volume, ts_init);

        if self.build_on_next_tick {
            if ts_init <= self.stored_close_ns {
                let ts_event = self.bar_ts_event(self.stored_open_ns, self.stored_close_ns);
                self.build_and_send(ts_event, ts_init);
            }

            // Reset flag and clear stored close
            self.build_on_next_tick = false;
            self.stored_close_ns = UnixNanos::default();
        }

        if self.batch_next_close_ns != UnixNanos::default() {
            self.batch_post_update(ts_init);
        }
    }

    fn start_batch_update(&mut self, handler: Box<dyn FnMut(Bar)>, time_ns: UnixNanos) {
        self.core.start_batch_update(handler);
        self.start_batch_time(time_ns);
    }

    fn stop_batch_update(&mut self) {
        self.core.stop_batch_update();
    }

    fn set_partial(&mut self, partial_bar: Bar) {
        self.core.set_partial(partial_bar);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nautilus_common::clock::TestClock;
    use nautilus_core::UUID4;
    use nautilus_model::{
        data::{BarSpecification, BarType},
        enums::{AggregationSource, BarAggregation, PriceType},
        instruments::{CurrencyPair, Equity, Instrument, InstrumentAny, stubs::*},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_bar_builder_initialization(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let builder = BarBuilder::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
        );

        assert!(!builder.initialized);
        assert_eq!(builder.ts_last, 0);
        assert_eq!(builder.count, 0);
    }

    #[rstest]
    fn test_set_partial_update(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
        );

        let partial_bar = Bar::new(
            bar_type,
            Price::from("101.00"),
            Price::from("102.00"),
            Price::from("100.00"),
            Price::from("101.00"),
            Quantity::from(100),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );

        builder.set_partial(partial_bar);
        let bar = builder.build_now();

        assert_eq!(bar.open, partial_bar.open);
        assert_eq!(bar.high, partial_bar.high);
        assert_eq!(bar.low, partial_bar.low);
        assert_eq!(bar.close, partial_bar.close);
        assert_eq!(bar.volume, partial_bar.volume);
        assert_eq!(builder.ts_last, 2);
    }

    #[rstest]
    fn test_bar_builder_maintains_ohlc_order(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
        );

        builder.update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::from(1000),
        );
        builder.update(
            Price::from("95.00"),
            Quantity::from(1),
            UnixNanos::from(2000),
        );
        builder.update(
            Price::from("105.00"),
            Quantity::from(1),
            UnixNanos::from(3000),
        );

        let bar = builder.build_now();
        assert!(bar.high > bar.low);
        assert_eq!(bar.open, Price::from("100.00"));
        assert_eq!(bar.high, Price::from("105.00"));
        assert_eq!(bar.low, Price::from("95.00"));
        assert_eq!(bar.close, Price::from("105.00"));
    }

    #[rstest]
    fn test_update_ignores_earlier_timestamps(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(100, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
        );

        builder.update(Price::from("1.00000"), Quantity::from(1), 1_000.into());
        builder.update(Price::from("1.00001"), Quantity::from(1), 500.into());

        assert_eq!(builder.ts_last, 1_000);
        assert_eq!(builder.count, 1);
    }

    #[rstest]
    fn test_bar_builder_set_partial_updates_bar_to_expected_properties(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
        );

        let partial_bar = Bar::new(
            bar_type,
            Price::from("1.00001"),
            Price::from("1.00010"),
            Price::from("1.00000"),
            Price::from("1.00002"),
            Quantity::from(1),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        builder.set_partial(partial_bar);
        let bar = builder.build_now();

        assert_eq!(bar.open, Price::from("1.00001"));
        assert_eq!(bar.high, Price::from("1.00010"));
        assert_eq!(bar.low, Price::from("1.00000"));
        assert_eq!(bar.close, Price::from("1.00002"));
        assert_eq!(bar.volume, Quantity::from(1));
        assert_eq!(bar.ts_init, 2_000_000_000);
        assert_eq!(builder.ts_last, 2_000_000_000);
    }

    #[rstest]
    fn test_bar_builder_set_partial_when_already_set_does_not_update(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
        );

        let partial_bar1 = Bar::new(
            bar_type,
            Price::from("1.00001"),
            Price::from("1.00010"),
            Price::from("1.00000"),
            Price::from("1.00002"),
            Quantity::from(1),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_000_000_000),
        );

        let partial_bar2 = Bar::new(
            bar_type,
            Price::from("2.00001"),
            Price::from("2.00010"),
            Price::from("2.00000"),
            Price::from("2.00002"),
            Quantity::from(2),
            UnixNanos::from(3_000_000_000),
            UnixNanos::from(3_000_000_000),
        );

        builder.set_partial(partial_bar1);
        builder.set_partial(partial_bar2);
        let bar = builder.build(
            UnixNanos::from(4_000_000_000),
            UnixNanos::from(4_000_000_000),
        );

        assert_eq!(bar.open, Price::from("1.00001"));
        assert_eq!(bar.high, Price::from("1.00010"));
        assert_eq!(bar.low, Price::from("1.00000"));
        assert_eq!(bar.close, Price::from("1.00002"));
        assert_eq!(bar.volume, Quantity::from(1));
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
        let mut builder = BarBuilder::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
        );

        builder.update(
            Price::from("1.00000"),
            Quantity::from(1),
            UnixNanos::default(),
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
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.update(
            Price::from("1.00000"),
            Quantity::from(1),
            UnixNanos::from(1_000),
        );
        builder.update(
            Price::from("1.00001"),
            Quantity::from(1),
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
        let mut builder = BarBuilder::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
        );

        for _ in 0..5 {
            builder.update(
                Price::from("1.00000"),
                Quantity::from(1),
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
        let mut builder = BarBuilder::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
        );
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
        let mut builder = BarBuilder::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
        );

        builder.update(
            Price::from("1.00001"),
            Quantity::from(2),
            UnixNanos::default(),
        );
        builder.update(
            Price::from("1.00002"),
            Quantity::from(2),
            UnixNanos::default(),
        );
        builder.update(
            Price::from("1.00000"),
            Quantity::from(1),
            UnixNanos::from(1_000_000_000),
        );

        let bar = builder.build_now();

        assert_eq!(bar.open, Price::from("1.00001"));
        assert_eq!(bar.high, Price::from("1.00002"));
        assert_eq!(bar.low, Price::from("1.00000"));
        assert_eq!(bar.close, Price::from("1.00000"));
        assert_eq!(bar.volume, Quantity::from(5));
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
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.update(
            Price::from("1.00001"),
            Quantity::from(1),
            UnixNanos::default(),
        );
        builder.build_now();

        builder.update(
            Price::from("1.00000"),
            Quantity::from(1),
            UnixNanos::default(),
        );
        builder.update(
            Price::from("1.00003"),
            Quantity::from(1),
            UnixNanos::default(),
        );
        builder.update(
            Price::from("1.00002"),
            Quantity::from(1),
            UnixNanos::default(),
        );

        let bar = builder.build_now();

        assert_eq!(bar.open, Price::from("1.00000"));
        assert_eq!(bar.high, Price::from("1.00003"));
        assert_eq!(bar.low, Price::from("1.00000"));
        assert_eq!(bar.close, Price::from("1.00002"));
        assert_eq!(bar.volume, Quantity::from(3));
    }

    #[rstest]
    fn test_tick_bar_aggregator_handle_trade_when_step_count_below_threshold(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(3, BarAggregation::Tick, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
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
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
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

    #[rstest]
    fn test_tick_bar_aggregator_aggregates_to_step_size(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(3, BarAggregation::Tick, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false,
        );

        aggregator.update(
            Price::from("1.00001"),
            Quantity::from(1),
            UnixNanos::default(),
        );
        aggregator.update(
            Price::from("1.00002"),
            Quantity::from(1),
            UnixNanos::from(1000),
        );
        aggregator.update(
            Price::from("1.00003"),
            Quantity::from(1),
            UnixNanos::from(2000),
        );

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 1);

        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.open, Price::from("1.00001"));
        assert_eq!(bar.high, Price::from("1.00003"));
        assert_eq!(bar.low, Price::from("1.00001"));
        assert_eq!(bar.close, Price::from("1.00003"));
        assert_eq!(bar.volume, Quantity::from(3));
    }

    #[rstest]
    fn test_tick_bar_aggregator_resets_after_bar_created(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::Tick, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false,
        );

        aggregator.update(
            Price::from("1.00001"),
            Quantity::from(1),
            UnixNanos::default(),
        );
        aggregator.update(
            Price::from("1.00002"),
            Quantity::from(1),
            UnixNanos::from(1000),
        );
        aggregator.update(
            Price::from("1.00003"),
            Quantity::from(1),
            UnixNanos::from(2000),
        );
        aggregator.update(
            Price::from("1.00004"),
            Quantity::from(1),
            UnixNanos::from(3000),
        );

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 2);

        let bar1 = &handler_guard[0];
        assert_eq!(bar1.open, Price::from("1.00001"));
        assert_eq!(bar1.close, Price::from("1.00002"));
        assert_eq!(bar1.volume, Quantity::from(2));

        let bar2 = &handler_guard[1];
        assert_eq!(bar2.open, Price::from("1.00003"));
        assert_eq!(bar2.close, Price::from("1.00004"));
        assert_eq!(bar2.volume, Quantity::from(2));
    }

    #[rstest]
    fn test_volume_bar_aggregator_builds_multiple_bars_from_large_update(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::Volume, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = VolumeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false,
        );

        aggregator.update(
            Price::from("1.00001"),
            Quantity::from(25),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 2);
        let bar1 = &handler_guard[0];
        assert_eq!(bar1.volume, Quantity::from(10));
        let bar2 = &handler_guard[1];
        assert_eq!(bar2.volume, Quantity::from(10));
    }

    #[rstest]
    fn test_value_bar_aggregator_builds_at_value_threshold(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1000, BarAggregation::Value, PriceType::Last); // $1000 value step
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false,
        );

        // Updates to reach value threshold: 100 * 5 + 100 * 5 = $1000
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(5),
            UnixNanos::default(),
        );
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(5),
            UnixNanos::from(1000),
        );

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 1);
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.volume, Quantity::from(10));
    }

    #[rstest]
    fn test_value_bar_aggregator_handles_large_update(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1000, BarAggregation::Value, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false,
        );

        // Single large update: $100 * 25 = $2500 (should create 2 bars)
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(25),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 2);
        let remaining_value = aggregator.get_cumulative_value();
        assert!(remaining_value < 1000.0); // Should be less than threshold
    }

    #[rstest]
    fn test_time_bar_aggregator_builds_at_interval(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        // One second bars
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false, // await_partial
            true,  // build_with_no_updates
            false, // timestamp_on_close
            BarIntervalType::LeftOpen,
            None,  // time_bars_origin
            15,    // composite_bar_build_delay
            false, // skip_first_non_full_bar
        );

        aggregator.update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::default(),
        );

        let next_sec = UnixNanos::from(1_000_000_000);
        clock.borrow_mut().set_time(next_sec);

        let event = TimeEvent::new(
            Ustr::from("1-SECOND-LAST"),
            UUID4::new(),
            next_sec,
            next_sec,
        );
        aggregator.build_bar(event);

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 1);
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.ts_event, UnixNanos::default());
        assert_eq!(bar.ts_init, next_sec);
    }

    #[rstest]
    fn test_time_bar_aggregator_left_open_interval(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false, // await_partial
            true,  // build_with_no_updates
            true,  // timestamp_on_close - changed to true to verify left-open behavior
            BarIntervalType::LeftOpen,
            None,
            15,
            false, // skip_first_non_full_bar
        );

        // Update in first interval
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::default(),
        );

        // First interval close
        let ts1 = UnixNanos::from(1_000_000_000);
        clock.borrow_mut().set_time(ts1);
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts1, ts1);
        aggregator.build_bar(event);

        // Update in second interval
        aggregator.update(Price::from("101.00"), Quantity::from(1), ts1);

        // Second interval close
        let ts2 = UnixNanos::from(2_000_000_000);
        clock.borrow_mut().set_time(ts2);
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts2, ts2);
        aggregator.build_bar(event);

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 2);

        let bar1 = &handler_guard[0];
        assert_eq!(bar1.ts_event, ts1); // For left-open with timestamp_on_close=true
        assert_eq!(bar1.ts_init, ts1);
        assert_eq!(bar1.close, Price::from("100.00"));
        let bar2 = &handler_guard[1];
        assert_eq!(bar2.ts_event, ts2);
        assert_eq!(bar2.ts_init, ts2);
        assert_eq!(bar2.close, Price::from("101.00"));
    }

    #[rstest]
    fn test_time_bar_aggregator_right_open_interval(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false, // await_partial
            true,  // build_with_no_updates
            true,  // timestamp_on_close
            BarIntervalType::RightOpen,
            None,
            15,
            false, // skip_first_non_full_bar
        );

        // Update in first interval
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::default(),
        );

        // First interval close
        let ts1 = UnixNanos::from(1_000_000_000);
        clock.borrow_mut().set_time(ts1);
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts1, ts1);
        aggregator.build_bar(event);

        // Update in second interval
        aggregator.update(Price::from("101.00"), Quantity::from(1), ts1);

        // Second interval close
        let ts2 = UnixNanos::from(2_000_000_000);
        clock.borrow_mut().set_time(ts2);
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts2, ts2);
        aggregator.build_bar(event);

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 2);

        let bar1 = &handler_guard[0];
        assert_eq!(bar1.ts_event, UnixNanos::default()); // Right-open interval starts inclusive
        assert_eq!(bar1.ts_init, ts1);
        assert_eq!(bar1.close, Price::from("100.00"));

        let bar2 = &handler_guard[1];
        assert_eq!(bar2.ts_event, ts1);
        assert_eq!(bar2.ts_init, ts2);
        assert_eq!(bar2.close, Price::from("101.00"));
    }

    #[rstest]
    fn test_time_bar_aggregator_no_updates_behavior(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        // First test with build_with_no_updates = false
        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false, // await_partial
            false, // build_with_no_updates disabled
            true,  // timestamp_on_close
            BarIntervalType::LeftOpen,
            None,
            15,
            false, // skip_first_non_full_bar
        );

        // No updates, just interval close
        let ts1 = UnixNanos::from(1_000_000_000);
        clock.borrow_mut().set_time(ts1);
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts1, ts1);
        aggregator.build_bar(event);

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 0); // No bar should be built without updates
        drop(handler_guard);

        // Now test with build_with_no_updates = true
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false,
            true, // build_with_no_updates enabled
            true, // timestamp_on_close
            BarIntervalType::LeftOpen,
            None,
            15,
            false, // skip_first_non_full_bar
        );

        aggregator.update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::default(),
        );

        // First interval with update
        let ts1 = UnixNanos::from(1_000_000_000);
        clock.borrow_mut().set_time(ts1);
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts1, ts1);
        aggregator.build_bar(event);

        // Second interval without updates
        let ts2 = UnixNanos::from(2_000_000_000);
        clock.borrow_mut().set_time(ts2);
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts2, ts2);
        aggregator.build_bar(event);

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 2); // Both bars should be built
        let bar1 = &handler_guard[0];
        assert_eq!(bar1.close, Price::from("100.00"));
        let bar2 = &handler_guard[1];
        assert_eq!(bar2.close, Price::from("100.00")); // Should use last close
    }

    #[rstest]
    fn test_time_bar_aggregator_respects_timestamp_on_close(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false, // await_partial
            true,  // build_with_no_updates
            true,  // timestamp_on_close
            BarIntervalType::RightOpen,
            None,
            15,
            false, // skip_first_non_full_bar
        );

        let ts1 = UnixNanos::from(1_000_000_000);
        aggregator.update(Price::from("100.00"), Quantity::from(1), ts1);

        let ts2 = UnixNanos::from(2_000_000_000);
        clock.borrow_mut().set_time(ts2);

        // Simulate timestamp on close
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts2, ts2);
        aggregator.build_bar(event);

        let handler_guard = handler.lock().unwrap();
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.ts_event, UnixNanos::default());
        assert_eq!(bar.ts_init, ts2);
    }

    #[rstest]
    fn test_time_bar_aggregator_batches_updates(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.push(bar);
            },
            false, // await_partial
            true,  // build_with_no_updates
            true,  // timestamp_on_close
            BarIntervalType::LeftOpen,
            None,
            15,
            false, // skip_first_non_full_bar
        );

        let ts1 = UnixNanos::from(1_000_000_000);
        clock.borrow_mut().set_time(ts1);

        let initial_time = clock.borrow().utc_now();
        aggregator.start_batch_time(UnixNanos::from(
            initial_time.timestamp_nanos_opt().unwrap() as u64
        ));

        let handler_guard = handler.lock().unwrap();
        assert_eq!(handler_guard.len(), 0);
    }
}
