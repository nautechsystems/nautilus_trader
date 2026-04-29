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

use ahash::AHashMap;
use chrono::{Duration, TimeDelta};
use nautilus_common::{
    clock::{Clock, TestClock},
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    UnixNanos,
    correctness::{self, FAILED},
    datetime::{
        add_n_months, add_n_months_nanos, add_n_years, add_n_years_nanos, subtract_n_months_nanos,
        subtract_n_years_nanos,
    },
};
use nautilus_model::{
    data::{
        QuoteTick, TradeTick,
        bar::{Bar, BarType, get_bar_interval_ns, get_time_bar_start},
    },
    enums::{AggregationSource, AggressorSide, BarAggregation, BarIntervalType},
    identifiers::InstrumentId,
    instruments::{FixedTickScheme, TickSchemeRule},
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
    fn build_bar(&mut self, _event: &TimeEvent) {}
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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

        // The open was checked, so we can assume all prices are Some
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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
            let mut size_diff = size_update * (value_diff / value_update);

            // Clamp to minimum representable size to avoid zero-volume bars
            if is_below_min_size(size_diff, size.precision) {
                if is_below_min_size(size_update, size.precision) {
                    break;
                }
                size_diff = min_size_f64(size.precision);
            }

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
            let mut volume_diff = volume_update.as_f64() * (value_diff / value_update);

            // Clamp to minimum representable size to avoid zero-volume bars
            if is_below_min_size(volume_diff, volume_update.precision) {
                if is_below_min_size(volume_update.as_f64(), volume_update.precision) {
                    break;
                }
                volume_diff = min_size_f64(volume_update.precision);
            }

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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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

            if self.imbalance_value == 0.0 || self.imbalance_value.signum() == side_sign {
                let needed = self.step_value - self.imbalance_value.abs();
                if value_remaining <= needed {
                    self.imbalance_value += side_sign * value_remaining;
                    self.core.apply_update(
                        trade.price,
                        Quantity::new(size_remaining, trade.size.precision),
                        trade.ts_init,
                    );

                    if self.imbalance_value.abs() >= self.step_value {
                        self.core.build_now_and_send();
                        self.imbalance_value = 0.0;
                    }
                    break;
                }

                let mut value_chunk = needed;
                let mut size_chunk = value_chunk / price_f64;

                // Clamp to minimum representable size to avoid zero-volume bars
                if is_below_min_size(size_chunk, trade.size.precision) {
                    if is_below_min_size(size_remaining, trade.size.precision) {
                        break;
                    }
                    size_chunk = min_size_f64(trade.size.precision);
                    value_chunk = price_f64 * size_chunk;
                }

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
                let mut value_to_flatten = self.imbalance_value.abs().min(value_remaining);
                let mut size_chunk = value_to_flatten / price_f64;

                // Clamp to minimum representable size to avoid zero-volume bars
                if is_below_min_size(size_chunk, trade.size.precision) {
                    if is_below_min_size(size_remaining, trade.size.precision) {
                        break;
                    }
                    size_chunk = min_size_f64(trade.size.precision);
                    value_to_flatten = price_f64 * size_chunk;
                }

                self.core.apply_update(
                    trade.price,
                    Quantity::new(size_chunk, trade.size.precision),
                    trade.ts_init,
                );
                self.imbalance_value += side_sign * value_to_flatten;

                // Min-size clamp can overshoot past threshold
                if self.imbalance_value.abs() >= self.step_value {
                    self.core.build_now_and_send();
                    self.imbalance_value = 0.0;
                }
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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
            let mut size_chunk = value_needed / price_f64;

            // Clamp to minimum representable size to avoid zero-volume bars
            if is_below_min_size(size_chunk, trade.size.precision) {
                if is_below_min_size(size_remaining, trade.size.precision) {
                    break;
                }
                size_chunk = min_size_f64(trade.size.precision);
            }

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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
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
    first_close_ns: UnixNanos,
    bar_build_delay: u64,
    time_bars_origin_offset: Option<TimeDelta>,
    skip_first_non_full_bar: bool,
    pub historical_mode: bool,
    historical_events: Vec<TimeEvent>,
    historical_event_at_ts_init: Option<TimeEvent>,
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
    /// Panics if `bar_type.aggregation_source` is not `AggregationSource::Internal`.
    #[expect(clippy::too_many_arguments)]
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
            timer_name: format!("TIME_BAR_{bar_type}"),
            interval_ns: get_bar_interval_ns(&bar_type),
            next_close_ns: UnixNanos::default(),
            first_close_ns: UnixNanos::default(),
            bar_build_delay,
            time_bars_origin_offset,
            skip_first_non_full_bar,
            historical_mode: false,
            historical_events: Vec::new(),
            historical_event_at_ts_init: None,
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

        let callback = TimeEventCallback::RustLocal(Rc::new(move |event: TimeEvent| {
            if let Some(agg) = aggregator_weak.upgrade() {
                agg.borrow_mut().build_bar(&event);
            }
        }));

        // Computing start_time
        let now = self.clock.borrow().utc_now();
        let mut start_time =
            get_time_bar_start(now, &self.bar_type(), self.time_bars_origin_offset);
        start_time += TimeDelta::microseconds(self.bar_build_delay as i64);

        // Closing a partial bar at the transition from historical to backtest data
        let fire_immediately = start_time == now;

        let spec = &self.bar_type().spec();
        let start_time_ns = UnixNanos::from(start_time);
        let step = spec.step.get() as u32;

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

            self.stored_open_ns = self.next_close_ns.saturating_sub_ns(self.interval_ns);
        } else {
            // The monthly/yearly alert time is defined iteratively at each alert time as there is no regular interval
            let alert_time = if fire_immediately {
                start_time
            } else if spec.aggregation == BarAggregation::Month {
                add_n_months(start_time, step).expect(FAILED)
            } else {
                add_n_years(start_time, step).expect(FAILED)
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
            // Mirror Cython: stored_open = close_time - step, so when fire_immediately the
            // current (partial) bar started `step` periods before start_time.
            self.stored_open_ns = if fire_immediately {
                if spec.aggregation == BarAggregation::Month {
                    subtract_n_months_nanos(start_time_ns, step).expect(FAILED)
                } else {
                    subtract_n_years_nanos(start_time_ns, step).expect(FAILED)
                }
            } else {
                start_time_ns
            };
        }

        if self.skip_first_non_full_bar {
            self.first_close_ns = self.next_close_ns;
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
        if self.skip_first_non_full_bar && ts_init <= self.first_close_ns {
            self.core.builder.reset();
        } else {
            // Clear for the transition from historical to live data; subsequent
            // bars always emit regardless of timestamp.
            self.skip_first_non_full_bar = false;
            self.core.build_and_send(ts_event, ts_init);
        }
    }

    fn build_bar(&mut self, event: &TimeEvent) {
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

        // Advance this aggregator's independent clock and collect timer events.
        let events = {
            let mut clock_borrow = self.clock.borrow_mut();
            let test_clock = clock_borrow
                .as_any_mut()
                .downcast_mut::<TestClock>()
                .expect("Expected TestClock in historical mode");
            test_clock.advance_time(ts_init, true)
        };

        for event in events {
            if event.ts_event == ts_init {
                self.historical_event_at_ts_init = Some(event);
            } else {
                self.build_bar(&event);
            }
        }
    }

    fn postprocess_historical_events(&mut self, _ts_init: UnixNanos) {
        if let Some(ref event) = self.historical_event_at_ts_init.take() {
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

    fn build_bar(&mut self, event: &TimeEvent) {
        // Delegate to the implementation method
        // We use the struct name here to disambiguate from the trait method
        {
            #[expect(clippy::use_self)]
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

fn is_below_min_size(size: f64, precision: u8) -> bool {
    Quantity::new(size, precision).raw == 0
}

fn min_size_f64(precision: u8) -> f64 {
    10_f64.powi(-(precision as i32))
}

/// Provider for vega per leg (option spreads). Returns `None` when greeks are unavailable.
pub trait VegaProvider {
    /// Returns vega for the given leg instrument, or `None` if not available.
    fn vega_for_leg(&self, instrument_id: InstrumentId) -> Option<f64>;
}

/// Rounder for spread bid/ask (e.g. tick scheme). When absent, raw prices are used with instrument precision.
pub trait SpreadPriceRounder {
    /// Rounds raw bid/ask to valid prices (handles negative prices with mirroring when using tick scheme).
    fn round_prices(&self, raw_bid: f64, raw_ask: f64, precision: u8) -> (Price, Price);
}

/// Vega provider that returns leg vegas from a map (e.g. populated from greeks cache).
#[derive(Debug, Default)]
pub struct MapVegaProvider {
    vegas: AHashMap<InstrumentId, f64>,
}

impl MapVegaProvider {
    pub fn new() -> Self {
        Self {
            vegas: AHashMap::new(),
        }
    }

    pub fn insert(&mut self, instrument_id: InstrumentId, vega: f64) {
        self.vegas.insert(instrument_id, vega);
    }

    pub fn get(&self, instrument_id: &InstrumentId) -> Option<f64> {
        self.vegas.get(instrument_id).copied()
    }
}

impl VegaProvider for MapVegaProvider {
    fn vega_for_leg(&self, instrument_id: InstrumentId) -> Option<f64> {
        self.vegas.get(&instrument_id).copied()
    }
}

/// Rounder that uses a fixed tick size; mirrors negative prices for tick alignment (Cython parity).
#[derive(Debug)]
pub struct FixedTickSchemeRounder {
    scheme: FixedTickScheme,
}

impl FixedTickSchemeRounder {
    /// Creates a rounder with the given tick size.
    ///
    /// # Errors
    ///
    /// Returns an error if `tick` is not positive.
    pub fn new(tick: f64) -> anyhow::Result<Self> {
        Ok(Self {
            scheme: FixedTickScheme::new(tick)?,
        })
    }

    fn round_one(&self, raw: f64, precision: u8, use_bid_rounding: bool) -> Price {
        if raw >= 0.0 {
            let p = if use_bid_rounding {
                self.scheme.next_bid_price(raw, 0, precision)
            } else {
                self.scheme.next_ask_price(raw, 0, precision)
            };
            p.unwrap_or_else(|| price_from_f64(raw, precision))
        } else {
            let p = if use_bid_rounding {
                self.scheme.next_ask_price(-raw, 0, precision)
            } else {
                self.scheme.next_bid_price(-raw, 0, precision)
            };
            p.map_or_else(
                || price_from_f64(raw, precision),
                |q| price_from_f64(-q.as_f64(), precision),
            )
        }
    }
}

impl SpreadPriceRounder for FixedTickSchemeRounder {
    fn round_prices(&self, raw_bid: f64, raw_ask: f64, precision: u8) -> (Price, Price) {
        let bid = self.round_one(raw_bid, precision, true);
        let ask = self.round_one(raw_ask, precision, false);
        (bid, ask)
    }
}

/// Spread quote aggregator: builds synthetic quotes from leg quotes (Cython parity).
///
/// Quote-driven mode (`update_interval_seconds == None`): emits when all legs have quotes.
/// Timer-driven mode: emits on timer fire when `_has_update` is true.
/// Historical mode: defers timer event at `ts_init` until after the update.
pub struct SpreadQuoteAggregator {
    spread_instrument_id: InstrumentId,
    leg_ids: Vec<InstrumentId>,
    ratios: Vec<i64>,
    n_legs: usize,
    is_futures_spread: bool,
    price_precision: u8,
    size_precision: u8,
    last_quotes: AHashMap<InstrumentId, QuoteTick>,
    mid_prices: Vec<f64>,
    bid_prices: Vec<f64>,
    ask_prices: Vec<f64>,
    vegas: Vec<f64>,
    bid_ask_spreads: Vec<f64>,
    bid_sizes: Vec<f64>,
    ask_sizes: Vec<f64>,
    handler: Box<dyn FnMut(QuoteTick)>,
    clock: Rc<RefCell<dyn Clock>>,
    historical_mode: bool,
    update_interval_seconds: Option<u64>,
    quote_build_delay: u64,
    has_update: bool,
    timer_name: String,
    historical_event_at_ts_init: Option<TimeEvent>,
    vega_provider: Option<Box<dyn VegaProvider>>,
    price_rounder: Option<Box<dyn SpreadPriceRounder>>,
    is_running: bool,
    aggregator_weak: Option<Weak<RefCell<Self>>>,
}

impl Debug for SpreadQuoteAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SpreadQuoteAggregator))
            .field("spread_instrument_id", &self.spread_instrument_id)
            .field("n_legs", &self.n_legs)
            .field("is_futures_spread", &self.is_futures_spread)
            .field("update_interval_seconds", &self.update_interval_seconds)
            .finish()
    }
}

impl SpreadQuoteAggregator {
    /// Creates a new [`SpreadQuoteAggregator`].
    ///
    /// # Panics
    ///
    /// Panics if `legs` has fewer than 2 entries or any ratio is zero.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        spread_instrument_id: InstrumentId,
        legs: &[(InstrumentId, i64)],
        is_futures_spread: bool,
        price_precision: u8,
        size_precision: u8,
        handler: Box<dyn FnMut(QuoteTick)>,
        clock: Rc<RefCell<dyn Clock>>,
        historical_mode: bool,
        update_interval_seconds: Option<u64>,
        quote_build_delay: u64,
        vega_provider: Option<Box<dyn VegaProvider>>,
        price_rounder: Option<Box<dyn SpreadPriceRounder>>,
    ) -> Self {
        assert!(legs.len() >= 2, "Spread must have more than one leg");
        let n_legs = legs.len();
        let leg_ids: Vec<InstrumentId> = legs.iter().map(|(id, _)| *id).collect();
        let ratios: Vec<i64> = legs.iter().map(|(_, r)| *r).collect();
        for &r in &ratios {
            assert!(r != 0, "Ratio cannot be zero");
        }
        let timer_name = format!("SPREAD_QUOTE_{spread_instrument_id}");
        Self {
            spread_instrument_id,
            leg_ids,
            ratios,
            n_legs,
            is_futures_spread,
            price_precision,
            size_precision,
            last_quotes: AHashMap::new(),
            mid_prices: vec![0.0; n_legs],
            bid_prices: vec![0.0; n_legs],
            ask_prices: vec![0.0; n_legs],
            vegas: vec![0.0; n_legs],
            bid_ask_spreads: vec![0.0; n_legs],
            bid_sizes: vec![0.0; n_legs],
            ask_sizes: vec![0.0; n_legs],
            handler,
            clock,
            historical_mode,
            update_interval_seconds,
            quote_build_delay,
            has_update: false,
            timer_name,
            historical_event_at_ts_init: None,
            vega_provider,
            price_rounder,
            is_running: false,
            aggregator_weak: None,
        }
    }

    /// Sets the weak reference to this aggregator (used when starting the timer so the callback can call back).
    /// Prefer [`Self::prepare_for_timer_mode`] so the owner passes the owning `Rc` in one step.
    pub fn set_aggregator_weak(&mut self, weak: Weak<RefCell<Self>>) {
        self.aggregator_weak = Some(weak);
    }

    /// One-step setup for timer-driven mode (live or historical). Call this with the `Rc` that owns
    /// this aggregator before feeding any quotes when `update_interval_seconds` is set. The timer
    /// callback will use the stored weak reference to call back into this aggregator; without this,
    /// [`Self::start_timer`] will panic in historical mode or when called with `None`.
    pub fn prepare_for_timer_mode(&mut self, self_rc: &Rc<RefCell<Self>>) {
        self.aggregator_weak = Some(Rc::downgrade(self_rc));
    }

    /// Sets historical mode and handler (and optionally greeks provider when switching).
    pub fn set_historical_mode(
        &mut self,
        historical_mode: bool,
        handler: Box<dyn FnMut(QuoteTick)>,
        vega_provider: Option<Box<dyn VegaProvider>>,
    ) {
        self.historical_mode = historical_mode;
        self.handler = handler;

        if let Some(vp) = vega_provider {
            self.vega_provider = Some(vp);
        }
    }

    pub fn set_running(&mut self, is_running: bool) {
        self.is_running = is_running;
    }

    pub fn set_clock(&mut self, clock: Rc<RefCell<dyn Clock>>) {
        self.clock = clock;
    }

    /// Starts the timer when `update_interval_seconds` is set (timer-driven mode).
    /// In live mode pass `Some(rc)` so the weak is set and the timer can call back.
    /// In historical mode the owner must have called [`Self::prepare_for_timer_mode`] with the
    /// owning `Rc` before any quote is processed, then call with `None` here.
    ///
    /// # Panics
    ///
    /// Panics if called with `None` in timer mode without a prior [`Self::prepare_for_timer_mode`] call.
    pub fn start_timer(&mut self, aggregator_rc: Option<Rc<RefCell<Self>>>) {
        let Some(interval_secs) = self.update_interval_seconds else {
            return;
        };
        let aggregator_weak = if let Some(rc) = aggregator_rc {
            let weak = Rc::downgrade(&rc);
            self.aggregator_weak = Some(weak.clone());
            weak
        } else {
            self.aggregator_weak.as_ref().cloned().expect(
                "SpreadQuoteAggregator: timer mode requires prepare_for_timer_mode(rc) to be \
                 called first with the Rc that wraps this aggregator (before feeding quotes in \
                 historical mode or before start_timer(None)).",
            )
        };

        let callback = TimeEventCallback::RustLocal(Rc::new(move |event: TimeEvent| {
            if let Some(agg) = aggregator_weak.upgrade() {
                agg.borrow_mut().on_timer_fire(event.ts_event);
            }
        }));

        let now_ns = self.clock.borrow().timestamp_ns();
        let interval_ns = interval_secs * 1_000_000_000;
        let start_ns = (now_ns.as_u64() / interval_ns) * interval_ns;
        let start_ns = start_ns + self.quote_build_delay * 1_000; // quote_build_delay in microseconds
        let start_time = UnixNanos::from(start_ns);
        let fire_immediately = now_ns == start_time;
        self.clock
            .borrow_mut()
            .set_timer_ns(
                &self.timer_name,
                interval_ns,
                Some(start_time),
                None,
                Some(callback),
                Some(true),
                Some(fire_immediately),
            )
            .expect("Failed to set spread quote timer");
    }

    /// Called when the timer fires (live mode). Builds and sends a spread quote using the timer event timestamp.
    pub fn on_timer_fire(&mut self, ts_event: UnixNanos) {
        if self.last_quotes.len() == self.n_legs {
            self.build_and_send_quote(ts_event);
        }
    }

    /// Stops the timer when in timer-driven mode.
    pub fn stop_timer(&mut self) {
        if self.update_interval_seconds.is_none() {
            return;
        }

        if self
            .clock
            .borrow()
            .timer_names()
            .contains(&self.timer_name.as_str())
        {
            self.clock.borrow_mut().cancel_timer(&self.timer_name);
        }
    }

    /// Handles an incoming leg quote (Cython `handle_quote_tick`).
    pub fn handle_quote_tick(&mut self, tick: QuoteTick) {
        let ts_init = tick.ts_init;

        if self.update_interval_seconds.is_some() && self.historical_mode {
            self.process_historical_events(ts_init);
        }
        self.last_quotes.insert(tick.instrument_id, tick);
        self.has_update = true;

        if self.update_interval_seconds.is_none() && self.last_quotes.len() == self.n_legs {
            self.build_and_send_quote(ts_init);
        }
    }

    /// Flushes the deferred historical timer event, if any.
    ///
    /// This is intended for historical request finalization, where we know no more historical
    /// quotes will arrive for the requested range and should not require a later live tick just
    /// to release the final same-timestamp spread quote.
    pub fn flush_pending_historical_quote(&mut self) {
        if self.update_interval_seconds.is_none() || !self.historical_mode {
            return;
        }

        let Some(event) = self.historical_event_at_ts_init.take() else {
            return;
        };

        if self.last_quotes.len() == self.n_legs {
            self.build_and_send_quote(event.ts_event);
        }
    }

    /// Advances the historical clock and collects timer events. Events at `ts_init` are
    /// deferred until the next call when time advances. The deferred event is only flushed
    /// when all legs have quotes and time has moved past the deferred timestamp. This
    /// prevents building a spread quote with stale leg data when multiple legs update at
    /// the same timestamp (Cython parity).
    fn process_historical_events(&mut self, ts_init: UnixNanos) {
        if self.clock.borrow().timestamp_ns() == UnixNanos::default() {
            let mut clock_borrow = self.clock.borrow_mut();
            let test_clock = clock_borrow
                .as_any_mut()
                .downcast_mut::<TestClock>()
                .expect("Expected TestClock in historical mode");
            test_clock.set_time(ts_init);
            drop(clock_borrow);
            self.start_timer(None);
        }

        if self.last_quotes.len() == self.n_legs
            && let Some(ref event) = self.historical_event_at_ts_init
            && event.ts_event < ts_init
        {
            // Guarded by `let Some(ref event)` above
            let event = self.historical_event_at_ts_init.take().unwrap();
            self.build_and_send_quote(event.ts_event);
        }

        let events = {
            let mut clock_borrow = self.clock.borrow_mut();
            let test_clock = clock_borrow
                .as_any_mut()
                .downcast_mut::<TestClock>()
                .expect("Expected TestClock in historical mode");
            test_clock.advance_time(ts_init, true)
        };

        for event in events {
            if event.ts_event == ts_init {
                self.historical_event_at_ts_init = Some(event);
            } else if self.last_quotes.len() == self.n_legs {
                self.build_and_send_quote(event.ts_event);
            }
        }
    }

    /// Builds and sends one spread quote (Cython `_build_and_send_quote`).
    fn build_and_send_quote(&mut self, ts_event: UnixNanos) {
        if !self.has_update {
            return;
        }

        for (idx, &leg_id) in self.leg_ids.iter().enumerate() {
            let Some(tick) = self.last_quotes.get(&leg_id) else {
                log::error!(
                    "SpreadQuoteAggregator[{}]: Missing quote for leg {}",
                    self.spread_instrument_id,
                    leg_id
                );
                return;
            };
            let ask_price = tick.ask_price.as_f64();
            let bid_price = tick.bid_price.as_f64();
            self.bid_prices[idx] = bid_price;
            self.ask_prices[idx] = ask_price;
            self.bid_sizes[idx] = tick.bid_size.as_f64();
            self.ask_sizes[idx] = tick.ask_size.as_f64();

            if !self.is_futures_spread {
                self.mid_prices[idx] = (ask_price + bid_price) * 0.5;
                self.bid_ask_spreads[idx] = ask_price - bid_price;

                if let Some(ref vp) = self.vega_provider
                    && let Some(vega) = vp.vega_for_leg(leg_id)
                {
                    self.vegas[idx] = vega;
                }
            }
        }
        let (raw_bid, raw_ask) = if self.is_futures_spread {
            self.create_futures_spread_prices()
        } else {
            self.create_option_spread_prices()
        };
        let spread_quote = self.create_quote_tick_from_raw_prices(raw_bid, raw_ask, ts_event);
        self.has_update = false;
        (self.handler)(spread_quote);
    }

    fn create_option_spread_prices(&self) -> (f64, f64) {
        let vega_multipliers: Vec<f64> = (0..self.n_legs)
            .map(|i| {
                if self.vegas[i] == 0.0 {
                    0.0
                } else {
                    self.bid_ask_spreads[i] / self.vegas[i]
                }
            })
            .collect();
        let non_zero: Vec<f64> = vega_multipliers
            .iter()
            .copied()
            .filter(|&x| x != 0.0)
            .collect();

        if non_zero.is_empty() {
            log::warn!(
                "No vega information available for the components of {}. Will generate spread quote using component quotes only",
                self.spread_instrument_id
            );
            return self.create_futures_spread_prices();
        }
        let vega_multiplier = non_zero.iter().map(|x| x.abs()).sum::<f64>() / non_zero.len() as f64;
        let spread_vega = self
            .vegas
            .iter()
            .zip(self.ratios.iter())
            .map(|(v, r)| v * (*r as f64))
            .sum::<f64>()
            .abs();
        let bid_ask_spread = spread_vega * vega_multiplier;
        let spread_mid_price: f64 = self
            .mid_prices
            .iter()
            .zip(self.ratios.iter())
            .map(|(m, r)| m * (*r as f64))
            .sum();
        let raw_bid = spread_mid_price - bid_ask_spread * 0.5;
        let raw_ask = spread_mid_price + bid_ask_spread * 0.5;
        (raw_bid, raw_ask)
    }

    fn create_futures_spread_prices(&self) -> (f64, f64) {
        let mut raw_ask = 0.0_f64;
        let mut raw_bid = 0.0_f64;

        for i in 0..self.n_legs {
            let r = self.ratios[i] as f64;
            if self.ratios[i] >= 0 {
                raw_ask += r * self.ask_prices[i];
                raw_bid += r * self.bid_prices[i];
            } else {
                raw_ask += r * self.bid_prices[i];
                raw_bid += r * self.ask_prices[i];
            }
        }
        (raw_bid, raw_ask)
    }

    fn create_quote_tick_from_raw_prices(
        &self,
        raw_bid_price: f64,
        raw_ask_price: f64,
        ts_event: UnixNanos,
    ) -> QuoteTick {
        let (bid_price, ask_price) = if let Some(ref rounder) = self.price_rounder {
            rounder.round_prices(raw_bid_price, raw_ask_price, self.price_precision)
        } else {
            let bid = price_from_f64(raw_bid_price, self.price_precision);
            let ask = price_from_f64(raw_ask_price, self.price_precision);
            (bid, ask)
        };
        let mut min_bid_size = f64::INFINITY;
        let mut min_ask_size = f64::INFINITY;
        for i in 0..self.n_legs {
            let abs_ratio = self.ratios[i].unsigned_abs() as f64;
            if self.ratios[i] >= 0 {
                let b = self.bid_sizes[i] / abs_ratio;
                if b < min_bid_size {
                    min_bid_size = b;
                }
                let a = self.ask_sizes[i] / abs_ratio;
                if a < min_ask_size {
                    min_ask_size = a;
                }
            } else {
                let b = self.ask_sizes[i] / abs_ratio;
                if b < min_bid_size {
                    min_bid_size = b;
                }
                let a = self.bid_sizes[i] / abs_ratio;
                if a < min_ask_size {
                    min_ask_size = a;
                }
            }
        }
        let bid_size = Quantity::new(min_bid_size, self.size_precision);
        let ask_size = Quantity::new(min_ask_size, self.size_precision);
        QuoteTick::new(
            self.spread_instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_event,
        )
    }
}

fn price_from_f64(v: f64, precision: u8) -> Price {
    Price::new(v, precision)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nautilus_common::{clock::TestClock, timer::TimeEvent};
    use nautilus_core::{MUTEX_POISONED, UUID4, UnixNanos};
    use nautilus_model::{
        data::{BarSpecification, BarType, QuoteTick},
        enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
        identifiers::InstrumentId,
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
    fn test_bar_builder_update_bar_initializes_then_accumulates(equity_aapl: Equity) {
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

        let bar_one = Bar::new(
            bar_type,
            Price::from("100.00"),
            Price::from("102.00"),
            Price::from("99.00"),
            Price::from("101.00"),
            Quantity::from(10),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
        );
        let bar_two = Bar::new(
            bar_type,
            Price::from("101.00"),
            Price::from("103.00"),
            Price::from("98.00"),
            Price::from("102.00"),
            Quantity::from(5),
            UnixNanos::from(2_000),
            UnixNanos::from(2_000),
        );

        builder.update_bar(bar_one, bar_one.volume, bar_one.ts_init);
        builder.update_bar(bar_two, bar_two.volume, bar_two.ts_init);
        let bar = builder.build_now();

        assert_eq!(bar.open, Price::from("100.00"));
        assert_eq!(bar.high, Price::from("103.00"));
        assert_eq!(bar.low, Price::from("98.00"));
        assert_eq!(bar.close, Price::from("102.00"));
        assert_eq!(bar.volume, Quantity::from(15));
        assert_eq!(builder.count, 0);
    }

    #[rstest]
    fn test_bar_builder_update_bar_ignores_earlier_timestamp(equity_aapl: Equity) {
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

        let bar_later = Bar::new(
            bar_type,
            Price::from("100.00"),
            Price::from("101.00"),
            Price::from("99.00"),
            Price::from("100.50"),
            Quantity::from(10),
            UnixNanos::from(2_000),
            UnixNanos::from(2_000),
        );
        let bar_earlier = Bar::new(
            bar_type,
            Price::from("200.00"),
            Price::from("210.00"),
            Price::from("190.00"),
            Price::from("205.00"),
            Quantity::from(50),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
        );

        builder.update_bar(bar_later, bar_later.volume, bar_later.ts_init);
        builder.update_bar(bar_earlier, bar_earlier.volume, bar_earlier.ts_init);

        assert_eq!(builder.ts_last, 2_000);
        assert_eq!(builder.count, 1);
        assert_eq!(builder.volume, Quantity::from(10));
    }

    #[rstest]
    fn test_bar_builder_build_promotes_close_above_high_from_previous_close(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.update(
            Price::from("110.00"),
            Quantity::from(1),
            UnixNanos::from(100),
        );
        builder.build_now();

        builder.update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::from(200),
        );
        builder.update(
            Price::from("101.00"),
            Quantity::from(1),
            UnixNanos::from(300),
        );
        builder.update(
            Price::from("200.00"),
            Quantity::from(1),
            UnixNanos::from(400),
        );

        let bar = builder.build_now();
        assert_eq!(bar.open, Price::from("100.00"));
        assert_eq!(bar.high, Price::from("200.00"));
        assert_eq!(bar.low, Price::from("100.00"));
        assert_eq!(bar.close, Price::from("200.00"));
    }

    #[rstest]
    fn test_bar_builder_build_clamps_low_to_close(equity_aapl: Equity) {
        // Rust BarBuilder mirrors Cython: on `build`, if `close < low` the low is pulled down to close.
        // Reaching this branch requires bypassing `update`'s low tracking (e.g. via bar updates where
        // a later bar's close is below the accumulated low). We simulate by direct field assignment.
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::from(100),
        );
        builder.close = Some(Price::from("50.00"));

        let bar = builder.build_now();
        assert_eq!(bar.low, Price::from("50.00"));
        assert_eq!(bar.close, Price::from("50.00"));
        assert!(bar.low <= bar.open);
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
    fn test_volume_bar_aggregator_zero_size_update_is_noop(equity_aapl: Equity) {
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
            Price::from("100.00"),
            Quantity::from(0),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 0);
    }

    #[rstest]
    fn test_volume_bar_aggregator_exact_threshold_emits_single_bar(equity_aapl: Equity) {
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
            Price::from("100.00"),
            Quantity::from(7),
            UnixNanos::from(1_000),
        );
        aggregator.update(
            Price::from("101.00"),
            Quantity::from(3),
            UnixNanos::from(2_000),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].volume, Quantity::from(10));
        assert_eq!(handler_guard[0].close, Price::from("101.00"));
    }

    #[rstest]
    fn test_volume_bar_aggregator_step_of_one_emits_per_unit(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Volume, PriceType::Last);
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
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].volume, Quantity::from(1));
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
    fn test_value_bar_aggregator_exact_threshold_emits_one_bar(equity_aapl: Equity) {
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

        aggregator.update(
            Price::from("100.00"),
            Quantity::from(5),
            UnixNanos::from(1_000),
        );
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(5),
            UnixNanos::from(2_000),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].volume, Quantity::from(10));
        assert_eq!(aggregator.get_cumulative_value(), 0.0);
    }

    #[rstest]
    fn test_value_bar_aggregator_precision_boundary_min_size_clamp(equity_aapl: Equity) {
        // step=100, price=100 per-unit value=100 with size_precision=0 lands the divided
        // size_chunk at the precision floor. Verifies the min-size clamp branch in update()
        // emits one bar per unit rather than looping on zero-volume chunks.
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::Value, PriceType::Last);
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

        // 4 units at $100 = $400 value, with step $100 gives 4 bars exactly.
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(4),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 4);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from(1));
        }
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
        assert_eq!(handler_guard.len(), 2);
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
        aggregator.build_bar(&event);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.ts_event, UnixNanos::default());
        assert_eq!(bar.ts_init, next_sec);
    }

    #[rstest]
    fn test_time_bar_aggregator_stop_clears_timer_and_allows_restart(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let timer_name = format!("TIME_BAR_{bar_type}");
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            |_bar: Bar| {},
            true,
            false,
            BarIntervalType::LeftOpen,
            None,
            15,
            false,
        );

        let boxed: Box<dyn BarAggregator> = Box::new(aggregator);
        let rc = Rc::new(RefCell::new(boxed));

        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));
        assert_eq!(clock.borrow().timer_names(), vec![timer_name.as_str()]);

        rc.borrow_mut().stop();
        assert!(clock.borrow().timer_names().is_empty());

        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));
        assert_eq!(clock.borrow().timer_names(), vec![timer_name.as_str()]);
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
        aggregator.build_bar(&event);

        // Update in second interval
        aggregator.update(Price::from("101.00"), Quantity::from(1), ts1);

        // Second interval close
        let ts2 = UnixNanos::from(2_000_000_000);
        clock.borrow_mut().set_time(ts2);
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts2, ts2);
        aggregator.build_bar(&event);

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
        aggregator.build_bar(&event);

        // Update in second interval
        aggregator.update(Price::from("101.00"), Quantity::from(1), ts1);

        // Second interval close
        let ts2 = UnixNanos::from(2_000_000_000);
        clock.borrow_mut().set_time(ts2);
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts2, ts2);
        aggregator.build_bar(&event);

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
        aggregator.build_bar(&event);

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
        aggregator.build_bar(&event);

        // Second interval without updates
        let ts2 = UnixNanos::from(2_000_000_000);
        clock.borrow_mut().set_time(ts2);
        let event = TimeEvent::new(Ustr::from("1-SECOND-LAST"), UUID4::new(), ts2, ts2);
        aggregator.build_bar(&event);

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
        aggregator.build_bar(&event);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.ts_event, UnixNanos::default());
        assert_eq!(bar.ts_init, ts2);
    }

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

    #[rstest]
    fn test_value_bar_high_price_low_step_no_zero_volume_bars(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::Value, PriceType::Last);
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

        // price=1000, size=3, value=3000, step=100 → size_chunk=0.1 rounds to 0 at precision 0
        aggregator.update(
            Price::from("1000.00"),
            Quantity::from(3),
            UnixNanos::default(),
        );

        // 3 bars (one per min-size unit), not 30 zero-volume bars
        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 3);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from(1));
        }
    }

    #[rstest]
    fn test_value_imbalance_high_price_low_step_no_zero_volume_bars(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueImbalance, PriceType::Last);
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

        let trade = TradeTick {
            price: Price::from("1000.00"),
            size: Quantity::from(3),
            aggressor_side: AggressorSide::Buyer,
            instrument_id: instrument.id(),
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 3);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from(1));
        }
    }

    #[rstest]
    fn test_value_imbalance_opposite_side_overshoot_emits_bar(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueImbalance, PriceType::Last);
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

        // Build seller imbalance of -50 (below step=100, no bar yet)
        let sell_tick = TradeTick {
            price: Price::from("10.00"),
            size: Quantity::from(5),
            aggressor_side: AggressorSide::Seller,
            instrument_id: instrument.id(),
            ..TradeTick::default()
        };

        // Opposite-side buyer: flatten amount 50/1000=0.05 < min_size (1),
        // clamp overshoots imbalance from -50 to +950, crossing threshold
        let buy_tick = TradeTick {
            price: Price::from("1000.00"),
            size: Quantity::from(1),
            aggressor_side: AggressorSide::Buyer,
            instrument_id: instrument.id(),
            ts_init: UnixNanos::from(1),
            ts_event: UnixNanos::from(1),
            ..TradeTick::default()
        };

        aggregator.handle_trade(sell_tick);
        aggregator.handle_trade(buy_tick);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].volume, Quantity::from(6));
    }

    #[rstest]
    fn test_value_runs_high_price_low_step_no_zero_volume_bars(equity_aapl: Equity) {
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
            price: Price::from("1000.00"),
            size: Quantity::from(3),
            aggressor_side: AggressorSide::Buyer,
            instrument_id: instrument.id(),
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 3);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from(1));
        }
    }

    #[rstest]
    #[case(1000_u64)]
    #[case(1500_u64)]
    fn test_volume_imbalance_bar_aggregator_large_step_no_overflow(
        equity_aapl: Equity,
        #[case] step: u64,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(
            step as usize,
            BarAggregation::VolumeImbalance,
            PriceType::Last,
        );
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

        let trade = TradeTick {
            size: Quantity::from(step * 2),
            aggressor_side: AggressorSide::Buyer,
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume.as_f64(), step as f64);
        }
    }

    #[rstest]
    fn test_volume_imbalance_bar_aggregator_different_large_steps_produce_different_bar_counts(
        equity_aapl: Equity,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let total_volume = 3000_u64;
        let mut results = Vec::new();

        for step in [1000_usize, 1500] {
            let bar_spec =
                BarSpecification::new(step, BarAggregation::VolumeImbalance, PriceType::Last);
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

            let trade = TradeTick {
                size: Quantity::from(total_volume),
                aggressor_side: AggressorSide::Buyer,
                ..TradeTick::default()
            };

            aggregator.handle_trade(trade);

            let handler_guard = handler.lock().expect(MUTEX_POISONED);
            results.push(handler_guard.len());
        }

        assert_eq!(results[0], 3); // 3000 / 1000
        assert_eq!(results[1], 2); // 3000 / 1500
        assert_ne!(results[0], results[1]);
    }

    #[rstest]
    #[case(1000_u64)]
    #[case(1500_u64)]
    fn test_volume_runs_bar_aggregator_large_step_no_overflow(
        equity_aapl: Equity,
        #[case] step: u64,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec =
            BarSpecification::new(step as usize, BarAggregation::VolumeRuns, PriceType::Last);
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
            size: Quantity::from(step * 2),
            aggressor_side: AggressorSide::Buyer,
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);

        let handler_guard = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(handler_guard.len(), 2);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume.as_f64(), step as f64);
        }
    }

    #[rstest]
    fn test_volume_runs_bar_aggregator_different_large_steps_produce_different_bar_counts(
        equity_aapl: Equity,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let total_volume = 3000_u64;
        let mut results = Vec::new();

        for step in [1000_usize, 1500] {
            let bar_spec = BarSpecification::new(step, BarAggregation::VolumeRuns, PriceType::Last);
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
                size: Quantity::from(total_volume),
                aggressor_side: AggressorSide::Buyer,
                ..TradeTick::default()
            };

            aggregator.handle_trade(trade);

            let handler_guard = handler.lock().expect(MUTEX_POISONED);
            results.push(handler_guard.len());
        }

        assert_eq!(results[0], 3); // 3000 / 1000
        assert_eq!(results[1], 2); // 3000 / 1500
        assert_ne!(results[0], results[1]);
    }

    /// Historical time-bar: event at ts_init is deferred until after the update (Cython parity).
    #[rstest]
    fn test_time_bar_historical_defers_event_at_ts_init_until_after_update(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut agg = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            move |bar: Bar| {
                let mut h = handler_clone.lock().expect(MUTEX_POISONED);
                h.push(bar);
            },
            true,
            true,
            BarIntervalType::LeftOpen,
            None,
            0,
            false,
        );
        agg.historical_mode = true;
        agg.set_clock_internal(clock);
        let boxed: Box<dyn BarAggregator> = Box::new(agg);
        let rc = Rc::new(RefCell::new(boxed));
        rc.borrow_mut().set_aggregator_weak(Rc::downgrade(&rc));

        rc.borrow_mut().update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::default(),
        );
        rc.borrow_mut().update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::from(1_000_000_000),
        );

        let bars = handler.lock().expect(MUTEX_POISONED);
        assert!(
            !bars.is_empty(),
            "deferred event at ts_init should produce a bar that includes the update"
        );
        let last_bar = bars.last().unwrap();
        assert_eq!(last_bar.close, Price::from("100.00"));
        assert!(
            last_bar.volume.as_f64() >= 1.0,
            "bar built after deferred event should include the update at ts_init"
        );
    }

    #[rstest]
    fn test_spread_quote_quote_driven_emits_when_all_legs_received(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(move |q: QuoteTick| {
                handler_clone.lock().expect(MUTEX_POISONED).push(q);
            }),
            clock,
            false,
            None,
            0,
            None,
            None,
        );

        let ts = UnixNanos::from(1_000_000_000);
        agg.handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("100.00"),
            Price::from("100.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts,
            ts,
        ));
        assert_eq!(handler.lock().expect(MUTEX_POISONED).len(), 0);

        agg.handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("99.00"),
            Price::from("99.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts,
            ts,
        ));
        let quotes = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].instrument_id, spread_id);
        assert!(quotes[0].bid_price < quotes[0].ask_price);
    }

    #[rstest]
    fn test_spread_quote_futures_pricing_signed_ratios(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(move |q: QuoteTick| {
                handler_clone.lock().expect(MUTEX_POISONED).push(q);
            }),
            clock,
            false,
            None,
            0,
            None,
            None,
        );

        let ts = UnixNanos::from(1_000_000_000);
        agg.handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("10.00"),
            Price::from("10.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        agg.handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("20.00"),
            Price::from("20.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        let quotes = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(quotes.len(), 1);
        let q = &quotes[0];
        assert_eq!(q.instrument_id, spread_id);
        assert_eq!(q.bid_price, Price::from("-10.10"));
        assert_eq!(q.ask_price, Price::from("-9.90"));
    }

    #[rstest]
    fn test_spread_quote_size_calculation_non_unit_ratios(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 2_i64), (leg2, -1_i64)];
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(move |q: QuoteTick| {
                handler_clone.lock().expect(MUTEX_POISONED).push(q);
            }),
            clock,
            false,
            None,
            0,
            None,
            None,
        );

        let ts = UnixNanos::from(1_000_000_000);
        agg.handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("10.00"),
            Price::from("10.10"),
            Quantity::from(100),
            Quantity::from(40),
            ts,
            ts,
        ));
        agg.handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("10.00"),
            Price::from("10.10"),
            Quantity::from(50),
            Quantity::from(30),
            ts,
            ts,
        ));
        let quotes = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(quotes.len(), 1);
        let q = &quotes[0];
        assert_eq!(q.bid_size.as_f64(), 30.0);
        assert_eq!(q.ask_size.as_f64(), 20.0);
    }

    #[rstest]
    fn test_spread_quote_timer_driven_emission_cadence(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(0));

        let agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(move |q: QuoteTick| {
                handler_clone.lock().expect(MUTEX_POISONED).push(q);
            }),
            clock.clone(),
            false,
            Some(1),
            0,
            None,
            None,
        );
        let rc = Rc::new(RefCell::new(agg));
        rc.borrow_mut().prepare_for_timer_mode(&rc);
        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

        for event in clock.borrow_mut().advance_time(UnixNanos::from(0), true) {
            rc.borrow_mut().on_timer_fire(event.ts_event);
        }
        assert_eq!(handler.lock().expect(MUTEX_POISONED).len(), 0);

        let ts1 = UnixNanos::from(1_000_000_000);
        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("100.00"),
            Price::from("100.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts1,
            ts1,
        ));
        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("99.00"),
            Price::from("99.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts1,
            ts1,
        ));

        for event in clock.borrow_mut().advance_time(ts1, true) {
            rc.borrow_mut().on_timer_fire(event.ts_event);
        }

        {
            let quotes = handler.lock().expect(MUTEX_POISONED);
            assert_eq!(quotes.len(), 1);
            assert_eq!(quotes[0].ts_event, ts1);
            assert_eq!(quotes[0].ts_init, ts1);
        }

        let ts2 = UnixNanos::from(2_000_000_000);
        for event in clock.borrow_mut().advance_time(ts2, true) {
            rc.borrow_mut().on_timer_fire(event.ts_event);
        }

        let quotes = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(quotes.len(), 1);
    }

    #[rstest]
    fn test_spread_quote_historical_timer_waits_for_all_legs(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(move |q: QuoteTick| {
                handler_clone.lock().expect(MUTEX_POISONED).push(q);
            }),
            // need clock for set_clock after
            clock.clone(),
            true,
            Some(1),
            0,
            None,
            None,
        );
        let rc = Rc::new(RefCell::new(agg));
        rc.borrow_mut().prepare_for_timer_mode(&rc);
        rc.borrow_mut().set_clock(clock);

        let ts1 = UnixNanos::from(1_000_000_000);
        let ts2 = UnixNanos::from(2_000_000_000);
        let ts3 = UnixNanos::from(3_000_000_000);
        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("100.00"),
            Price::from("100.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts1,
            ts1,
        ));
        assert_eq!(handler.lock().expect(MUTEX_POISONED).len(), 0);

        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("99.00"),
            Price::from("99.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts2,
            ts2,
        ));
        assert_eq!(handler.lock().expect(MUTEX_POISONED).len(), 0);

        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("100.00"),
            Price::from("100.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts3,
            ts3,
        ));
        let quotes = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(
            quotes.len(),
            1,
            "deferred event at ts2 is processed when we have all legs and advance to ts3"
        );
    }

    #[rstest]
    fn test_spread_quote_historical_flush_emits_pending_final_quote(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(move |q: QuoteTick| {
                handler_clone.lock().expect(MUTEX_POISONED).push(q);
            }),
            // need clock for set_clock after
            clock.clone(),
            true,
            Some(1),
            0,
            None,
            None,
        );
        let rc = Rc::new(RefCell::new(agg));
        rc.borrow_mut().prepare_for_timer_mode(&rc);
        rc.borrow_mut().set_clock(clock);

        let ts1 = UnixNanos::from(1_000_000_000);
        let ts2 = UnixNanos::from(2_000_000_000);
        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("100.00"),
            Price::from("100.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts1,
            ts1,
        ));
        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("99.00"),
            Price::from("99.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts2,
            ts2,
        ));

        assert_eq!(handler.lock().expect(MUTEX_POISONED).len(), 0);

        rc.borrow_mut().flush_pending_historical_quote();

        let quotes = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(
            quotes.len(),
            1,
            "final historical quote should be emitted when the deferred event is flushed",
        );
        assert_eq!(quotes[0].ts_event, ts2);
    }

    #[rstest]
    fn test_spread_quote_option_vega_weighting(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut vega_provider = MapVegaProvider::new();
        vega_provider.insert(leg1, 0.15);
        vega_provider.insert(leg2, 0.12);

        let mut agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            false,
            instrument.price_precision(),
            0,
            Box::new(move |q: QuoteTick| {
                handler_clone.lock().expect(MUTEX_POISONED).push(q);
            }),
            clock,
            false,
            None,
            0,
            Some(Box::new(vega_provider)),
            None,
        );

        let ts = UnixNanos::from(1_000_000_000);
        agg.handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("10.00"),
            Price::from("10.20"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        agg.handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("11.00"),
            Price::from("11.20"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        let quotes = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(quotes.len(), 1);
        let q = &quotes[0];
        assert!(q.bid_price < q.ask_price);
        assert!(q.ask_price.as_f64() - q.bid_price.as_f64() > 0.0);
    }

    #[rstest]
    fn test_spread_quote_all_zero_vega_fallback(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut vega_provider = MapVegaProvider::new();
        vega_provider.insert(leg1, 0.0);
        vega_provider.insert(leg2, 0.0);

        let mut agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            false,
            instrument.price_precision(),
            0,
            Box::new(move |q: QuoteTick| {
                handler_clone.lock().expect(MUTEX_POISONED).push(q);
            }),
            clock,
            false,
            None,
            0,
            Some(Box::new(vega_provider)),
            None,
        );

        let ts = UnixNanos::from(1_000_000_000);
        agg.handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("10.00"),
            Price::from("10.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        agg.handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("20.00"),
            Price::from("20.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        let quotes = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(quotes.len(), 1);
        let q = &quotes[0];
        assert_eq!(q.bid_price, Price::from("-10.10"));
        assert_eq!(q.ask_price, Price::from("-9.90"));
    }

    #[rstest]
    fn test_spread_quote_negative_prices_tick_scheme(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let rounder = FixedTickSchemeRounder::new(0.01).unwrap();

        let mut agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            2,
            0,
            Box::new(move |q: QuoteTick| {
                handler_clone.lock().expect(MUTEX_POISONED).push(q);
            }),
            clock,
            false,
            None,
            0,
            None,
            Some(Box::new(rounder)),
        );

        let ts = UnixNanos::from(1_000_000_000);
        agg.handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("10.00"),
            Price::from("10.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        agg.handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("20.00"),
            Price::from("20.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        let quotes = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(quotes.len(), 1);
        let q = &quotes[0];
        assert!(q.bid_price.as_f64() < 0.0);
        assert!(q.ask_price.as_f64() < 0.0);
        assert!(q.bid_price < q.ask_price);
    }

    #[rstest]
    #[case(BarIntervalType::LeftOpen)]
    #[case(BarIntervalType::RightOpen)]
    fn test_time_bar_skip_first_non_full_bar_noop_on_boundary(
        equity_aapl: Equity,
        #[case] interval_type: BarIntervalType,
    ) {
        // When the clock sits on a bar boundary, fire_immediately=true and
        // first_close_ns equals that boundary. Every subsequent bar closes
        // strictly after first_close_ns, so skip_first_non_full_bar never
        // triggers and both bars emit.
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(1_000_000_000));
        let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock,
            move |bar: Bar| {
                let mut h = handler_clone.lock().expect(MUTEX_POISONED);
                h.push(bar);
            },
            false,
            false,
            interval_type,
            None,
            0,
            true, // skip_first_non_full_bar
        );

        let boxed: Box<dyn BarAggregator> = Box::new(aggregator);
        let rc = Rc::new(RefCell::new(boxed));
        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

        rc.borrow_mut().update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::from(1_000_000_000),
        );
        rc.borrow_mut().build_bar(&TimeEvent::new(
            event_name,
            UUID4::new(),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(2_000_000_000),
        ));
        rc.borrow_mut().update(
            Price::from("101.00"),
            Quantity::from(1),
            UnixNanos::from(2_500_000_000),
        );
        rc.borrow_mut().build_bar(&TimeEvent::new(
            event_name,
            UUID4::new(),
            UnixNanos::from(3_000_000_000),
            UnixNanos::from(3_000_000_000),
        ));

        let bars = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].close, Price::from("100.00"));
        assert_eq!(bars[1].close, Price::from("101.00"));
    }

    #[rstest]
    #[case(BarIntervalType::LeftOpen)]
    #[case(BarIntervalType::RightOpen)]
    fn test_time_bar_skip_first_non_full_bar_drops_partial_bar(
        equity_aapl: Equity,
        #[case] interval_type: BarIntervalType,
    ) {
        // When the clock starts past a boundary (mid-interval), first_close_ns
        // is the upcoming boundary. The bar closing at first_close_ns is partial,
        // so skip_first_non_full_bar drops it; subsequent full bars emit.
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(1_500_000_000));
        let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock,
            move |bar: Bar| {
                let mut h = handler_clone.lock().expect(MUTEX_POISONED);
                h.push(bar);
            },
            false,
            false,
            interval_type,
            None,
            0,
            true, // skip_first_non_full_bar
        );

        let boxed: Box<dyn BarAggregator> = Box::new(aggregator);
        let rc = Rc::new(RefCell::new(boxed));
        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

        rc.borrow_mut().update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::from(1_500_000_000),
        );
        rc.borrow_mut().build_bar(&TimeEvent::new(
            event_name,
            UUID4::new(),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(2_000_000_000),
        ));
        rc.borrow_mut().update(
            Price::from("101.00"),
            Quantity::from(1),
            UnixNanos::from(2_500_000_000),
        );
        rc.borrow_mut().build_bar(&TimeEvent::new(
            event_name,
            UUID4::new(),
            UnixNanos::from(3_000_000_000),
            UnixNanos::from(3_000_000_000),
        ));

        let bars = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].close, Price::from("101.00"));
    }

    #[rstest]
    fn test_time_bar_skip_first_non_full_bar_skips_every_call_before_first_close(
        equity_aapl: Equity,
    ) {
        // The flag must remain set across every build_and_send call whose
        // ts_init <= first_close_ns, and only flip once a bar actually emits.
        // Catches a mutation that flips skip_first_non_full_bar early.
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(5_000_000_000));
        let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock,
            move |bar: Bar| {
                let mut h = handler_clone.lock().expect(MUTEX_POISONED);
                h.push(bar);
            },
            false,
            false,
            BarIntervalType::LeftOpen,
            None,
            0,
            true, // skip_first_non_full_bar
        );

        let boxed: Box<dyn BarAggregator> = Box::new(aggregator);
        let rc = Rc::new(RefCell::new(boxed));
        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

        // first_close_ns is 10_000_000_000 (first 10s boundary after start).
        // Drive three build_bar calls at ts <= first_close_ns, each preceded by a
        // distinct update. Every one of them must be skipped.
        for (price, update_ts, event_ts) in [
            ("100.00", 5_500_000_000_u64, 7_000_000_000_u64),
            ("101.00", 7_500_000_000_u64, 8_000_000_000_u64),
            ("102.00", 9_000_000_000_u64, 10_000_000_000_u64),
        ] {
            rc.borrow_mut().update(
                Price::from(price),
                Quantity::from(1),
                UnixNanos::from(update_ts),
            );
            rc.borrow_mut().build_bar(&TimeEvent::new(
                event_name,
                UUID4::new(),
                UnixNanos::from(event_ts),
                UnixNanos::from(event_ts),
            ));
        }

        // Final update + build past first_close_ns emits for the first time.
        rc.borrow_mut().update(
            Price::from("103.00"),
            Quantity::from(1),
            UnixNanos::from(10_500_000_000),
        );
        rc.borrow_mut().build_bar(&TimeEvent::new(
            event_name,
            UUID4::new(),
            UnixNanos::from(11_000_000_000),
            UnixNanos::from(11_000_000_000),
        ));

        let bars = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].close, Price::from("103.00"));
    }

    #[rstest]
    fn test_time_bar_skip_first_non_full_bar_skips_when_build_delay_shifts_start(
        equity_aapl: Equity,
    ) {
        // Cython parity: when bar_build_delay > 0 pushes start_time past a
        // boundary (even if `now` is on a boundary), first_close_ns is set and
        // the first bar is skipped. The previous Rust `now > start_time` guard
        // incorrectly kept this first bar.
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(2_000_000_000));
        let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock,
            move |bar: Bar| {
                let mut h = handler_clone.lock().expect(MUTEX_POISONED);
                h.push(bar);
            },
            false,
            false,
            BarIntervalType::LeftOpen,
            None,
            100,  // bar_build_delay (microseconds)
            true, // skip_first_non_full_bar
        );

        let boxed: Box<dyn BarAggregator> = Box::new(aggregator);
        let rc = Rc::new(RefCell::new(boxed));
        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

        // start_time = 2s + 100us = 2_000_100_000 ns; first_close_ns = 3_000_100_000 ns.
        rc.borrow_mut().update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::from(2_500_000_000),
        );
        rc.borrow_mut().build_bar(&TimeEvent::new(
            event_name,
            UUID4::new(),
            UnixNanos::from(3_000_100_000),
            UnixNanos::from(3_000_100_000),
        ));
        rc.borrow_mut().update(
            Price::from("101.00"),
            Quantity::from(1),
            UnixNanos::from(3_500_000_000),
        );
        rc.borrow_mut().build_bar(&TimeEvent::new(
            event_name,
            UUID4::new(),
            UnixNanos::from(4_000_100_000),
            UnixNanos::from(4_000_100_000),
        ));

        let bars = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].close, Price::from("101.00"));
    }

    #[rstest]
    #[case(
        BarAggregation::Month,
        1_735_689_600_000_000_000_u64,
        1_733_011_200_000_000_000_u64
    )]
    #[case(
        BarAggregation::Year,
        1_735_689_600_000_000_000_u64,
        1_704_067_200_000_000_000_u64
    )]
    fn test_time_bar_fire_immediately_month_year_stored_open_points_to_previous_period(
        equity_aapl: Equity,
        #[case] aggregation: BarAggregation,
        #[case] start_ns: u64,
        #[case] expected_stored_open_ns: u64,
    ) {
        // When the clock is exactly on a month/year boundary, fire_immediately=true.
        // stored_open_ns must resolve to one step before start_time (mirrors Cython
        // close_time - step arithmetic) so the first bar's open timestamp marks
        // the true start of the in-progress interval.
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, aggregation, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(start_ns));
        let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock,
            move |bar: Bar| {
                let mut h = handler_clone.lock().expect(MUTEX_POISONED);
                h.push(bar);
            },
            false,
            false,
            BarIntervalType::RightOpen, // ts_event = stored_open_ns
            None,
            0,
            false, // skip_first_non_full_bar
        );

        let boxed: Box<dyn BarAggregator> = Box::new(aggregator);
        let rc = Rc::new(RefCell::new(boxed));
        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

        rc.borrow_mut().update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::from(start_ns),
        );
        rc.borrow_mut().build_bar(&TimeEvent::new(
            event_name,
            UUID4::new(),
            UnixNanos::from(start_ns),
            UnixNanos::from(start_ns),
        ));

        let bars = handler.lock().expect(MUTEX_POISONED);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].ts_event, UnixNanos::from(expected_stored_open_ns));
        assert_eq!(bars[0].ts_init, UnixNanos::from(start_ns));
    }

    #[rstest]
    fn test_time_bar_historical_prevents_bars_for_timer_before_last_data(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let handler = Arc::new(Mutex::new(Vec::new()));
        let handler_clone = Arc::clone(&handler);
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut agg = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            move |bar: Bar| {
                let mut h = handler_clone.lock().expect(MUTEX_POISONED);
                h.push(bar);
            },
            true,
            true,
            BarIntervalType::LeftOpen,
            None,
            0,
            false,
        );
        agg.historical_mode = true;
        agg.set_clock_internal(clock);
        let boxed: Box<dyn BarAggregator> = Box::new(agg);
        let rc = Rc::new(RefCell::new(boxed));
        rc.borrow_mut().set_aggregator_weak(Rc::downgrade(&rc));

        let ts1 = UnixNanos::from(2_000_000_000);
        rc.borrow_mut()
            .update(Price::from("100.00"), Quantity::from(1), ts1);

        let ts2 = UnixNanos::from(3_000_000_000);
        rc.borrow_mut()
            .update(Price::from("101.00"), Quantity::from(1), ts2);

        let bars = handler.lock().expect(MUTEX_POISONED);
        assert!(
            !bars.is_empty(),
            "advancing time from ts1 to ts2 should produce at least one bar"
        );
        assert_eq!(bars[0].close, Price::from("100.00"));
    }
}

#[cfg(test)]
mod property_tests {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    use nautilus_common::{clock::TestClock, timer::TimeEvent};
    use nautilus_core::{MUTEX_POISONED, UUID4, UnixNanos};
    use nautilus_model::{
        data::{Bar, BarSpecification, BarType, bar::get_bar_interval_ns},
        enums::{AggregationSource, BarAggregation, BarIntervalType, PriceType},
        instruments::{Instrument, InstrumentAny, stubs::equity_aapl},
        types::{Price, Quantity},
    };
    use proptest::prelude::*;
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    fn aggregation_strategy() -> impl Strategy<Value = BarAggregation> {
        prop_oneof![
            Just(BarAggregation::Second),
            Just(BarAggregation::Minute),
            Just(BarAggregation::Hour),
        ]
    }

    fn interval_type_strategy() -> impl Strategy<Value = BarIntervalType> {
        prop_oneof![
            Just(BarIntervalType::LeftOpen),
            Just(BarIntervalType::RightOpen),
        ]
    }

    proptest! {
        #[rstest]
        fn prop_skip_first_drops_partial_then_emits(
            aggregation in aggregation_strategy(),
            step in 1usize..=5,
            interval_type in interval_type_strategy(),
            skip_first in any::<bool>(),
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(step, aggregation, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let interval_ns = get_bar_interval_ns(&bar_type).as_u64();

            // Anchor the clock one full interval past epoch plus a half-interval offset
            // so start_time lands mid-interval and fire_immediately is false.
            let now_ns = interval_ns + interval_ns / 2;

            let handler = Arc::new(Mutex::new(Vec::<Bar>::new()));
            let handler_clone = Arc::clone(&handler);
            let clock = Rc::new(RefCell::new(TestClock::new()));
            clock.borrow_mut().set_time(UnixNanos::from(now_ns));
            let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

            let aggregator = TimeBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                clock,
                move |bar: Bar| {
                    let mut h = handler_clone.lock().expect(MUTEX_POISONED);
                    h.push(bar);
                },
                false,
                false,
                interval_type,
                None,
                0,
                skip_first,
            );

            let boxed: Box<dyn BarAggregator> = Box::new(aggregator);
            let rc = Rc::new(RefCell::new(boxed));
            rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

            // First tick + first close event. start_time = 1 * interval, first_close
            // = 2 * interval. ts_init == first_close_ns: partial bar.
            rc.borrow_mut().update(
                Price::from("100.00"),
                Quantity::from(1),
                UnixNanos::from(now_ns),
            );
            let first_close = 2 * interval_ns;
            rc.borrow_mut().build_bar(&TimeEvent::new(
                event_name,
                UUID4::new(),
                UnixNanos::from(first_close),
                UnixNanos::from(first_close),
            ));

            // Second tick + later close; emits unconditionally.
            rc.borrow_mut().update(
                Price::from("101.00"),
                Quantity::from(1),
                UnixNanos::from(first_close + interval_ns / 2),
            );
            let second_close = first_close + interval_ns;
            rc.borrow_mut().build_bar(&TimeEvent::new(
                event_name,
                UUID4::new(),
                UnixNanos::from(second_close),
                UnixNanos::from(second_close),
            ));

            let bars = handler.lock().expect(MUTEX_POISONED);
            let expected = if skip_first { 1 } else { 2 };
            prop_assert_eq!(bars.len(), expected);
            prop_assert_eq!(bars.last().unwrap().close, Price::from("101.00"));
            for bar in bars.iter() {
                prop_assert!(bar.high >= bar.open);
                prop_assert!(bar.high >= bar.close);
                prop_assert!(bar.low <= bar.open);
                prop_assert!(bar.low <= bar.close);
            }
        }

        #[rstest]
        fn prop_skip_first_noop_on_exact_boundary(
            aggregation in aggregation_strategy(),
            step in 1usize..=5,
            interval_type in interval_type_strategy(),
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(step, aggregation, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let interval_ns = get_bar_interval_ns(&bar_type).as_u64();

            // Clock exactly on a bar boundary: fire_immediately=true, so the first
            // bar that reaches build_and_send must emit regardless of skip_first.
            let now_ns = interval_ns;
            let handler = Arc::new(Mutex::new(Vec::<Bar>::new()));
            let handler_clone = Arc::clone(&handler);
            let clock = Rc::new(RefCell::new(TestClock::new()));
            clock.borrow_mut().set_time(UnixNanos::from(now_ns));
            let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

            let aggregator = TimeBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                clock,
                move |bar: Bar| {
                    let mut h = handler_clone.lock().expect(MUTEX_POISONED);
                    h.push(bar);
                },
                false,
                false,
                interval_type,
                None,
                0,
                true, // skip_first_non_full_bar
            );

            let boxed: Box<dyn BarAggregator> = Box::new(aggregator);
            let rc = Rc::new(RefCell::new(boxed));
            rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

            rc.borrow_mut().update(
                Price::from("100.00"),
                Quantity::from(1),
                UnixNanos::from(now_ns),
            );
            let next_close = now_ns + interval_ns;
            rc.borrow_mut().build_bar(&TimeEvent::new(
                event_name,
                UUID4::new(),
                UnixNanos::from(next_close),
                UnixNanos::from(next_close),
            ));

            let bars = handler.lock().expect(MUTEX_POISONED);
            prop_assert_eq!(bars.len(), 1);
            prop_assert_eq!(bars[0].close, Price::from("100.00"));
        }

        #[rstest]
        fn prop_bar_builder_ohlc_invariants(
            updates in prop::collection::vec((1i64..=100_000i64, 1u64..=1_000u64), 1..=50),
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(1, BarAggregation::Tick, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let mut builder = BarBuilder::new(bar_type, 2, 0);

            let mut total_volume: u64 = 0;

            for (i, (price_cents, size)) in updates.iter().enumerate() {
                let price = Price::new((*price_cents as f64) / 100.0, 2);
                let qty = Quantity::new(*size as f64, 0);
                let ts = UnixNanos::from((i as u64 + 1) * 1_000);
                total_volume += *size;
                builder.update(price, qty, ts);
            }

            let bar = builder.build_now();
            prop_assert!(bar.low <= bar.open);
            prop_assert!(bar.low <= bar.close);
            prop_assert!(bar.high >= bar.open);
            prop_assert!(bar.high >= bar.close);
            prop_assert!(bar.low <= bar.high);
            prop_assert_eq!(bar.volume.as_f64(), total_volume as f64);
        }

        #[rstest]
        fn prop_tick_bar_aggregator_volume_conservation(
            ticks in prop::collection::vec((1i64..=1_000i64, 1u64..=100u64), 3..=60),
            step in 1usize..=5,
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(step, BarAggregation::Tick, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let handler = Arc::new(Mutex::new(Vec::<Bar>::new()));
            let handler_clone = Arc::clone(&handler);

            let mut aggregator = TickBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                move |bar: Bar| {
                    handler_clone.lock().expect(MUTEX_POISONED).push(bar);
                },
            );

            let mut total_input: u64 = 0;

            for (i, (price_cents, size)) in ticks.iter().enumerate() {
                let price = Price::new((*price_cents as f64) / 100.0, 2);
                let qty = Quantity::new(*size as f64, 0);
                aggregator.update(price, qty, UnixNanos::from((i as u64 + 1) * 1_000));
                total_input += *size;
            }

            let bars = handler.lock().expect(MUTEX_POISONED);
            let emitted_count = bars.len();
            prop_assert_eq!(emitted_count, ticks.len() / step);

            let mut sum_emitted: f64 = 0.0;

            for bar in bars.iter() {
                prop_assert!(bar.low <= bar.open);
                prop_assert!(bar.low <= bar.close);
                prop_assert!(bar.high >= bar.open);
                prop_assert!(bar.high >= bar.close);
                sum_emitted += bar.volume.as_f64();
            }

            // Unemitted pending size remains in the builder for the remainder `ticks.len() % step` ticks.
            let pending_size: u64 = ticks.iter()
                .skip(emitted_count * step)
                .map(|(_, s)| *s)
                .sum();
            prop_assert!((sum_emitted + pending_size as f64 - total_input as f64).abs() < 1e-6);
        }

        #[rstest]
        fn prop_volume_bar_aggregator_conservation(
            sizes in prop::collection::vec(1u64..=50u64, 3..=40),
            step in 2u64..=10u64,
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(step as usize, BarAggregation::Volume, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let handler = Arc::new(Mutex::new(Vec::<Bar>::new()));
            let handler_clone = Arc::clone(&handler);

            let mut aggregator = VolumeBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                move |bar: Bar| {
                    handler_clone.lock().expect(MUTEX_POISONED).push(bar);
                },
            );

            let mut total_input: u64 = 0;

            for (i, size) in sizes.iter().enumerate() {
                aggregator.update(
                    Price::from("100.00"),
                    Quantity::new(*size as f64, 0),
                    UnixNanos::from((i as u64 + 1) * 1_000),
                );
                total_input += *size;
            }

            let bars = handler.lock().expect(MUTEX_POISONED);

            // Every emitted bar has exactly `step` volume and OHLC ordering holds.
            for bar in bars.iter() {
                prop_assert_eq!(bar.volume, Quantity::from(step));
                prop_assert!(bar.low <= bar.open);
                prop_assert!(bar.low <= bar.close);
                prop_assert!(bar.high >= bar.open);
                prop_assert!(bar.high >= bar.close);
            }

            // Conservation: total emitted + pending builder volume equals total input.
            let emitted_total: u64 = bars.len() as u64 * step;
            let pending = aggregator.core.builder.volume.as_f64();
            prop_assert!((emitted_total as f64 + pending - total_input as f64).abs() < 1e-6);
        }

        #[rstest]
        fn prop_value_bar_aggregator_ohlc_invariants(
            ticks in prop::collection::vec((50i64..=500i64, 1u64..=20u64), 2..=30),
            step in 100u64..=2_000u64,
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(step as usize, BarAggregation::Value, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let handler = Arc::new(Mutex::new(Vec::<Bar>::new()));
            let handler_clone = Arc::clone(&handler);

            let mut aggregator = ValueBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                move |bar: Bar| {
                    handler_clone.lock().expect(MUTEX_POISONED).push(bar);
                },
            );

            for (i, (price_cents, size)) in ticks.iter().enumerate() {
                aggregator.update(
                    Price::new((*price_cents as f64) / 100.0, 2),
                    Quantity::new(*size as f64, 0),
                    UnixNanos::from((i as u64 + 1) * 1_000),
                );
            }

            let bars = handler.lock().expect(MUTEX_POISONED);
            for bar in bars.iter() {
                prop_assert!(bar.low <= bar.open);
                prop_assert!(bar.low <= bar.close);
                prop_assert!(bar.high >= bar.open);
                prop_assert!(bar.high >= bar.close);
                prop_assert!(bar.volume.as_f64() > 0.0);
            }
        }
    }
}
