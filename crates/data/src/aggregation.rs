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
//!
//! Defines the `BarAggregator` trait and core aggregation types (tick, volume, value, time),
//! along with the `BarBuilder` and `BarAggregatorCore` helpers for constructing bars.

use std::{
    any::Any,
    cell::RefCell,
    fmt::Debug,
    ops::Add,
    rc::{Rc, Weak},
};

use chrono::{Duration, TimeDelta};
use nautilus_common::{
    clock::{Clock, TestClock},
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    UnixNanos,
    correctness::{self, FAILED},
    datetime::{add_n_months, add_n_months_nanos, add_n_years, add_n_years_nanos},
};
use nautilus_model::{
    data::{
        QuoteTick, TradeTick,
        bar::{Bar, BarType, get_bar_interval_ns, get_time_bar_start},
    },
    enums::{AggregationSource, AggressorSide, BarAggregation, BarIntervalType},
    types::{Price, Quantity, fixed::FIXED_SCALAR, price::PriceRaw, quantity::QuantityRaw},
};

/// Type alias for bar handler to reduce type complexity.
type BarHandler = Box<dyn FnMut(Bar)>;

/// Trait for aggregating incoming price and trade events into time-, tick-, volume-, or value-based bars.
///
/// Implementors receive updates and produce completed bars via handlers.
pub trait BarAggregator: Any + Debug {
    /// The [`BarType`] to be aggregated.
    fn bar_type(&self) -> BarType;
    /// If the aggregator is running and will receive data from the message bus.
    fn is_running(&self) -> bool;
    /// Sets the running state of the aggregator (receiving updates when `true`).
    fn set_is_running(&mut self, value: bool);
    /// Updates the aggregator  with the given price and size.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos);
    /// Updates the aggregator with the given quote.
    fn handle_quote(&mut self, quote: QuoteTick) {
        let spec = self.bar_type().spec();
        self.update(
            quote.extract_price(spec.price_type),
            quote.extract_size(spec.price_type),
            quote.ts_init,
        );
    }
    /// Updates the aggregator with the given trade.
    fn handle_trade(&mut self, trade: TradeTick) {
        self.update(trade.price, trade.size, trade.ts_init);
    }
    /// Updates the aggregator with the given bar.
    fn handle_bar(&mut self, bar: Bar) {
        self.update_bar(bar, bar.volume, bar.ts_init);
    }
    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos);
    /// Stop the aggregator, e.g., cancel timers. Default is no-op.
    fn stop(&mut self) {}
    /// Sets historical mode (default implementation does nothing, TimeBarAggregator overrides)
    fn set_historical_mode(&mut self, _historical_mode: bool, _handler: Box<dyn FnMut(Bar)>) {}
    /// Sets historical events (default implementation does nothing, TimeBarAggregator overrides)
    fn set_historical_events(&mut self, _events: Vec<TimeEvent>) {}
    /// Sets clock for time bar aggregators (default implementation does nothing, TimeBarAggregator overrides)
    fn set_clock(&mut self, _clock: Rc<RefCell<dyn Clock>>) {}
    /// Builds a bar from a time event (default implementation does nothing, TimeBarAggregator overrides)
    fn build_bar(&mut self, _event: TimeEvent) {}
    /// Starts the timer for time bar aggregators.
    /// Default implementation does nothing, TimeBarAggregator overrides.
    /// Takes an optional Rc to create weak reference internally.
    fn start_timer(&mut self, _aggregator_rc: Option<Rc<RefCell<Box<dyn BarAggregator>>>>) {}
    /// Sets the weak reference to the aggregator wrapper (for historical mode).
    /// Default implementation does nothing, TimeBarAggregator overrides.
    fn set_aggregator_weak(&mut self, _weak: Weak<RefCell<Box<dyn BarAggregator>>>) {}
}

impl dyn BarAggregator {
    /// Returns a reference to this aggregator as `Any` for downcasting.
    pub fn as_any(&self) -> &dyn Any {
        self
    }
    /// Returns a mutable reference to this aggregator as `Any` for downcasting.
    pub fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Provides a generic bar builder for aggregation.
#[derive(Debug)]
pub struct BarBuilder {
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
    initialized: bool,
    ts_last: UnixNanos,
    count: usize,
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
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    #[must_use]
    pub fn new(bar_type: BarType, price_precision: u8, size_precision: u8) -> Self {
        correctness::check_equal(
            &bar_type.aggregation_source(),
            &AggregationSource::Internal,
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
            last_close: None,
            open: None,
            high: None,
            low: None,
            close: None,
            volume: Quantity::zero(size_precision),
        }
    }

    /// Updates the builder state with the given price, size, and init timestamp.
    ///
    /// # Panics
    ///
    /// Panics if `high` or `low` values are unexpectedly `None` when updating.
    pub fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        if ts_init < self.ts_last {
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
        self.ts_last = ts_init;
    }

    /// Updates the builder state with a completed bar, its volume, and the bar init timestamp.
    ///
    /// # Panics
    ///
    /// Panics if `high` or `low` values are unexpectedly `None` when updating.
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

    /// Returns the aggregated bar for the given timestamps, then resets the builder.
    ///
    /// # Panics
    ///
    /// Panics if `open`, `high`, `low`, or `close` values are `None` when building the bar.
    pub fn build(&mut self, ts_event: UnixNanos, ts_init: UnixNanos) -> Bar {
        if self.open.is_none() {
            self.open = self.last_close;
            self.high = self.last_close;
            self.low = self.last_close;
            self.close = self.last_close;
        }

        if let (Some(close), Some(low)) = (self.close, self.low)
            && close < low
        {
            self.low = Some(close);
        }

        if let (Some(close), Some(high)) = (self.close, self.high)
            && close > high
        {
            self.high = Some(close);
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
pub struct BarAggregatorCore {
    bar_type: BarType,
    builder: BarBuilder,
    handler: BarHandler,
    is_running: bool,
}

impl Debug for BarAggregatorCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BarAggregatorCore))
            .field("bar_type", &self.bar_type)
            .field("builder", &self.builder)
            .field("is_running", &self.is_running)
            .finish()
    }
}

impl BarAggregatorCore {
    /// Creates a new [`BarAggregatorCore`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
    ) -> Self {
        Self {
            bar_type,
            builder: BarBuilder::new(bar_type, price_precision, size_precision),
            handler: Box::new(handler),
            is_running: false,
        }
    }

    /// Sets the running state of the aggregator (receives updates when `true`).
    pub const fn set_is_running(&mut self, value: bool) {
        self.is_running = value;
    }
    fn apply_update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.builder.update(price, size, ts_init);
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

/// Provides a means of building tick bars aggregated from quote and trades.
///
/// When received tick count reaches the step threshold of the bar
/// specification, then a bar is created and sent to the handler.
pub struct TickBarAggregator {
    core: BarAggregatorCore,
}

impl Debug for TickBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(TickBarAggregator))
            .field("core", &self.core)
            .finish()
    }
}

impl TickBarAggregator {
    /// Creates a new [`TickBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(bar_type, price_precision, size_precision, handler),
        }
    }
}

impl BarAggregator for TickBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.apply_update(price, size, ts_init);
        let spec = self.core.bar_type.spec();

        if self.core.builder.count >= spec.step.get() {
            self.core.build_now_and_send();
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        self.core.builder.update_bar(bar, volume, ts_init);
        let spec = self.core.bar_type.spec();

        if self.core.builder.count >= spec.step.get() {
            self.core.build_now_and_send();
        }
    }
}

/// Aggregates bars based on tick buy/sell imbalance.
///
/// Increments imbalance by +1 for buyer-aggressed trades and -1 for seller-aggressed trades.
/// Emits a bar when the absolute imbalance reaches the step threshold.
pub struct TickImbalanceBarAggregator {
    core: BarAggregatorCore,
    imbalance: isize,
}

impl Debug for TickImbalanceBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(TickImbalanceBarAggregator))
            .field("core", &self.core)
            .field("imbalance", &self.imbalance)
            .finish()
    }
}

impl TickImbalanceBarAggregator {
    /// Creates a new [`TickImbalanceBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(bar_type, price_precision, size_precision, handler),
            imbalance: 0,
        }
    }
}

impl BarAggregator for TickImbalanceBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.apply_update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        self.core
            .apply_update(trade.price, trade.size, trade.ts_init);

        let delta = match trade.aggressor_side {
            AggressorSide::Buyer => 1,
            AggressorSide::Seller => -1,
            AggressorSide::NoAggressor => 0,
        };

        if delta == 0 {
            return;
        }

        self.imbalance += delta;
        let threshold = self.core.bar_type.spec().step.get();
        if self.imbalance.unsigned_abs() >= threshold {
            self.core.build_now_and_send();
            self.imbalance = 0;
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        self.core.builder.update_bar(bar, volume, ts_init);
    }
}

/// Aggregates bars based on consecutive buy/sell tick runs.
pub struct TickRunsBarAggregator {
    core: BarAggregatorCore,
    current_run_side: Option<AggressorSide>,
    run_count: usize,
}

impl Debug for TickRunsBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(TickRunsBarAggregator))
            .field("core", &self.core)
            .field("current_run_side", &self.current_run_side)
            .field("run_count", &self.run_count)
            .finish()
    }
}

impl TickRunsBarAggregator {
    /// Creates a new [`TickRunsBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(bar_type, price_precision, size_precision, handler),
            current_run_side: None,
            run_count: 0,
        }
    }
}

impl BarAggregator for TickRunsBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.apply_update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        let side = match trade.aggressor_side {
            AggressorSide::Buyer => Some(AggressorSide::Buyer),
            AggressorSide::Seller => Some(AggressorSide::Seller),
            AggressorSide::NoAggressor => None,
        };

        if let Some(side) = side {
            if self.current_run_side != Some(side) {
                self.current_run_side = Some(side);
                self.run_count = 0;
                self.core.builder.reset();
            }

            self.core
                .apply_update(trade.price, trade.size, trade.ts_init);
            self.run_count += 1;

            let threshold = self.core.bar_type.spec().step.get();
            if self.run_count >= threshold {
                self.core.build_now_and_send();
                self.run_count = 0;
                self.current_run_side = None;
            }
        } else {
            self.core
                .apply_update(trade.price, trade.size, trade.ts_init);
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        self.core.builder.update_bar(bar, volume, ts_init);
    }
}

/// Provides a means of building volume bars aggregated from quote and trades.
pub struct VolumeBarAggregator {
    core: BarAggregatorCore,
}

impl Debug for VolumeBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(VolumeBarAggregator))
            .field("core", &self.core)
            .finish()
    }
}

impl VolumeBarAggregator {
    /// Creates a new [`VolumeBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(
                bar_type.standard(),
                price_precision,
                size_precision,
                handler,
            ),
        }
    }
}

impl BarAggregator for VolumeBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        let mut raw_size_update = size.raw;
        let spec = self.core.bar_type.spec();
        let raw_step = (spec.step.get() as f64 * FIXED_SCALAR) as QuantityRaw;

        while raw_size_update > 0 {
            if self.core.builder.volume.raw + raw_size_update < raw_step {
                self.core.apply_update(
                    price,
                    Quantity::from_raw(raw_size_update, size.precision),
                    ts_init,
                );
                break;
            }

            let raw_size_diff = raw_step - self.core.builder.volume.raw;
            self.core.apply_update(
                price,
                Quantity::from_raw(raw_size_diff, size.precision),
                ts_init,
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
}

/// Aggregates bars based on buy/sell volume imbalance.
pub struct VolumeImbalanceBarAggregator {
    core: BarAggregatorCore,
    imbalance_raw: i128,
    raw_step: i128,
}

impl Debug for VolumeImbalanceBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(VolumeImbalanceBarAggregator))
            .field("core", &self.core)
            .field("imbalance_raw", &self.imbalance_raw)
            .field("raw_step", &self.raw_step)
            .finish()
    }
}

impl VolumeImbalanceBarAggregator {
    /// Creates a new [`VolumeImbalanceBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
    ) -> Self {
        let raw_step = (bar_type.spec().step.get() as f64 * FIXED_SCALAR) as i128;
        Self {
            core: BarAggregatorCore::new(
                bar_type.standard(),
                price_precision,
                size_precision,
                handler,
            ),
            imbalance_raw: 0,
            raw_step,
        }
    }
}

impl BarAggregator for VolumeImbalanceBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.apply_update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        let side = match trade.aggressor_side {
            AggressorSide::Buyer => 1,
            AggressorSide::Seller => -1,
            AggressorSide::NoAggressor => {
                self.core
                    .apply_update(trade.price, trade.size, trade.ts_init);
                return;
            }
        };

        let mut raw_remaining = trade.size.raw as i128;
        while raw_remaining > 0 {
            let imbalance_abs = self.imbalance_raw.abs();
            let needed = (self.raw_step - imbalance_abs).max(1);
            let raw_chunk = raw_remaining.min(needed);
            let qty_chunk = Quantity::from_raw(raw_chunk as QuantityRaw, trade.size.precision);

            self.core
                .apply_update(trade.price, qty_chunk, trade.ts_init);

            self.imbalance_raw += side * raw_chunk;
            raw_remaining -= raw_chunk;

            if self.imbalance_raw.abs() >= self.raw_step {
                self.core.build_now_and_send();
                self.imbalance_raw = 0;
            }
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        self.core.builder.update_bar(bar, volume, ts_init);
    }
}

/// Aggregates bars based on consecutive buy/sell volume runs.
pub struct VolumeRunsBarAggregator {
    core: BarAggregatorCore,
    current_run_side: Option<AggressorSide>,
    run_volume_raw: QuantityRaw,
    raw_step: QuantityRaw,
}

impl Debug for VolumeRunsBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(VolumeRunsBarAggregator))
            .field("core", &self.core)
            .field("current_run_side", &self.current_run_side)
            .field("run_volume_raw", &self.run_volume_raw)
            .field("raw_step", &self.raw_step)
            .finish()
    }
}

impl VolumeRunsBarAggregator {
    /// Creates a new [`VolumeRunsBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
    ) -> Self {
        let raw_step = (bar_type.spec().step.get() as f64 * FIXED_SCALAR) as QuantityRaw;
        Self {
            core: BarAggregatorCore::new(
                bar_type.standard(),
                price_precision,
                size_precision,
                handler,
            ),
            current_run_side: None,
            run_volume_raw: 0,
            raw_step,
        }
    }
}

impl BarAggregator for VolumeRunsBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.apply_update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        let side = match trade.aggressor_side {
            AggressorSide::Buyer => Some(AggressorSide::Buyer),
            AggressorSide::Seller => Some(AggressorSide::Seller),
            AggressorSide::NoAggressor => None,
        };

        let Some(side) = side else {
            self.core
                .apply_update(trade.price, trade.size, trade.ts_init);
            return;
        };

        if self.current_run_side != Some(side) {
            self.current_run_side = Some(side);
            self.run_volume_raw = 0;
            self.core.builder.reset();
        }

        let mut raw_remaining = trade.size.raw;
        while raw_remaining > 0 {
            let needed = self.raw_step.saturating_sub(self.run_volume_raw).max(1);
            let raw_chunk = raw_remaining.min(needed);

            self.core.apply_update(
                trade.price,
                Quantity::from_raw(raw_chunk, trade.size.precision),
                trade.ts_init,
            );

            self.run_volume_raw += raw_chunk;
            raw_remaining -= raw_chunk;

            if self.run_volume_raw >= self.raw_step {
                self.core.build_now_and_send();
                self.run_volume_raw = 0;
                self.current_run_side = None;
            }
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        self.core.builder.update_bar(bar, volume, ts_init);
    }
}

/// Provides a means of building value bars aggregated from quote and trades.
///
/// When received value reaches the step threshold of the bar
/// specification, then a bar is created and sent to the handler.
pub struct ValueBarAggregator {
    core: BarAggregatorCore,
    cum_value: f64,
}

impl Debug for ValueBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ValueBarAggregator))
            .field("core", &self.core)
            .field("cum_value", &self.cum_value)
            .finish()
    }
}

impl ValueBarAggregator {
    /// Creates a new [`ValueBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(
                bar_type.standard(),
                price_precision,
                size_precision,
                handler,
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

impl BarAggregator for ValueBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        let mut size_update = size.as_f64();
        let spec = self.core.bar_type.spec();

        while size_update > 0.0 {
            let value_update = price.as_f64() * size_update;
            if value_update == 0.0 {
                // Prevent division by zero - apply remaining size without triggering bar
                self.core
                    .apply_update(price, Quantity::new(size_update, size.precision), ts_init);
                break;
            }

            if self.cum_value + value_update < spec.step.get() as f64 {
                self.cum_value += value_update;
                self.core
                    .apply_update(price, Quantity::new(size_update, size.precision), ts_init);
                break;
            }

            let value_diff = spec.step.get() as f64 - self.cum_value;
            let size_diff = size_update * (value_diff / value_update);
            self.core
                .apply_update(price, Quantity::new(size_diff, size.precision), ts_init);

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
            if value_update == 0.0 {
                // Prevent division by zero - apply remaining volume without triggering bar
                self.core.builder.update_bar(bar, volume_update, ts_init);
                break;
            }

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
}

/// Aggregates bars based on buy/sell notional imbalance.
pub struct ValueImbalanceBarAggregator {
    core: BarAggregatorCore,
    imbalance_value: f64,
    step_value: f64,
}

impl Debug for ValueImbalanceBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ValueImbalanceBarAggregator))
            .field("core", &self.core)
            .field("imbalance_value", &self.imbalance_value)
            .field("step_value", &self.step_value)
            .finish()
    }
}

impl ValueImbalanceBarAggregator {
    /// Creates a new [`ValueImbalanceBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(
                bar_type.standard(),
                price_precision,
                size_precision,
                handler,
            ),
            imbalance_value: 0.0,
            step_value: bar_type.spec().step.get() as f64,
        }
    }
}

impl BarAggregator for ValueImbalanceBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.apply_update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        let price_f64 = trade.price.as_f64();
        if price_f64 == 0.0 {
            self.core
                .apply_update(trade.price, trade.size, trade.ts_init);
            return;
        }

        let side_sign = match trade.aggressor_side {
            AggressorSide::Buyer => 1.0,
            AggressorSide::Seller => -1.0,
            AggressorSide::NoAggressor => {
                self.core
                    .apply_update(trade.price, trade.size, trade.ts_init);
                return;
            }
        };

        let mut size_remaining = trade.size.as_f64();
        while size_remaining > 0.0 {
            let value_remaining = price_f64 * size_remaining;
            let current_sign = self.imbalance_value.signum();

            if current_sign == 0.0 || current_sign == side_sign {
                let needed = self.step_value - self.imbalance_value.abs();
                if value_remaining <= needed {
                    self.imbalance_value += side_sign * value_remaining;
                    self.core.apply_update(
                        trade.price,
                        Quantity::new(size_remaining, trade.size.precision),
                        trade.ts_init,
                    );
                    break;
                }

                let value_chunk = needed;
                let size_chunk = value_chunk / price_f64;
                self.core.apply_update(
                    trade.price,
                    Quantity::new(size_chunk, trade.size.precision),
                    trade.ts_init,
                );
                self.imbalance_value += side_sign * value_chunk;
                size_remaining -= size_chunk;

                if self.imbalance_value.abs() >= self.step_value {
                    self.core.build_now_and_send();
                    self.imbalance_value = 0.0;
                }
            } else {
                // Opposing side: first neutralize existing imbalance
                let value_to_flatten = self.imbalance_value.abs().min(value_remaining);
                let size_chunk = value_to_flatten / price_f64;
                self.core.apply_update(
                    trade.price,
                    Quantity::new(size_chunk, trade.size.precision),
                    trade.ts_init,
                );
                self.imbalance_value += side_sign * value_to_flatten;
                size_remaining -= size_chunk;
            }
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        self.core.builder.update_bar(bar, volume, ts_init);
    }
}

/// Aggregates bars based on consecutive buy/sell notional runs.
pub struct ValueRunsBarAggregator {
    core: BarAggregatorCore,
    current_run_side: Option<AggressorSide>,
    run_value: f64,
    step_value: f64,
}

impl Debug for ValueRunsBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ValueRunsBarAggregator))
            .field("core", &self.core)
            .field("current_run_side", &self.current_run_side)
            .field("run_value", &self.run_value)
            .field("step_value", &self.step_value)
            .finish()
    }
}

impl ValueRunsBarAggregator {
    /// Creates a new [`ValueRunsBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        handler: H,
    ) -> Self {
        Self {
            core: BarAggregatorCore::new(
                bar_type.standard(),
                price_precision,
                size_precision,
                handler,
            ),
            current_run_side: None,
            run_value: 0.0,
            step_value: bar_type.spec().step.get() as f64,
        }
    }
}

impl BarAggregator for ValueRunsBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.apply_update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        let price_f64 = trade.price.as_f64();
        if price_f64 == 0.0 {
            self.core
                .apply_update(trade.price, trade.size, trade.ts_init);
            return;
        }

        let side = match trade.aggressor_side {
            AggressorSide::Buyer => Some(AggressorSide::Buyer),
            AggressorSide::Seller => Some(AggressorSide::Seller),
            AggressorSide::NoAggressor => None,
        };

        let Some(side) = side else {
            self.core
                .apply_update(trade.price, trade.size, trade.ts_init);
            return;
        };

        if self.current_run_side != Some(side) {
            self.current_run_side = Some(side);
            self.run_value = 0.0;
            self.core.builder.reset();
        }

        let mut size_remaining = trade.size.as_f64();
        while size_remaining > 0.0 {
            let value_update = price_f64 * size_remaining;
            if self.run_value + value_update < self.step_value {
                self.run_value += value_update;
                self.core.apply_update(
                    trade.price,
                    Quantity::new(size_remaining, trade.size.precision),
                    trade.ts_init,
                );
                break;
            }

            let value_needed = self.step_value - self.run_value;
            let size_chunk = value_needed / price_f64;
            self.core.apply_update(
                trade.price,
                Quantity::new(size_chunk, trade.size.precision),
                trade.ts_init,
            );

            self.core.build_now_and_send();
            self.run_value = 0.0;
            self.current_run_side = None;
            size_remaining -= size_chunk;
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        self.core.builder.update_bar(bar, volume, ts_init);
    }
}

/// Provides a means of building Renko bars aggregated from quote and trades.
///
/// Renko bars are created when the price moves by a fixed amount (brick size)
/// regardless of time or volume. Each bar represents a price movement equal
/// to the step size in the bar specification.
pub struct RenkoBarAggregator {
    core: BarAggregatorCore,
    pub brick_size: PriceRaw,
    last_close: Option<Price>,
}

impl Debug for RenkoBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RenkoBarAggregator))
            .field("core", &self.core)
            .field("brick_size", &self.brick_size)
            .field("last_close", &self.last_close)
            .finish()
    }
}

impl RenkoBarAggregator {
    /// Creates a new [`RenkoBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        handler: H,
    ) -> Self {
        // Calculate brick size in raw price units (step * price_increment.raw)
        let brick_size = bar_type.spec().step.get() as PriceRaw * price_increment.raw;

        Self {
            core: BarAggregatorCore::new(
                bar_type.standard(),
                price_precision,
                size_precision,
                handler,
            ),
            brick_size,
            last_close: None,
        }
    }
}

impl BarAggregator for RenkoBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Apply the given update to the aggregator.
    ///
    /// For Renko bars, we check if the price movement from the last close
    /// is greater than or equal to the brick size. If so, we create new bars.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        // Always update the builder with the current tick
        self.core.apply_update(price, size, ts_init);

        // Initialize last_close if this is the first update
        if self.last_close.is_none() {
            self.last_close = Some(price);
            return;
        }

        let last_close = self.last_close.unwrap();

        // Convert prices to raw units (integers) to avoid floating point precision issues
        let current_raw = price.raw;
        let last_close_raw = last_close.raw;
        let price_diff_raw = current_raw - last_close_raw;
        let abs_price_diff_raw = price_diff_raw.abs();

        // Check if we need to create one or more Renko bars
        if abs_price_diff_raw >= self.brick_size {
            let num_bricks = (abs_price_diff_raw / self.brick_size) as usize;
            let direction = if price_diff_raw > 0 { 1.0 } else { -1.0 };
            let mut current_close = last_close;

            // Store the current builder volume to distribute across bricks
            let total_volume = self.core.builder.volume;

            for _i in 0..num_bricks {
                // Calculate the close price for this brick using raw price units
                let brick_close_raw = current_close.raw + (direction as PriceRaw) * self.brick_size;
                let brick_close = Price::from_raw(brick_close_raw, price.precision);

                // For Renko bars: open = previous close, high/low depend on direction
                let (brick_high, brick_low) = if direction > 0.0 {
                    (brick_close, current_close)
                } else {
                    (current_close, brick_close)
                };

                // Reset builder for this brick
                self.core.builder.reset();
                self.core.builder.open = Some(current_close);
                self.core.builder.high = Some(brick_high);
                self.core.builder.low = Some(brick_low);
                self.core.builder.close = Some(brick_close);
                self.core.builder.volume = total_volume; // Each brick gets the full volume
                self.core.builder.count = 1;
                self.core.builder.ts_last = ts_init;
                self.core.builder.initialized = true;

                // Build and send the bar
                self.core.build_and_send(ts_init, ts_init);

                // Update for the next brick
                current_close = brick_close;
                self.last_close = Some(brick_close);
            }
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        // Always update the builder with the current bar
        self.core.builder.update_bar(bar, volume, ts_init);

        // Initialize last_close if this is the first update
        if self.last_close.is_none() {
            self.last_close = Some(bar.close);
            return;
        }

        let last_close = self.last_close.unwrap();

        // Convert prices to raw units (integers) to avoid floating point precision issues
        let current_raw = bar.close.raw;
        let last_close_raw = last_close.raw;
        let price_diff_raw = current_raw - last_close_raw;
        let abs_price_diff_raw = price_diff_raw.abs();

        // Check if we need to create one or more Renko bars
        if abs_price_diff_raw >= self.brick_size {
            let num_bricks = (abs_price_diff_raw / self.brick_size) as usize;
            let direction = if price_diff_raw > 0 { 1.0 } else { -1.0 };
            let mut current_close = last_close;

            // Store the current builder volume to distribute across bricks
            let total_volume = self.core.builder.volume;

            for _i in 0..num_bricks {
                // Calculate the close price for this brick using raw price units
                let brick_close_raw = current_close.raw + (direction as PriceRaw) * self.brick_size;
                let brick_close = Price::from_raw(brick_close_raw, bar.close.precision);

                // For Renko bars: open = previous close, high/low depend on direction
                let (brick_high, brick_low) = if direction > 0.0 {
                    (brick_close, current_close)
                } else {
                    (current_close, brick_close)
                };

                // Reset builder for this brick
                self.core.builder.reset();
                self.core.builder.open = Some(current_close);
                self.core.builder.high = Some(brick_high);
                self.core.builder.low = Some(brick_low);
                self.core.builder.close = Some(brick_close);
                self.core.builder.volume = total_volume; // Each brick gets the full volume
                self.core.builder.count = 1;
                self.core.builder.ts_last = ts_init;
                self.core.builder.initialized = true;

                // Build and send the bar
                self.core.build_and_send(ts_init, ts_init);

                // Update for the next brick
                current_close = brick_close;
                self.last_close = Some(brick_close);
            }
        }
    }
}

/// Provides a means of building time bars aggregated from quote and trades.
///
/// At each aggregation time interval, a bar is created and sent to the handler.
pub struct TimeBarAggregator {
    core: BarAggregatorCore,
    clock: Rc<RefCell<dyn Clock>>,
    build_with_no_updates: bool,
    timestamp_on_close: bool,
    is_left_open: bool,
    stored_open_ns: UnixNanos,
    timer_name: String,
    interval_ns: UnixNanos,
    next_close_ns: UnixNanos,
    bar_build_delay: u64,
    time_bars_origin_offset: Option<TimeDelta>,
    skip_first_non_full_bar: bool,
    pub historical_mode: bool,
    historical_events: Vec<TimeEvent>,
    aggregator_weak: Option<Weak<RefCell<Box<dyn BarAggregator>>>>,
}

impl Debug for TimeBarAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(TimeBarAggregator))
            .field("core", &self.core)
            .field("build_with_no_updates", &self.build_with_no_updates)
            .field("timestamp_on_close", &self.timestamp_on_close)
            .field("is_left_open", &self.is_left_open)
            .field("timer_name", &self.timer_name)
            .field("interval_ns", &self.interval_ns)
            .field("bar_build_delay", &self.bar_build_delay)
            .field("skip_first_non_full_bar", &self.skip_first_non_full_bar)
            .finish()
    }
}

impl TimeBarAggregator {
    /// Creates a new [`TimeBarAggregator`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `instrument.id` is not equal to the `bar_type.instrument_id`.
    /// - `bar_type.aggregation_source` is not equal to `AggregationSource::Internal`.
    #[allow(clippy::too_many_arguments)]
    pub fn new<H: FnMut(Bar) + 'static>(
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        clock: Rc<RefCell<dyn Clock>>,
        handler: H,
        build_with_no_updates: bool,
        timestamp_on_close: bool,
        interval_type: BarIntervalType,
        time_bars_origin_offset: Option<TimeDelta>,
        bar_build_delay: u64,
        skip_first_non_full_bar: bool,
    ) -> Self {
        let is_left_open = match interval_type {
            BarIntervalType::LeftOpen => true,
            BarIntervalType::RightOpen => false,
        };

        let core = BarAggregatorCore::new(
            bar_type.standard(),
            price_precision,
            size_precision,
            handler,
        );

        Self {
            core,
            clock,
            build_with_no_updates,
            timestamp_on_close,
            is_left_open,
            stored_open_ns: UnixNanos::default(),
            timer_name: bar_type.to_string(),
            interval_ns: get_bar_interval_ns(&bar_type),
            next_close_ns: UnixNanos::default(),
            bar_build_delay,
            time_bars_origin_offset,
            skip_first_non_full_bar,
            historical_mode: false,
            historical_events: Vec::new(),
            aggregator_weak: None,
        }
    }

    /// Sets the clock for the aggregator (internal method).
    pub fn set_clock_internal(&mut self, clock: Rc<RefCell<dyn Clock>>) {
        self.clock = clock;
    }

    /// Starts the time bar aggregator, scheduling periodic bar builds on the clock.
    ///
    /// This matches the Cython `start_timer()` method exactly.
    /// Creates a callback to `build_bar` using a weak reference to the aggregator.
    ///
    /// # Panics
    ///
    /// Panics if aggregator_rc is None and aggregator_weak hasn't been set, or if timer registration fails.
    pub fn start_timer_internal(
        &mut self,
        aggregator_rc: Option<Rc<RefCell<Box<dyn BarAggregator>>>>,
    ) {
        // Create callback that calls build_bar through the weak reference
        let aggregator_weak = if let Some(rc) = aggregator_rc {
            // Store weak reference for future use (e.g., in build_bar for month/year)
            let weak = Rc::downgrade(&rc);
            self.aggregator_weak = Some(weak.clone());
            weak
        } else {
            // Use existing weak reference (for historical mode where it was set earlier)
            self.aggregator_weak
                .as_ref()
                .expect("Aggregator weak reference must be set before calling start_timer()")
                .clone()
        };

        let callback = TimeEventCallback::Rust(Rc::new(move |event: TimeEvent| {
            if let Some(agg) = aggregator_weak.upgrade() {
                agg.borrow_mut().build_bar(event);
            }
        }));

        // Computing start_time
        let now = self.clock.borrow().utc_now();
        let mut start_time =
            get_time_bar_start(now, &self.bar_type(), self.time_bars_origin_offset);
        start_time += TimeDelta::microseconds(self.bar_build_delay as i64);

        // Closing a partial bar at the transition from historical to backtest data
        let fire_immediately = start_time == now;

        self.skip_first_non_full_bar = self.skip_first_non_full_bar && now > start_time;

        let spec = &self.bar_type().spec();
        let start_time_ns = UnixNanos::from(start_time);

        if spec.aggregation != BarAggregation::Month && spec.aggregation != BarAggregation::Year {
            self.clock
                .borrow_mut()
                .set_timer_ns(
                    &self.timer_name,
                    self.interval_ns.as_u64(),
                    Some(start_time_ns),
                    None,
                    Some(callback),
                    Some(true), // allow_past
                    Some(fire_immediately),
                )
                .expect(FAILED);

            if fire_immediately {
                self.next_close_ns = start_time_ns;
            } else {
                let interval_duration = Duration::nanoseconds(self.interval_ns.as_i64());
                self.next_close_ns = UnixNanos::from(start_time + interval_duration);
            }

            self.stored_open_ns = self.next_close_ns - self.interval_ns;
        } else {
            // The monthly/yearly alert time is defined iteratively at each alert time as there is no regular interval
            let alert_time = if fire_immediately {
                start_time
            } else {
                let step = spec.step.get() as u32;
                if spec.aggregation == BarAggregation::Month {
                    add_n_months(start_time, step).expect(FAILED)
                } else {
                    // Year aggregation
                    add_n_years(start_time, step).expect(FAILED)
                }
            };

            self.clock
                .borrow_mut()
                .set_time_alert_ns(
                    &self.timer_name,
                    UnixNanos::from(alert_time),
                    Some(callback),
                    Some(true), // allow_past
                )
                .expect(FAILED);

            self.next_close_ns = UnixNanos::from(alert_time);
            self.stored_open_ns = UnixNanos::from(start_time);
        }

        log::debug!(
            "Started timer {}, start_time={:?}, historical_mode={}, fire_immediately={}, now={:?}, bar_build_delay={}",
            self.timer_name,
            start_time,
            self.historical_mode,
            fire_immediately,
            now,
            self.bar_build_delay
        );
    }

    /// Stops the time bar aggregator.
    pub fn stop(&mut self) {
        self.clock.borrow_mut().cancel_timer(&self.timer_name);
    }

    fn build_and_send(&mut self, ts_event: UnixNanos, ts_init: UnixNanos) {
        if self.skip_first_non_full_bar {
            self.core.builder.reset();
            self.skip_first_non_full_bar = false;
        } else {
            self.core.build_and_send(ts_event, ts_init);
        }
    }

    fn build_bar(&mut self, event: TimeEvent) {
        if !self.core.builder.initialized {
            return;
        }

        if !self.build_with_no_updates && self.core.builder.count == 0 {
            return; // Do not build bar when no update
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

        self.build_and_send(ts_event, ts_init);

        // Close time becomes the next open time
        self.stored_open_ns = event.ts_event;

        if self.bar_type().spec().aggregation == BarAggregation::Month {
            let step = self.bar_type().spec().step.get() as u32;
            let alert_time_ns = add_n_months_nanos(event.ts_event, step).expect(FAILED);

            self.clock
                .borrow_mut()
                .set_time_alert_ns(&self.timer_name, alert_time_ns, None, None)
                .expect(FAILED);

            self.next_close_ns = alert_time_ns;
        } else if self.bar_type().spec().aggregation == BarAggregation::Year {
            let step = self.bar_type().spec().step.get() as u32;
            let alert_time_ns = add_n_years_nanos(event.ts_event, step).expect(FAILED);

            self.clock
                .borrow_mut()
                .set_time_alert_ns(&self.timer_name, alert_time_ns, None, None)
                .expect(FAILED);

            self.next_close_ns = alert_time_ns;
        } else {
            // On receiving this event, timer should now have a new `next_time_ns`
            self.next_close_ns = self
                .clock
                .borrow()
                .next_time_ns(&self.timer_name)
                .unwrap_or_default();
        }
    }

    fn preprocess_historical_events(&mut self, ts_init: UnixNanos) {
        if self.clock.borrow().timestamp_ns() == UnixNanos::default() {
            // In historical mode, clock is always a TestClock (set by data engine)
            {
                let mut clock_borrow = self.clock.borrow_mut();
                let test_clock = clock_borrow
                    .as_any_mut()
                    .downcast_mut::<TestClock>()
                    .expect("Expected TestClock in historical mode");
                test_clock.set_time(ts_init);
            }
            // In historical mode, weak reference should already be set
            self.start_timer_internal(None);
        }

        // Advance this aggregator's independent clock and collect timer events
        {
            let mut clock_borrow = self.clock.borrow_mut();
            let test_clock = clock_borrow
                .as_any_mut()
                .downcast_mut::<TestClock>()
                .expect("Expected TestClock in historical mode");
            self.historical_events = test_clock.advance_time(ts_init, true);
        }
    }

    fn postprocess_historical_events(&mut self, _ts_init: UnixNanos) {
        // Process timer events after data processing
        // Collect events first to avoid borrow checker issues
        let events: Vec<TimeEvent> = self.historical_events.drain(..).collect();
        for event in events {
            self.build_bar(event);
        }
    }

    /// Sets historical events (called by data engine after advancing clock)
    pub fn set_historical_events_internal(&mut self, events: Vec<TimeEvent>) {
        self.historical_events = events;
    }
}

impl BarAggregator for TimeBarAggregator {
    fn bar_type(&self) -> BarType {
        self.core.bar_type
    }

    fn is_running(&self) -> bool {
        self.core.is_running
    }

    fn set_is_running(&mut self, value: bool) {
        self.core.set_is_running(value);
    }

    /// Stop time-based aggregator by canceling its timer.
    fn stop(&mut self) {
        Self::stop(self);
    }

    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        if self.historical_mode {
            self.preprocess_historical_events(ts_init);
        }

        self.core.apply_update(price, size, ts_init);

        if self.historical_mode {
            self.postprocess_historical_events(ts_init);
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        if self.historical_mode {
            self.preprocess_historical_events(ts_init);
        }

        self.core.builder.update_bar(bar, volume, ts_init);

        if self.historical_mode {
            self.postprocess_historical_events(ts_init);
        }
    }

    fn set_historical_mode(&mut self, historical_mode: bool, handler: Box<dyn FnMut(Bar)>) {
        self.historical_mode = historical_mode;
        self.core.handler = handler;
    }

    fn set_historical_events(&mut self, events: Vec<TimeEvent>) {
        self.set_historical_events_internal(events);
    }

    fn set_clock(&mut self, clock: Rc<RefCell<dyn Clock>>) {
        self.set_clock_internal(clock);
    }

    fn build_bar(&mut self, event: TimeEvent) {
        // Delegate to the implementation method
        // We use the struct name here to disambiguate from the trait method
        {
            #[allow(clippy::use_self)]
            TimeBarAggregator::build_bar(self, event);
        }
    }

    fn set_aggregator_weak(&mut self, weak: Weak<RefCell<Box<dyn BarAggregator>>>) {
        self.aggregator_weak = Some(weak);
    }

    fn start_timer(&mut self, aggregator_rc: Option<Rc<RefCell<Box<dyn BarAggregator>>>>) {
        self.start_timer_internal(aggregator_rc);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nautilus_common::clock::TestClock;
    use nautilus_core::{MUTEX_POISONED, UUID4};
    use nautilus_model::{
        data::{BarSpecification, BarType},
        enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let trade = TradeTick::default();
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let trade = TradeTick::default();
        aggregator.handle_trade(trade);
        aggregator.handle_trade(trade);
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
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

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
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

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
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
    fn test_tick_imbalance_bar_aggregator_emits_at_threshold(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let trade = TradeTick::default();
        aggregator.handle_trade(trade);
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.volume, Quantity::from(200000));
    }

    #[rstest]
    fn test_tick_imbalance_bar_aggregator_handles_seller_direction(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::TickImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let sell = TradeTick {
            aggressor_side: AggressorSide::Seller,
            ..TradeTick::default()
        };

        aggregator.handle_trade(sell);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
    }

    #[rstest]
    fn test_tick_runs_bar_aggregator_resets_on_side_change(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick::default();
        let sell = TradeTick {
            aggressor_side: AggressorSide::Seller,
            ..buy
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);
        aggregator.handle_trade(sell);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_tick_runs_bar_aggregator_volume_conservation(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick {
            size: Quantity::from(1),
            ..TradeTick::default()
        };
        let sell = TradeTick {
            aggressor_side: AggressorSide::Seller,
            size: Quantity::from(1),
            ..buy
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);
        aggregator.handle_trade(sell);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
        assert_eq!(handler_guard[0].volume, Quantity::from(2));
        assert_eq!(handler_guard[1].volume, Quantity::from(2));
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        aggregator.update(
            Price::from("1.00001"),
            Quantity::from(25),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
        let bar1 = &handler_guard[0];
        assert_eq!(bar1.volume, Quantity::from(10));
        let bar2 = &handler_guard[1];
        assert_eq!(bar2.volume, Quantity::from(10));
    }

    #[rstest]
    fn test_volume_runs_bar_aggregator_side_change_resets(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::VolumeRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = VolumeRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick {
            instrument_id: instrument.id(),
            price: Price::from("1.0"),
            size: Quantity::from(1),
            ..TradeTick::default()
        };
        let sell = TradeTick {
            aggressor_side: AggressorSide::Seller,
            ..buy
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(buy); // emit first bar at 2
        aggregator.handle_trade(sell);
        aggregator.handle_trade(sell); // emit second bar at 2 sell-side

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert!(handler_guard.len() >= 2);
        assert!(
            (handler_guard[0].volume.as_f64() - handler_guard[1].volume.as_f64()).abs()
                < f64::EPSILON
        );
    }

    #[rstest]
    fn test_volume_runs_bar_aggregator_handles_large_single_trade(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(3, BarAggregation::VolumeRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = VolumeRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let trade = TradeTick {
            instrument_id: instrument.id(),
            price: Price::from("1.0"),
            size: Quantity::from(5),
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert!(!handler_guard.is_empty());
        assert!(handler_guard[0].volume.as_f64() > 0.0);
        assert!(handler_guard[0].volume.as_f64() < trade.size.as_f64());
    }

    #[rstest]
    fn test_volume_imbalance_bar_aggregator_splits_large_trade(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::VolumeImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = VolumeImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let trade_small = TradeTick {
            instrument_id: instrument.id(),
            price: Price::from("1.0"),
            size: Quantity::from(1),
            ..TradeTick::default()
        };
        let trade_large = TradeTick {
            size: Quantity::from(3),
            ..trade_small
        };

        aggregator.handle_trade(trade_small);
        aggregator.handle_trade(trade_large);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
        let total_output = handler_guard
            .iter()
            .map(|bar| bar.volume.as_f64())
            .sum::<f64>();
        let total_input = trade_small.size.as_f64() + trade_large.size.as_f64();
        assert!((total_output - total_input).abs() < f64::EPSILON);
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
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

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // Single large update: $100 * 25 = $2500 (should create 2 bars)
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(25),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
        let remaining_value = aggregator.get_cumulative_value();
        assert!(remaining_value < 1000.0); // Should be less than threshold
    }

    #[rstest]
    fn test_value_bar_aggregator_handles_zero_price(equity_aapl: Equity) {
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // Update with zero price should not cause division by zero
        aggregator.update(
            Price::from("0.00"),
            Quantity::from(100),
            UnixNanos::default(),
        );

        // No bars should be emitted since value is zero
        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 0);

        // Cumulative value should remain zero
        assert_eq!(aggregator.get_cumulative_value(), 0.0);
    }

    #[rstest]
    fn test_value_bar_aggregator_handles_zero_size(equity_aapl: Equity) {
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // Update with zero size should not cause issues
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(0),
            UnixNanos::default(),
        );

        // No bars should be emitted
        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 0);

        // Cumulative value should remain zero
        assert_eq!(aggregator.get_cumulative_value(), 0.0);
    }

    #[rstest]
    fn test_value_imbalance_bar_aggregator_emits_on_opposing_overflow(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::ValueImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = ValueImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick {
            price: Price::from("5.0"),
            size: Quantity::from(2), // value 10, should emit one bar
            instrument_id: instrument.id(),
            ..TradeTick::default()
        };
        let sell = TradeTick {
            price: Price::from("5.0"),
            size: Quantity::from(2), // value 10, should emit another bar
            aggressor_side: AggressorSide::Seller,
            instrument_id: instrument.id(),
            ..buy
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert!(handler_guard.is_empty());
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_emits_on_consecutive_side(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = ValueRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let trade = TradeTick {
            price: Price::from("10.0"),
            size: Quantity::from(5),
            instrument_id: instrument.id(),
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.volume, Quantity::from(10));
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_resets_on_side_change(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = ValueRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick {
            price: Price::from("10.0"),
            size: Quantity::from(5),
            instrument_id: instrument.id(),
            ..TradeTick::default()
        }; // value 50
        let sell = TradeTick {
            price: Price::from("10.0"),
            size: Quantity::from(10),
            aggressor_side: AggressorSide::Seller,
            ..buy
        }; // value 100

        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].volume, Quantity::from(10));
    }

    #[rstest]
    fn test_tick_runs_bar_aggregator_continues_run_after_bar_emission(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick::default();

        aggregator.handle_trade(buy);
        aggregator.handle_trade(buy); // Emit bar 1 (run complete)
        aggregator.handle_trade(buy); // Start new run
        aggregator.handle_trade(buy); // Emit bar 2 (new run complete)

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_tick_runs_bar_aggregator_handles_no_aggressor_trades(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick::default();
        let no_aggressor = TradeTick {
            aggressor_side: AggressorSide::NoAggressor,
            ..buy
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(no_aggressor); // Should not affect run count
        aggregator.handle_trade(no_aggressor); // Should not affect run count
        aggregator.handle_trade(buy); // Continue run to threshold

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
    }

    #[rstest]
    fn test_volume_runs_bar_aggregator_continues_run_after_bar_emission(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::VolumeRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = VolumeRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick {
            instrument_id: instrument.id(),
            price: Price::from("1.0"),
            size: Quantity::from(1),
            ..TradeTick::default()
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(buy); // Emit bar 1 (2.0 volume reached)
        aggregator.handle_trade(buy); // Start new run
        aggregator.handle_trade(buy); // Emit bar 2 (new 2.0 volume reached)

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
        assert_eq!(handler_guard[0].volume, Quantity::from(2));
        assert_eq!(handler_guard[1].volume, Quantity::from(2));
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_continues_run_after_bar_emission(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = ValueRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick {
            instrument_id: instrument.id(),
            price: Price::from("10.0"),
            size: Quantity::from(5),
            ..TradeTick::default()
        }; // value 50 per trade

        aggregator.handle_trade(buy);
        aggregator.handle_trade(buy); // Emit bar 1 (100 value reached)
        aggregator.handle_trade(buy); // Start new run
        aggregator.handle_trade(buy); // Emit bar 2 (new 100 value reached)

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
        assert_eq!(handler_guard[0].volume, Quantity::from(10));
        assert_eq!(handler_guard[1].volume, Quantity::from(10));
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
            true,  // build_with_no_updates
            false, // timestamp_on_close
            BarIntervalType::LeftOpen,
            None,  // time_bars_origin_offset
            15,    // bar_build_delay
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

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
            true, // build_with_no_updates
            true, // timestamp_on_close - changed to true to verify left-open behavior
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

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
            true, // build_with_no_updates
            true, // timestamp_on_close
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

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
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

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
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

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
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
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
            true, // build_with_no_updates
            true, // timestamp_on_close
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

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.ts_event, UnixNanos::default());
        assert_eq!(bar.ts_init, ts2);
    }

    // ========================================================================
    // RenkoBarAggregator Tests
    // ========================================================================

    #[rstest]
    fn test_renko_bar_aggregator_initialization(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        assert_eq!(aggregator.bar_type(), bar_type);
        assert!(!aggregator.is_running());
        // 10 pips * price_increment.raw (depends on precision mode)
        let expected_brick_size = 10 * instrument.price_increment().raw;
        assert_eq!(aggregator.brick_size, expected_brick_size);
    }

    #[rstest]
    fn test_renko_bar_aggregator_update_below_brick_size_no_bar(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // Small price movement (5 pips, less than 10 pip brick size)
        aggregator.update(
            Price::from("1.00000"),
            Quantity::from(1),
            UnixNanos::default(),
        );
        aggregator.update(
            Price::from("1.00005"),
            Quantity::from(1),
            UnixNanos::from(1000),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 0); // No bar created yet
    }

    #[rstest]
    fn test_renko_bar_aggregator_update_exceeds_brick_size_creates_bar(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // Price movement exceeding brick size (15 pips)
        aggregator.update(
            Price::from("1.00000"),
            Quantity::from(1),
            UnixNanos::default(),
        );
        aggregator.update(
            Price::from("1.00015"),
            Quantity::from(1),
            UnixNanos::from(1000),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);

        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.open, Price::from("1.00000"));
        assert_eq!(bar.high, Price::from("1.00010"));
        assert_eq!(bar.low, Price::from("1.00000"));
        assert_eq!(bar.close, Price::from("1.00010"));
        assert_eq!(bar.volume, Quantity::from(2));
        assert_eq!(bar.ts_event, UnixNanos::from(1000));
        assert_eq!(bar.ts_init, UnixNanos::from(1000));
    }

    #[rstest]
    fn test_renko_bar_aggregator_multiple_bricks_in_one_update(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // Large price movement creating multiple bricks (25 pips = 2 bricks)
        aggregator.update(
            Price::from("1.00000"),
            Quantity::from(1),
            UnixNanos::default(),
        );
        aggregator.update(
            Price::from("1.00025"),
            Quantity::from(1),
            UnixNanos::from(1000),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);

        let bar1 = &handler_guard[0];
        assert_eq!(bar1.open, Price::from("1.00000"));
        assert_eq!(bar1.high, Price::from("1.00010"));
        assert_eq!(bar1.low, Price::from("1.00000"));
        assert_eq!(bar1.close, Price::from("1.00010"));

        let bar2 = &handler_guard[1];
        assert_eq!(bar2.open, Price::from("1.00010"));
        assert_eq!(bar2.high, Price::from("1.00020"));
        assert_eq!(bar2.low, Price::from("1.00010"));
        assert_eq!(bar2.close, Price::from("1.00020"));
    }

    #[rstest]
    fn test_renko_bar_aggregator_downward_movement(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // Start at higher price and move down
        aggregator.update(
            Price::from("1.00020"),
            Quantity::from(1),
            UnixNanos::default(),
        );
        aggregator.update(
            Price::from("1.00005"),
            Quantity::from(1),
            UnixNanos::from(1000),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);

        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.open, Price::from("1.00020"));
        assert_eq!(bar.high, Price::from("1.00020"));
        assert_eq!(bar.low, Price::from("1.00010"));
        assert_eq!(bar.close, Price::from("1.00010"));
        assert_eq!(bar.volume, Quantity::from(2));
    }

    #[rstest]
    fn test_renko_bar_aggregator_handle_bar_below_brick_size(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // Create a bar with small price movement (5 pips)
        let input_bar = Bar::new(
            BarType::new(
                instrument.id(),
                BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid),
                AggregationSource::Internal,
            ),
            Price::from("1.00000"),
            Price::from("1.00005"),
            Price::from("0.99995"),
            Price::from("1.00005"), // 5 pip move up (less than 10 pip brick)
            Quantity::from(100),
            UnixNanos::default(),
            UnixNanos::from(1000),
        );

        aggregator.handle_bar(input_bar);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 0); // No bar created yet
    }

    #[rstest]
    fn test_renko_bar_aggregator_handle_bar_exceeds_brick_size(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // First bar to establish baseline
        let bar1 = Bar::new(
            BarType::new(
                instrument.id(),
                BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid),
                AggregationSource::Internal,
            ),
            Price::from("1.00000"),
            Price::from("1.00005"),
            Price::from("0.99995"),
            Price::from("1.00000"),
            Quantity::from(100),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        // Second bar with price movement exceeding brick size (10 pips)
        let bar2 = Bar::new(
            BarType::new(
                instrument.id(),
                BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid),
                AggregationSource::Internal,
            ),
            Price::from("1.00000"),
            Price::from("1.00015"),
            Price::from("0.99995"),
            Price::from("1.00010"), // 10 pip move up (exactly 1 brick)
            Quantity::from(50),
            UnixNanos::from(60_000_000_000),
            UnixNanos::from(60_000_000_000),
        );

        aggregator.handle_bar(bar1);
        aggregator.handle_bar(bar2);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);

        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.open, Price::from("1.00000"));
        assert_eq!(bar.high, Price::from("1.00010"));
        assert_eq!(bar.low, Price::from("1.00000"));
        assert_eq!(bar.close, Price::from("1.00010"));
        assert_eq!(bar.volume, Quantity::from(150));
    }

    #[rstest]
    fn test_renko_bar_aggregator_handle_bar_multiple_bricks(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // First bar to establish baseline
        let bar1 = Bar::new(
            BarType::new(
                instrument.id(),
                BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid),
                AggregationSource::Internal,
            ),
            Price::from("1.00000"),
            Price::from("1.00005"),
            Price::from("0.99995"),
            Price::from("1.00000"),
            Quantity::from(100),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        // Second bar with large price movement (30 pips = 3 bricks)
        let bar2 = Bar::new(
            BarType::new(
                instrument.id(),
                BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid),
                AggregationSource::Internal,
            ),
            Price::from("1.00000"),
            Price::from("1.00035"),
            Price::from("0.99995"),
            Price::from("1.00030"), // 30 pip move up (exactly 3 bricks)
            Quantity::from(50),
            UnixNanos::from(60_000_000_000),
            UnixNanos::from(60_000_000_000),
        );

        aggregator.handle_bar(bar1);
        aggregator.handle_bar(bar2);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 3);

        let bar1 = &handler_guard[0];
        assert_eq!(bar1.open, Price::from("1.00000"));
        assert_eq!(bar1.close, Price::from("1.00010"));

        let bar2 = &handler_guard[1];
        assert_eq!(bar2.open, Price::from("1.00010"));
        assert_eq!(bar2.close, Price::from("1.00020"));

        let bar3 = &handler_guard[2];
        assert_eq!(bar3.open, Price::from("1.00020"));
        assert_eq!(bar3.close, Price::from("1.00030"));
    }

    #[rstest]
    fn test_renko_bar_aggregator_handle_bar_downward_movement(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // First bar to establish baseline
        let bar1 = Bar::new(
            BarType::new(
                instrument.id(),
                BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid),
                AggregationSource::Internal,
            ),
            Price::from("1.00020"),
            Price::from("1.00025"),
            Price::from("1.00015"),
            Price::from("1.00020"),
            Quantity::from(100),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        // Second bar with downward price movement (10 pips down)
        let bar2 = Bar::new(
            BarType::new(
                instrument.id(),
                BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid),
                AggregationSource::Internal,
            ),
            Price::from("1.00020"),
            Price::from("1.00025"),
            Price::from("1.00005"),
            Price::from("1.00010"), // 10 pip move down (exactly 1 brick)
            Quantity::from(50),
            UnixNanos::from(60_000_000_000),
            UnixNanos::from(60_000_000_000),
        );

        aggregator.handle_bar(bar1);
        aggregator.handle_bar(bar2);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);

        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.open, Price::from("1.00020"));
        assert_eq!(bar.high, Price::from("1.00020"));
        assert_eq!(bar.low, Price::from("1.00010"));
        assert_eq!(bar.close, Price::from("1.00010"));
        assert_eq!(bar.volume, Quantity::from(150));
    }

    #[rstest]
    fn test_renko_bar_aggregator_brick_size_calculation(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);

        // Test different brick sizes
        let bar_spec_5 = BarSpecification::new(5, BarAggregation::Renko, PriceType::Mid); // 5 pip brick size
        let bar_type_5 = BarType::new(instrument.id(), bar_spec_5, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let aggregator_5 = RenkoBarAggregator::new(
            bar_type_5,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |_bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(_bar);
            },
        );

        // 5 pips * price_increment.raw (depends on precision mode)
        let expected_brick_size_5 = 5 * instrument.price_increment().raw;
        assert_eq!(aggregator_5.brick_size, expected_brick_size_5);

        let bar_spec_20 = BarSpecification::new(20, BarAggregation::Renko, PriceType::Mid); // 20 pip brick size
        let bar_type_20 = BarType::new(instrument.id(), bar_spec_20, AggregationSource::Internal);
        let handler2 = Arc::new(Mutex::new(Vec::new()));
        let handler2_clone = Arc::clone(&handler2);

        let aggregator_20 = RenkoBarAggregator::new(
            bar_type_20,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |_bar: Bar| {
                let mut handler_guard = handler2_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(_bar);
            },
        );

        // 20 pips * price_increment.raw (depends on precision mode)
        let expected_brick_size_20 = 20 * instrument.price_increment().raw;
        assert_eq!(aggregator_20.brick_size, expected_brick_size_20);
    }

    #[rstest]
    fn test_renko_bar_aggregator_sequential_updates(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // Sequential updates creating multiple bars
        aggregator.update(
            Price::from("1.00000"),
            Quantity::from(1),
            UnixNanos::from(1000),
        );
        aggregator.update(
            Price::from("1.00010"),
            Quantity::from(1),
            UnixNanos::from(2000),
        ); // First brick
        aggregator.update(
            Price::from("1.00020"),
            Quantity::from(1),
            UnixNanos::from(3000),
        ); // Second brick
        aggregator.update(
            Price::from("1.00025"),
            Quantity::from(1),
            UnixNanos::from(4000),
        ); // Partial third brick
        aggregator.update(
            Price::from("1.00030"),
            Quantity::from(1),
            UnixNanos::from(5000),
        ); // Complete third brick

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 3);

        let bar1 = &handler_guard[0];
        assert_eq!(bar1.open, Price::from("1.00000"));
        assert_eq!(bar1.close, Price::from("1.00010"));

        let bar2 = &handler_guard[1];
        assert_eq!(bar2.open, Price::from("1.00010"));
        assert_eq!(bar2.close, Price::from("1.00020"));

        let bar3 = &handler_guard[2];
        assert_eq!(bar3.open, Price::from("1.00020"));
        assert_eq!(bar3.close, Price::from("1.00030"));
    }

    #[rstest]
    fn test_renko_bar_aggregator_mixed_direction_movement(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        // Mixed direction movement: up then down
        aggregator.update(
            Price::from("1.00000"),
            Quantity::from(1),
            UnixNanos::from(1000),
        );
        aggregator.update(
            Price::from("1.00010"),
            Quantity::from(1),
            UnixNanos::from(2000),
        ); // Up brick
        aggregator.update(
            Price::from("0.99990"),
            Quantity::from(1),
            UnixNanos::from(3000),
        ); // Down 2 bricks (20 pips)

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 3);

        let bar1 = &handler_guard[0]; // Up brick
        assert_eq!(bar1.open, Price::from("1.00000"));
        assert_eq!(bar1.high, Price::from("1.00010"));
        assert_eq!(bar1.low, Price::from("1.00000"));
        assert_eq!(bar1.close, Price::from("1.00010"));

        let bar2 = &handler_guard[1]; // First down brick
        assert_eq!(bar2.open, Price::from("1.00010"));
        assert_eq!(bar2.high, Price::from("1.00010"));
        assert_eq!(bar2.low, Price::from("1.00000"));
        assert_eq!(bar2.close, Price::from("1.00000"));

        let bar3 = &handler_guard[2]; // Second down brick
        assert_eq!(bar3.open, Price::from("1.00000"));
        assert_eq!(bar3.high, Price::from("1.00000"));
        assert_eq!(bar3.low, Price::from("0.99990"));
        assert_eq!(bar3.close, Price::from("0.99990"));
    }

    #[rstest]
    fn test_tick_imbalance_bar_aggregator_mixed_trades_cancel_out(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(3, BarAggregation::TickImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick {
            aggressor_side: AggressorSide::Buyer,
            ..TradeTick::default()
        };
        let sell = TradeTick {
            aggressor_side: AggressorSide::Seller,
            ..TradeTick::default()
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);
        aggregator.handle_trade(buy);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 0);
    }

    #[rstest]
    fn test_tick_imbalance_bar_aggregator_no_aggressor_ignored(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick {
            aggressor_side: AggressorSide::Buyer,
            ..TradeTick::default()
        };
        let no_aggressor = TradeTick {
            aggressor_side: AggressorSide::NoAggressor,
            ..TradeTick::default()
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(no_aggressor);
        aggregator.handle_trade(buy);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
    }

    #[rstest]
    fn test_tick_runs_bar_aggregator_multiple_consecutive_runs(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = TickRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick {
            aggressor_side: AggressorSide::Buyer,
            ..TradeTick::default()
        };
        let sell = TradeTick {
            aggressor_side: AggressorSide::Seller,
            ..TradeTick::default()
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);
        aggregator.handle_trade(sell);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_volume_imbalance_bar_aggregator_large_trade_spans_bars(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::VolumeImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = VolumeImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let large_trade = TradeTick {
            size: Quantity::from(25),
            aggressor_side: AggressorSide::Buyer,
            ..TradeTick::default()
        };

        aggregator.handle_trade(large_trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_volume_imbalance_bar_aggregator_no_aggressor_does_not_affect_imbalance(
        equity_aapl: Equity,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::VolumeImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = VolumeImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let buy = TradeTick {
            size: Quantity::from(5),
            aggressor_side: AggressorSide::Buyer,
            ..TradeTick::default()
        };
        let no_aggressor = TradeTick {
            size: Quantity::from(3),
            aggressor_side: AggressorSide::NoAggressor,
            ..TradeTick::default()
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(no_aggressor);
        aggregator.handle_trade(buy);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
    }

    #[rstest]
    fn test_volume_runs_bar_aggregator_large_trade_spans_bars(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::VolumeRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = VolumeRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let large_trade = TradeTick {
            size: Quantity::from(25),
            aggressor_side: AggressorSide::Buyer,
            ..TradeTick::default()
        };

        aggregator.handle_trade(large_trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_large_trade_spans_bars(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(50, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);

        let mut aggregator = ValueRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            move |bar: Bar| {
                let mut handler_guard = handler_clone.lock().expect(MUTEX_POISONED);
                handler_guard.push(bar);
            },
        );

        let large_trade = TradeTick {
            price: Price::from("5.00"),
            size: Quantity::from(25),
            aggressor_side: AggressorSide::Buyer,
            ..TradeTick::default()
        };

        aggregator.handle_trade(large_trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
    }
}
