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
use jiff::SignedDuration;
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
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BarIntervalType,
        ContinuousFutureAdjustmentType,
    },
    identifiers::InstrumentId,
    instruments::{FixedTickScheme, TickSchemeRule},
    types::{
        Price, Quantity,
        fixed::{FIXED_PRECISION, FIXED_SCALAR, mantissa_exponent_to_fixed_i128},
        price::PriceRaw,
        quantity::QuantityRaw,
    },
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

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
        // Quote-fed aggregators use Bid/Ask/Mid (Last uses trades), so this cannot fail; guard
        // rather than unwrap to stay panic-free
        let (Ok(price), Ok(size)) = (
            quote.extract_price(spec.price_type),
            quote.extract_size(spec.price_type),
        ) else {
            log::error!(
                "Cannot aggregate quote for {}: price type {} unsupported for quotes",
                self.bar_type(),
                spec.price_type,
            );
            return;
        };

        self.update(price, size, quote.ts_init);
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
    /// Sets historical mode and the handler used for completed bars.
    fn set_historical_mode(&mut self, _historical_mode: bool, _handler: Box<dyn FnMut(Bar)>) {}
    /// Sets historical events (default implementation does nothing, `TimeBarAggregator` overrides)
    fn set_historical_events(&mut self, _events: Vec<TimeEvent>) {}
    /// Sets clock for time bar aggregators (default implementation does nothing, `TimeBarAggregator` overrides)
    fn set_clock(&mut self, _clock: Rc<RefCell<dyn Clock>>) {}
    /// Builds a bar from a time event (default implementation does nothing, `TimeBarAggregator` overrides)
    fn build_bar(&mut self, _event: &TimeEvent) {}
    /// Starts the timer for time bar aggregators.
    /// Default implementation does nothing, `TimeBarAggregator` overrides.
    /// Takes an optional Rc to create weak reference internally.
    fn start_timer(&mut self, _aggregator_rc: Option<Rc<RefCell<Box<dyn BarAggregator>>>>) {}
    /// Sets the weak reference to the aggregator wrapper (for historical mode).
    /// Default implementation does nothing, `TimeBarAggregator` overrides.
    fn set_aggregator_weak(&mut self, _weak: Weak<RefCell<Box<dyn BarAggregator>>>) {}
    /// Configures the continuous-future price adjustment for the underlying builder.
    fn set_adjustment(&mut self, _adjustment: Decimal, _mode: ContinuousFutureAdjustmentType) {}
    /// Sets whether empty intervals emit bars at the last close.
    /// Default implementation does nothing, `TimeBarAggregator` overrides.
    fn set_build_with_no_updates(&mut self, _value: bool) {}
    /// If the aggregator is processing historical data on a private clock.
    /// Default implementation returns `false`, `TimeBarAggregator` overrides.
    fn is_historical(&self) -> bool {
        false
    }
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
    adjustment_raw: PriceRaw,
    adjustment_ratio: f64,
    adjustment_active: bool,
    adjustment_is_ratio: bool,
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
            adjustment_raw: 0,
            adjustment_ratio: 1.0,
            adjustment_active: false,
            adjustment_is_ratio: false,
        }
    }

    /// Configures the per-tick continuous-future price adjustment.
    ///
    /// Adjustment applies on ingress in [`Self::update`] and [`Self::update_bar`], so the running
    /// OHLC state is always in the adjusted (common) frame. The adjustment configuration is
    /// retained across [`Self::reset`] so it spans subsequent bars within the same continuous-
    /// future segment.
    ///
    /// # Panics
    ///
    /// Panics if scaling the spread `adjustment` to the fixed-point representation overflows.
    pub fn set_adjustment(&mut self, adjustment: Decimal, mode: ContinuousFutureAdjustmentType) {
        if mode.is_ratio() {
            self.adjustment_is_ratio = true;
            self.adjustment_ratio = adjustment.to_f64().unwrap_or(1.0);
            self.adjustment_active = adjustment != Decimal::ONE;
            return;
        }

        // Spread mode: scale the Decimal offset to FIXED_PRECISION once so the hot path
        // can add it straight onto `price.raw`. Signed PriceRaw supports negatives, so
        // backward-spread offsets that push prices below zero remain representable.
        self.adjustment_is_ratio = false;
        let exponent = -(adjustment.scale() as i8);
        let raw_i128 =
            mantissa_exponent_to_fixed_i128(adjustment.mantissa(), exponent, FIXED_PRECISION)
                .expect("Failed to scale continuous-future adjustment to fixed precision");

        #[allow(
            clippy::useless_conversion,
            reason = "i128 to PriceRaw is real when not high-precision"
        )]
        let raw: PriceRaw = raw_i128
            .try_into()
            .expect("Continuous-future adjustment exceeds PriceRaw range");

        self.adjustment_raw = raw;
        self.adjustment_active = self.adjustment_raw != 0;
    }

    fn apply_adjustment_to_price(&self, price: Price) -> Price {
        if !self.adjustment_active {
            return price;
        }

        if self.adjustment_is_ratio {
            // Multiply in double; `Price::new` rounds to the target precision.
            // Float can shift 1 ULP for high-precision raws (spread mode is exact).
            return Price::new(price.as_f64() * self.adjustment_ratio, price.precision);
        }

        // Spread: signed raw addition.
        Price::from_raw(price.raw + self.adjustment_raw, price.precision)
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

        let price = self.apply_adjustment_to_price(price);

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

        debug_assert!(self.high >= self.low, "OHLC invariant violated: high < low");
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

        let bar_open = self.apply_adjustment_to_price(bar.open);
        let bar_high = self.apply_adjustment_to_price(bar.high);
        let bar_low = self.apply_adjustment_to_price(bar.low);
        let bar_close = self.apply_adjustment_to_price(bar.close);

        if self.open.is_none() {
            self.open = Some(bar_open);
            self.high = Some(bar_high);
            self.low = Some(bar_low);
            self.initialized = true;
        } else {
            if bar_high > self.high.unwrap() {
                self.high = Some(bar_high);
            }

            if bar_low < self.low.unwrap() {
                self.low = Some(bar_low);
            }
        }

        self.close = Some(bar_close);
        self.volume = self.volume.add(volume);
        self.count += 1;
        self.ts_last = ts_init;

        debug_assert!(self.high >= self.low, "OHLC invariant violated: high < low");
    }

    /// Resets per-bar OHLCV state.
    ///
    /// Adjustment configuration set via [`Self::set_adjustment`] is retained across resets so it
    /// spans subsequent bars within the same continuous-future segment.
    pub fn reset(&mut self) {
        self.open = None;
        self.high = None;
        self.low = None;
        self.close = None;
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
    builder: BarBuilder,
    handler: BarHandler,
    is_running: bool,
}

impl Debug for BarAggregatorCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BarAggregatorCore))
            .field("bar_type", &self.builder.bar_type)
            .field("builder", &self.builder)
            .field("is_running", &self.is_running)
            .finish()
    }
}

impl BarAggregatorCore {
    /// Creates a new [`BarAggregatorCore`] instance.
    ///
    /// The `bar_type` is standardized so aggregators always emit bars carrying the
    /// standard form: the composite suffix is a local aggregation detail and must not
    /// leak into emitted bars, publish topics, or cache keys.
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
        let bar_type = bar_type.standard();
        Self {
            builder: BarBuilder::new(bar_type, price_precision, size_precision),
            handler: Box::new(handler),
            is_running: false,
        }
    }

    /// Sets the running state of the aggregator (receives updates when `true`).
    pub const fn set_is_running(&mut self, value: bool) {
        self.is_running = value;
    }

    fn set_handler(&mut self, handler: BarHandler) {
        self.handler = handler;
    }

    fn is_stale(&self, ts_init: UnixNanos) -> bool {
        ts_init < self.builder.ts_last
    }

    fn build_now_and_send(&mut self) {
        let bar = self.builder.build_now();
        (self.handler)(bar);
    }

    fn build_and_send(&mut self, ts_event: UnixNanos, ts_init: UnixNanos) {
        let bar = self.builder.build(ts_event, ts_init);
        (self.handler)(bar);
    }

    fn set_adjustment(&mut self, adjustment: Decimal, mode: ContinuousFutureAdjustmentType) {
        self.builder.set_adjustment(adjustment, mode);
    }
}

macro_rules! impl_core_bar_aggregator {
    () => {
        fn bar_type(&self) -> BarType {
            self.core.builder.bar_type
        }

        fn is_running(&self) -> bool {
            self.core.is_running
        }

        fn set_is_running(&mut self, value: bool) {
            self.core.set_is_running(value);
        }

        fn set_historical_mode(&mut self, _historical_mode: bool, handler: Box<dyn FnMut(Bar)>) {
            self.core.set_handler(handler);
        }

        fn set_adjustment(&mut self, adjustment: Decimal, mode: ContinuousFutureAdjustmentType) {
            self.core.set_adjustment(adjustment, mode);
        }
    };
}

/// Provides a means of building tick bars aggregated from quote and trades.
///
/// When received tick count reaches the step threshold of the bar
/// specification, then a bar is created and sent to the handler.
#[derive(Debug)]
pub struct TickBarAggregator {
    core: BarAggregatorCore,
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
    impl_core_bar_aggregator!();

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.builder.update(price, size, ts_init);
        let spec = self.core.builder.bar_type.spec();

        if self.core.builder.count >= spec.step.get() {
            self.core.build_now_and_send();
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        self.core.builder.update_bar(bar, volume, ts_init);
        let spec = self.core.builder.bar_type.spec();

        if self.core.builder.count >= spec.step.get() {
            self.core.build_now_and_send();
        }
    }
}

/// Aggregates bars based on tick buy/sell imbalance.
///
/// Increments imbalance by +1 for buyer-aggressed trades and -1 for seller-aggressed trades.
/// Emits a bar when the absolute imbalance reaches the step threshold.
#[derive(Debug)]
pub struct TickImbalanceBarAggregator {
    core: BarAggregatorCore,
    imbalance: isize,
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
    impl_core_bar_aggregator!();

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.builder.update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        if self.core.is_stale(trade.ts_init) {
            return;
        }

        self.core
            .builder
            .update(trade.price, trade.size, trade.ts_init);

        let delta = match trade.aggressor_side {
            AggressorSide::Buy => 1,
            AggressorSide::Sell => -1,
            AggressorSide::NoAggressor => return,
        };

        self.imbalance += delta;
        let threshold = self.core.builder.bar_type.spec().step.get();
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
#[derive(Debug)]
pub struct TickRunsBarAggregator {
    core: BarAggregatorCore,
    current_run_side: Option<AggressorSide>,
    run_count: usize,
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
    impl_core_bar_aggregator!();

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.builder.update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        if self.core.is_stale(trade.ts_init) {
            return;
        }

        let side = match trade.aggressor_side {
            AggressorSide::Buy => AggressorSide::Buy,
            AggressorSide::Sell => AggressorSide::Sell,
            AggressorSide::NoAggressor => {
                self.core
                    .builder
                    .update(trade.price, trade.size, trade.ts_init);
                return;
            }
        };

        if self.current_run_side != Some(side) {
            self.current_run_side = Some(side);
            self.run_count = 0;
            self.core.builder.reset();
        }

        self.core
            .builder
            .update(trade.price, trade.size, trade.ts_init);
        self.run_count += 1;

        let threshold = self.core.builder.bar_type.spec().step.get();
        if self.run_count >= threshold {
            self.core.build_now_and_send();
            self.run_count = 0;
            self.current_run_side = None;
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        self.core.builder.update_bar(bar, volume, ts_init);
    }
}

/// Provides a means of building volume bars aggregated from quote and trades.
#[derive(Debug)]
pub struct VolumeBarAggregator {
    core: BarAggregatorCore,
    raw_step: QuantityRaw,
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
            core: BarAggregatorCore::new(bar_type, price_precision, size_precision, handler),
            raw_step: step_as_quantity_raw(bar_type.spec().step.get()),
        }
    }
}

impl BarAggregator for VolumeBarAggregator {
    impl_core_bar_aggregator!();

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        if self.core.is_stale(ts_init) {
            return;
        }

        let mut raw_size_update = size.raw;
        let raw_step = self.raw_step;

        while raw_size_update > 0 {
            debug_assert!(
                self.core.builder.volume.raw < raw_step,
                "builder volume must stay below the step threshold between emissions"
            );

            if self.core.builder.volume.raw + raw_size_update < raw_step {
                self.core.builder.update(
                    price,
                    Quantity::from_raw(raw_size_update, size.precision),
                    ts_init,
                );
                break;
            }

            let raw_size_diff = raw_step - self.core.builder.volume.raw;
            self.core.builder.update(
                price,
                Quantity::from_raw(raw_size_diff, size.precision),
                ts_init,
            );

            self.core.build_now_and_send();
            raw_size_update -= raw_size_diff;
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        if self.core.is_stale(ts_init) {
            return;
        }

        let mut raw_volume_update = volume.raw;
        let raw_step = self.raw_step;

        while raw_volume_update > 0 {
            debug_assert!(
                self.core.builder.volume.raw < raw_step,
                "builder volume must stay below the step threshold between emissions"
            );

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
#[derive(Debug)]
pub struct VolumeImbalanceBarAggregator {
    core: BarAggregatorCore,
    imbalance_raw: i128,
    raw_step: i128,
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
        // Cast cannot overflow: usize::MAX * FIXED_SCALAR < i128::MAX
        let raw_step = step_as_quantity_raw(bar_type.spec().step.get()) as i128;
        Self {
            core: BarAggregatorCore::new(bar_type, price_precision, size_precision, handler),
            imbalance_raw: 0,
            raw_step,
        }
    }
}

impl BarAggregator for VolumeImbalanceBarAggregator {
    impl_core_bar_aggregator!();

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.builder.update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        if self.core.is_stale(trade.ts_init) {
            return;
        }

        let side = match trade.aggressor_side {
            AggressorSide::Buy => 1,
            AggressorSide::Sell => -1,
            AggressorSide::NoAggressor => {
                self.core
                    .builder
                    .update(trade.price, trade.size, trade.ts_init);
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
                .builder
                .update(trade.price, qty_chunk, trade.ts_init);

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
#[derive(Debug)]
pub struct VolumeRunsBarAggregator {
    core: BarAggregatorCore,
    current_run_side: Option<AggressorSide>,
    run_volume_raw: QuantityRaw,
    raw_step: QuantityRaw,
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
        let raw_step = step_as_quantity_raw(bar_type.spec().step.get());
        Self {
            core: BarAggregatorCore::new(bar_type, price_precision, size_precision, handler),
            current_run_side: None,
            run_volume_raw: 0,
            raw_step,
        }
    }
}

impl BarAggregator for VolumeRunsBarAggregator {
    impl_core_bar_aggregator!();

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.builder.update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        if self.core.is_stale(trade.ts_init) {
            return;
        }

        let side = match trade.aggressor_side {
            AggressorSide::Buy => AggressorSide::Buy,
            AggressorSide::Sell => AggressorSide::Sell,
            AggressorSide::NoAggressor => {
                self.core
                    .builder
                    .update(trade.price, trade.size, trade.ts_init);
                return;
            }
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

            self.core.builder.update(
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

        // Leftover volume past the last emitted bar starts a new run on the same
        // side; without this the next same-side trade reads as a side change and
        // resets the builder, silently dropping the pending volume.
        if self.run_volume_raw > 0 {
            self.current_run_side = Some(side);
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
#[derive(Debug)]
pub struct ValueBarAggregator {
    core: BarAggregatorCore,
    cum_value: Decimal,
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
            core: BarAggregatorCore::new(bar_type, price_precision, size_precision, handler),
            cum_value: Decimal::ZERO,
        }
    }

    #[must_use]
    /// Returns the cumulative value for the aggregator.
    pub const fn get_cumulative_value(&self) -> Decimal {
        self.cum_value
    }
}

impl BarAggregator for ValueBarAggregator {
    impl_core_bar_aggregator!();

    /// Apply the given update to the aggregator.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        if self.core.is_stale(ts_init) {
            return;
        }

        let step_value = Decimal::from(self.core.builder.bar_type.spec().step.get());
        let price_value = price.as_decimal();
        let mut size_update = size.as_decimal();

        while size_update > Decimal::ZERO {
            // cum_value < step_value holds between emissions, so a zero value_update
            // (zero price) always falls into the accumulate branch below and the
            // division cannot see a zero divisor.
            debug_assert!(self.cum_value < step_value);
            let value_update = price_value * size_update;

            if self.cum_value + value_update < step_value {
                self.cum_value += value_update;
                self.core.builder.update(
                    price,
                    quantity_from_decimal(size_update, size.precision),
                    ts_init,
                );
                break;
            }

            let value_diff = step_value - self.cum_value;
            let mut size_diff = size_update * (value_diff / value_update);

            // Clamp to minimum representable size to avoid zero-volume bars
            if is_below_min_size_decimal(size_diff, size.precision) {
                if is_below_min_size_decimal(size_update, size.precision) {
                    break;
                }
                size_diff = min_size_decimal(size.precision);
            }

            // Subtract the representable quantity actually applied, not the ideal
            // fraction, so rounding does not leak volume from the accounting
            let applied = quantity_from_decimal(size_diff, size.precision);
            self.core.builder.update(price, applied, ts_init);

            self.core.build_now_and_send();
            self.cum_value = Decimal::ZERO;
            size_update -= applied.as_decimal();
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        if self.core.is_stale(ts_init) {
            return;
        }

        let step_value = Decimal::from(self.core.builder.bar_type.spec().step.get());
        let average_price =
            ((bar.high.as_decimal() + bar.low.as_decimal() + bar.close.as_decimal())
                / Decimal::from(3))
            .round_dp(u32::from(self.core.builder.price_precision));
        let mut volume_update = volume.as_decimal();

        while volume_update > Decimal::ZERO {
            // See `update` for why a zero divisor cannot occur here.
            debug_assert!(self.cum_value < step_value);
            let value_update = average_price * volume_update;

            if self.cum_value + value_update < step_value {
                self.cum_value += value_update;
                self.core.builder.update_bar(
                    bar,
                    quantity_from_decimal(volume_update, volume.precision),
                    ts_init,
                );
                break;
            }

            let value_diff = step_value - self.cum_value;
            let mut volume_diff = volume_update * (value_diff / value_update);

            // Clamp to minimum representable size to avoid zero-volume bars
            if is_below_min_size_decimal(volume_diff, volume.precision) {
                if is_below_min_size_decimal(volume_update, volume.precision) {
                    break;
                }
                volume_diff = min_size_decimal(volume.precision);
            }

            // Subtract the representable quantity actually applied, not the ideal
            // fraction, so rounding does not leak volume from the accounting
            let applied = quantity_from_decimal(volume_diff, volume.precision);
            self.core.builder.update_bar(bar, applied, ts_init);

            self.core.build_now_and_send();
            self.cum_value = Decimal::ZERO;
            volume_update -= applied.as_decimal();
        }
    }
}

/// Aggregates bars based on buy/sell notional imbalance.
#[derive(Debug)]
pub struct ValueImbalanceBarAggregator {
    core: BarAggregatorCore,
    imbalance_value: Decimal,
    step_value: Decimal,
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
            core: BarAggregatorCore::new(bar_type, price_precision, size_precision, handler),
            imbalance_value: Decimal::ZERO,
            step_value: Decimal::from(bar_type.spec().step.get()),
        }
    }
}

impl BarAggregator for ValueImbalanceBarAggregator {
    impl_core_bar_aggregator!();

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.builder.update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        if self.core.is_stale(trade.ts_init) {
            return;
        }

        let price_value = trade.price.as_decimal();
        if price_value.is_zero() {
            self.core
                .builder
                .update(trade.price, trade.size, trade.ts_init);
            return;
        }

        let (side_sign, side_is_buy) = match trade.aggressor_side {
            AggressorSide::Buy => (Decimal::ONE, true),
            AggressorSide::Sell => (Decimal::NEGATIVE_ONE, false),
            AggressorSide::NoAggressor => {
                self.core
                    .builder
                    .update(trade.price, trade.size, trade.ts_init);
                return;
            }
        };

        let precision = trade.size.precision;
        let mut size_remaining = trade.size.as_decimal();
        while size_remaining > Decimal::ZERO {
            let value_remaining = price_value * size_remaining;

            if self.imbalance_value.is_zero()
                || self.imbalance_value.is_sign_positive() == side_is_buy
            {
                let needed = self.step_value - self.imbalance_value.abs();
                if value_remaining <= needed {
                    self.imbalance_value += side_sign * value_remaining;
                    self.core.builder.update(
                        trade.price,
                        quantity_from_decimal(size_remaining, precision),
                        trade.ts_init,
                    );

                    if self.imbalance_value.abs() >= self.step_value {
                        self.core.build_now_and_send();
                        self.imbalance_value = Decimal::ZERO;
                    }
                    break;
                }

                let mut value_chunk = needed;
                let mut size_chunk = value_chunk / price_value;

                // Clamp to minimum representable size to avoid zero-volume bars
                if is_below_min_size_decimal(size_chunk, precision) {
                    if is_below_min_size_decimal(size_remaining, precision) {
                        break;
                    }
                    size_chunk = min_size_decimal(precision);
                    value_chunk = price_value * size_chunk;
                }

                // Subtract the representable quantity actually applied, not the ideal
                // fraction, so rounding does not leak volume from the accounting
                let applied = quantity_from_decimal(size_chunk, precision);
                self.core
                    .builder
                    .update(trade.price, applied, trade.ts_init);
                self.imbalance_value += side_sign * value_chunk;
                size_remaining -= applied.as_decimal();

                if self.imbalance_value.abs() >= self.step_value {
                    self.core.build_now_and_send();
                    self.imbalance_value = Decimal::ZERO;
                }
            } else {
                // Opposing side: first neutralize existing imbalance
                let mut value_to_flatten = self.imbalance_value.abs().min(value_remaining);
                let mut size_chunk = value_to_flatten / price_value;

                // Clamp to minimum representable size to avoid zero-volume bars
                if is_below_min_size_decimal(size_chunk, precision) {
                    if is_below_min_size_decimal(size_remaining, precision) {
                        break;
                    }
                    size_chunk = min_size_decimal(precision);
                    value_to_flatten = price_value * size_chunk;
                }

                // Subtract the representable quantity actually applied, not the ideal
                // fraction, so rounding does not leak volume from the accounting
                let applied = quantity_from_decimal(size_chunk, precision);
                self.core
                    .builder
                    .update(trade.price, applied, trade.ts_init);
                self.imbalance_value += side_sign * value_to_flatten;

                // Min-size clamp can overshoot past threshold
                if self.imbalance_value.abs() >= self.step_value {
                    self.core.build_now_and_send();
                    self.imbalance_value = Decimal::ZERO;
                }
                size_remaining -= applied.as_decimal();
            }
        }
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        self.core.builder.update_bar(bar, volume, ts_init);
    }
}

/// Aggregates bars based on consecutive buy/sell notional runs.
#[derive(Debug)]
pub struct ValueRunsBarAggregator {
    core: BarAggregatorCore,
    current_run_side: Option<AggressorSide>,
    run_value: Decimal,
    step_value: Decimal,
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
            core: BarAggregatorCore::new(bar_type, price_precision, size_precision, handler),
            current_run_side: None,
            run_value: Decimal::ZERO,
            step_value: Decimal::from(bar_type.spec().step.get()),
        }
    }
}

impl BarAggregator for ValueRunsBarAggregator {
    impl_core_bar_aggregator!();

    /// Apply the given update to the aggregator.
    ///
    /// Note: side-aware logic lives in `handle_trade`. This method is used for
    /// quote/bar updates where no aggressor side is available.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        self.core.builder.update(price, size, ts_init);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        if self.core.is_stale(trade.ts_init) {
            return;
        }

        let price_value = trade.price.as_decimal();
        if price_value.is_zero() {
            self.core
                .builder
                .update(trade.price, trade.size, trade.ts_init);
            return;
        }

        let side = match trade.aggressor_side {
            AggressorSide::Buy => AggressorSide::Buy,
            AggressorSide::Sell => AggressorSide::Sell,
            AggressorSide::NoAggressor => {
                self.core
                    .builder
                    .update(trade.price, trade.size, trade.ts_init);
                return;
            }
        };

        if self.current_run_side != Some(side) {
            self.current_run_side = Some(side);
            self.run_value = Decimal::ZERO;
            self.core.builder.reset();
        }

        let precision = trade.size.precision;
        let mut size_remaining = trade.size.as_decimal();
        while size_remaining > Decimal::ZERO {
            let value_update = price_value * size_remaining;
            if self.run_value + value_update < self.step_value {
                self.run_value += value_update;
                self.core.builder.update(
                    trade.price,
                    quantity_from_decimal(size_remaining, precision),
                    trade.ts_init,
                );
                break;
            }

            let value_needed = self.step_value - self.run_value;
            let mut size_chunk = value_needed / price_value;

            // Clamp to minimum representable size to avoid zero-volume bars
            if is_below_min_size_decimal(size_chunk, precision) {
                if is_below_min_size_decimal(size_remaining, precision) {
                    break;
                }
                size_chunk = min_size_decimal(precision);
            }

            // Subtract the representable quantity actually applied, not the ideal
            // fraction, so rounding does not leak volume from the accounting
            let applied = quantity_from_decimal(size_chunk, precision);
            self.core
                .builder
                .update(trade.price, applied, trade.ts_init);

            self.core.build_now_and_send();
            self.run_value = Decimal::ZERO;
            self.current_run_side = None;
            size_remaining -= applied.as_decimal();
        }

        // Leftover value past the last emitted bar starts a new run on the same
        // side; without this the next same-side trade reads as a side change and
        // resets the builder, silently dropping the pending volume.
        if self.run_value > Decimal::ZERO {
            self.current_run_side = Some(side);
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
#[derive(Debug)]
pub struct RenkoBarAggregator {
    core: BarAggregatorCore,
    pub brick_size: PriceRaw,
    last_close: Option<Price>,
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
            core: BarAggregatorCore::new(bar_type, price_precision, size_precision, handler),
            brick_size,
            last_close: None,
        }
    }
}

impl BarAggregator for RenkoBarAggregator {
    impl_core_bar_aggregator!();

    /// Apply the given update to the aggregator.
    ///
    /// For Renko bars, we check if the price movement from the last close
    /// is greater than or equal to the brick size. If so, we create new bars.
    fn update(&mut self, price: Price, size: Quantity, ts_init: UnixNanos) {
        if self.core.is_stale(ts_init) {
            return;
        }

        // Always update the builder with the current tick
        self.core.builder.update(price, size, ts_init);
        self.build_bricks(price, ts_init);
    }

    fn update_bar(&mut self, bar: Bar, volume: Quantity, ts_init: UnixNanos) {
        if self.core.is_stale(ts_init) {
            return;
        }

        // Always update the builder with the current bar
        self.core.builder.update_bar(bar, volume, ts_init);
        self.build_bricks(bar.close, ts_init);
    }
}

impl RenkoBarAggregator {
    fn build_bricks(&mut self, price: Price, ts_init: UnixNanos) {
        let Some(last_close) = self.last_close else {
            self.last_close = Some(price);
            return;
        };

        let price_diff_raw = price.raw - last_close.raw;
        let abs_price_diff_raw = price_diff_raw.abs();
        if abs_price_diff_raw < self.brick_size {
            return;
        }

        let num_bricks = (abs_price_diff_raw / self.brick_size) as usize;
        let direction = if price_diff_raw > 0 { 1.0 } else { -1.0 };
        let mut current_close = last_close;
        let total_volume = self.core.builder.volume;

        for _ in 0..num_bricks {
            let brick_close_raw = current_close.raw + (direction as PriceRaw) * self.brick_size;
            let brick_close = Price::from_raw(brick_close_raw, price.precision);
            let (brick_high, brick_low) = if direction > 0.0 {
                (brick_close, current_close)
            } else {
                (current_close, brick_close)
            };

            self.core.builder.reset();
            self.core.builder.open = Some(current_close);
            self.core.builder.high = Some(brick_high);
            self.core.builder.low = Some(brick_low);
            self.core.builder.close = Some(brick_close);
            self.core.builder.volume = total_volume;
            self.core.builder.count = 1;
            self.core.builder.ts_last = ts_init;
            self.core.builder.initialized = true;
            self.core.build_and_send(ts_init, ts_init);

            current_close = brick_close;
            self.last_close = Some(brick_close);
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
    time_bars_origin_offset: Option<SignedDuration>,
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
        time_bars_origin_offset: Option<SignedDuration>,
        bar_build_delay: u64,
        skip_first_non_full_bar: bool,
    ) -> Self {
        let is_left_open = match interval_type {
            BarIntervalType::LeftOpen => true,
            BarIntervalType::RightOpen => false,
        };

        let core = BarAggregatorCore::new(bar_type, price_precision, size_precision, handler);

        Self {
            clock,
            build_with_no_updates,
            timestamp_on_close,
            is_left_open,
            stored_open_ns: UnixNanos::default(),
            timer_name: format!("TIME_BAR_{}", core.builder.bar_type),
            interval_ns: get_bar_interval_ns(&bar_type),
            core,
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
    /// Creates a callback to `build_bar` using a weak reference to the aggregator.
    ///
    /// # Panics
    ///
    /// Panics if `aggregator_rc` is None and `aggregator_weak` hasn't been set, or if timer registration fails.
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
        start_time += SignedDuration::from_micros(self.bar_build_delay as i64);

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
                let interval_duration = SignedDuration::from_nanos(self.interval_ns.as_i64());
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
            // With fire_immediately the current (partial) bar started `step` periods before
            // start_time, so stored_open resolves to close_time - step.
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
        self.core.builder.bar_type
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

        self.core.builder.update(price, size, ts_init);

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

    fn set_adjustment(&mut self, adjustment: Decimal, mode: ContinuousFutureAdjustmentType) {
        self.core.set_adjustment(adjustment, mode);
    }

    fn set_build_with_no_updates(&mut self, value: bool) {
        self.build_with_no_updates = value;
    }

    fn is_historical(&self) -> bool {
        self.historical_mode
    }
}

fn is_below_min_size_decimal(size: Decimal, precision: u8) -> bool {
    quantity_from_decimal(size, precision).raw == 0
}

fn min_size_decimal(precision: u8) -> Decimal {
    Decimal::new(1, u32::from(precision))
}

fn quantity_from_decimal(size: Decimal, precision: u8) -> Quantity {
    Quantity::from_decimal_dp(size, precision).expect(FAILED)
}

// Converts a bar specification step to raw quantity units with exact integer arithmetic
fn step_as_quantity_raw(step: usize) -> QuantityRaw {
    (FIXED_SCALAR as QuantityRaw)
        .checked_mul(step as QuantityRaw)
        .expect("`step` overflows raw quantity units for volume aggregation")
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
        Self::default()
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

/// Rounder that uses a fixed tick size; mirrors negative prices for tick alignment.
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
            p.unwrap_or_else(|| Price::new(raw, precision))
        } else {
            let p = if use_bid_rounding {
                self.scheme.next_ask_price(-raw, 0, precision)
            } else {
                self.scheme.next_bid_price(-raw, 0, precision)
            };
            p.map_or_else(
                || Price::new(raw, precision),
                |q| Price::new(-q.as_f64(), precision),
            )
        }
    }
}

impl SpreadPriceRounder for FixedTickSchemeRounder {
    fn round_prices(&self, raw_bid: f64, raw_ask: f64, precision: u8) -> (Price, Price) {
        (
            self.round_one(raw_bid, precision, true),
            self.round_one(raw_ask, precision, false),
        )
    }
}

/// Spread quote aggregator: builds synthetic quotes from leg quotes.
///
/// Quote-driven mode (`update_interval_seconds == None`): emits when all legs have quotes.
/// Timer-driven mode: emits on timer fire when `_has_update` is true.
/// Historical mode: defers timer event at `ts_init` until after the update.
pub struct SpreadQuoteAggregator {
    spread_instrument_id: InstrumentId,
    leg_ids: Vec<InstrumentId>,
    ratios: Vec<i64>,
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
    vega_pricing_timeout_timer_name: String,
    historical_event_at_ts_init: Option<TimeEvent>,
    vega_provider: Option<Box<dyn VegaProvider>>,
    disable_vega_pricing: bool,
    vega_pricing_temporarily_disabled: bool,
    vega_pricing_timeout_seconds: u64,
    price_rounder: Option<Box<dyn SpreadPriceRounder>>,
    is_running: bool,
    aggregator_weak: Option<Weak<RefCell<Self>>>,
}

impl Debug for SpreadQuoteAggregator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SpreadQuoteAggregator))
            .field("spread_instrument_id", &self.spread_instrument_id)
            .field("n_legs", &self.leg_ids.len())
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
        disable_vega_pricing: bool,
        vega_pricing_timeout_seconds: u64,
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
        let vega_pricing_timeout_timer_name =
            format!("VEGA_PRICING_TIMEOUT_{spread_instrument_id}");
        Self {
            spread_instrument_id,
            leg_ids,
            ratios,
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
            vega_pricing_timeout_timer_name,
            historical_event_at_ts_init: None,
            vega_provider,
            disable_vega_pricing,
            vega_pricing_temporarily_disabled: false,
            vega_pricing_timeout_seconds,
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
        if let Some(rc) = aggregator_rc {
            self.aggregator_weak = Some(Rc::downgrade(&rc));
        }

        let Some(interval_secs) = self.update_interval_seconds else {
            return;
        };
        let aggregator_weak = self.aggregator_weak.clone().expect(
            "SpreadQuoteAggregator: timer mode requires prepare_for_timer_mode(rc) to be \
                 called first with the Rc that wraps this aggregator (before feeding quotes in \
                 historical mode or before start_timer(None)).",
        );

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
        if self.last_quotes.len() == self.leg_ids.len() {
            self.build_and_send_quote(ts_event);
        }
    }

    /// Stops the timer when in timer-driven mode.
    pub fn stop_timer(&mut self) {
        if self.update_interval_seconds.is_some()
            && self
                .clock
                .borrow()
                .timer_names()
                .contains(&self.timer_name.as_str())
        {
            self.clock.borrow_mut().cancel_timer(&self.timer_name);
        }

        if self
            .clock
            .borrow()
            .timer_names()
            .contains(&self.vega_pricing_timeout_timer_name.as_str())
        {
            self.clock
                .borrow_mut()
                .cancel_timer(&self.vega_pricing_timeout_timer_name);
        }
    }

    /// Handles an incoming leg quote.
    pub fn handle_quote_tick(&mut self, tick: QuoteTick) {
        let ts_init = tick.ts_init;

        if self.update_interval_seconds.is_some() && self.historical_mode {
            self.process_historical_events(ts_init);
        }
        self.last_quotes.insert(tick.instrument_id, tick);
        self.has_update = true;

        if self.update_interval_seconds.is_none() && self.last_quotes.len() == self.leg_ids.len() {
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

        if self.last_quotes.len() == self.leg_ids.len() {
            self.build_and_send_quote(event.ts_event);
        }
    }

    /// Advances the historical clock and collects timer events. Events at `ts_init` are
    /// deferred until the next call when time advances. The deferred event is only flushed
    /// when all legs have quotes and time has moved past the deferred timestamp. This
    /// prevents building a spread quote with stale leg data when multiple legs update at
    /// the same timestamp.
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

        if self.last_quotes.len() == self.leg_ids.len()
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
            } else if self.last_quotes.len() == self.leg_ids.len() {
                self.build_and_send_quote(event.ts_event);
            }
        }
    }

    /// Builds and sends one spread quote.
    fn build_and_send_quote(&mut self, ts_event: UnixNanos) {
        if !self.has_update {
            return;
        }

        let use_vega_pricing =
            !(self.disable_vega_pricing || self.vega_pricing_temporarily_disabled);

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
                self.mid_prices[idx] = f64::midpoint(ask_price, bid_price);
                self.bid_ask_spreads[idx] = ask_price - bid_price;

                if use_vega_pricing
                    && let Some(ref vp) = self.vega_provider
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

    fn create_option_spread_prices(&mut self) -> (f64, f64) {
        if self.disable_vega_pricing || self.vega_pricing_temporarily_disabled {
            return self.create_futures_spread_prices();
        }

        let (vega_multiplier_sum, vega_multiplier_count) = (0..self.leg_ids.len())
            .filter_map(|i| {
                let multiplier = if self.vegas[i] == 0.0 {
                    0.0
                } else {
                    self.bid_ask_spreads[i] / self.vegas[i]
                };
                (multiplier != 0.0).then_some(multiplier.abs())
            })
            .fold((0.0, 0_usize), |(sum, count), multiplier| {
                (sum + multiplier, count + 1)
            });

        if vega_multiplier_count == 0 {
            log::warn!(
                "No vega information available for the components of {}; will generate spread quote using component quotes only, vega pricing is disabled for {} seconds, subscribe to some underlying price information for more precise quotes",
                self.spread_instrument_id,
                self.vega_pricing_timeout_seconds
            );
            self.start_vega_pricing_timeout();
            return self.create_futures_spread_prices();
        }
        let vega_multiplier = vega_multiplier_sum / vega_multiplier_count as f64;
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

    fn clear_vega_pricing_timeout(&mut self) {
        self.vega_pricing_temporarily_disabled = false;
    }

    fn start_vega_pricing_timeout(&mut self) {
        self.vega_pricing_temporarily_disabled = true;

        if self
            .clock
            .borrow()
            .timer_names()
            .contains(&self.vega_pricing_timeout_timer_name.as_str())
        {
            return;
        }

        let Some(aggregator_weak) = self.aggregator_weak.clone() else {
            return;
        };
        let callback = TimeEventCallback::RustLocal(Rc::new(move |_event: TimeEvent| {
            if let Some(agg) = aggregator_weak.upgrade() {
                agg.borrow_mut().clear_vega_pricing_timeout();
            }
        }));
        let alert_time =
            self.clock.borrow().timestamp_ns() + self.vega_pricing_timeout_seconds * 1_000_000_000;

        self.clock
            .borrow_mut()
            .set_time_alert_ns(
                &self.vega_pricing_timeout_timer_name,
                alert_time,
                Some(callback),
                Some(true),
            )
            .expect("Failed to set spread quote vega pricing timeout");
    }

    fn create_futures_spread_prices(&self) -> (f64, f64) {
        let mut raw_ask = 0.0_f64;
        let mut raw_bid = 0.0_f64;

        for i in 0..self.leg_ids.len() {
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
            (
                Price::new(raw_bid_price, self.price_precision),
                Price::new(raw_ask_price, self.price_precision),
            )
        };
        let mut min_bid_size = f64::INFINITY;
        let mut min_ask_size = f64::INFINITY;
        for i in 0..self.leg_ids.len() {
            let abs_ratio = self.ratios[i].unsigned_abs() as f64;
            let (bid_size, ask_size) = if self.ratios[i] >= 0 {
                (self.bid_sizes[i], self.ask_sizes[i])
            } else {
                (self.ask_sizes[i], self.bid_sizes[i])
            };
            let bid_size = bid_size / abs_ratio;
            if bid_size < min_bid_size {
                min_bid_size = bid_size;
            }
            let ask_size = ask_size / abs_ratio;
            if ask_size < min_ask_size {
                min_ask_size = ask_size;
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_common::{clock::TestClock, timer::TimeEvent};
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::{BarSpecification, BarType, QuoteTick},
        enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
        identifiers::InstrumentId,
        instruments::{CurrencyPair, Equity, Instrument, InstrumentAny, stubs::*},
        types::{Price, Quantity},
    };
    use parking_lot::Mutex;
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
    #[case::spread_zero_inactive(
        Decimal::ZERO,
        ContinuousFutureAdjustmentType::BackwardSpread,
        false
    )]
    #[case::spread_positive_active(
        Decimal::new(150, 2), // 1.50
        ContinuousFutureAdjustmentType::BackwardSpread,
        true,
    )]
    #[case::spread_negative_active(
        Decimal::new(-250, 2), // -2.50
        ContinuousFutureAdjustmentType::ForwardSpread,
        true,
    )]
    #[case::spread_sub_precision_inactive(
        // 1e-28 scales to 0 raw under banker's rounding, so should be inactive.
        Decimal::new(1, 28),
        ContinuousFutureAdjustmentType::BackwardSpread,
        false,
    )]
    #[case::ratio_one_inactive(Decimal::ONE, ContinuousFutureAdjustmentType::BackwardRatio, false)]
    #[case::ratio_non_one_active(
        Decimal::new(105, 2), // 1.05
        ContinuousFutureAdjustmentType::ForwardRatio,
        true,
    )]
    fn test_bar_builder_set_adjustment_active_flag(
        equity_aapl: Equity,
        #[case] adjustment: Decimal,
        #[case] mode: ContinuousFutureAdjustmentType,
        #[case] expected_active: bool,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.set_adjustment(adjustment, mode);

        assert_eq!(builder.adjustment_active, expected_active);
        assert_eq!(builder.adjustment_is_ratio, mode.is_ratio());
    }

    #[rstest]
    fn test_bar_builder_set_adjustment_mode_switch_resets_flags(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        // ratio -> spread: subsequent update must shift, not scale.
        builder.set_adjustment(
            Decimal::new(150, 2), // 1.50
            ContinuousFutureAdjustmentType::BackwardRatio,
        );
        builder.set_adjustment(
            Decimal::new(50, 2), // +0.50
            ContinuousFutureAdjustmentType::BackwardSpread,
        );
        assert!(!builder.adjustment_is_ratio);
        builder.update(Price::from("100.00"), Quantity::from(1), 1_000.into());
        assert_eq!(builder.build_now().close, Price::from("100.50"));

        // spread -> ratio: subsequent update must scale, not shift.
        builder.set_adjustment(
            Decimal::new(11, 1), // 1.1
            ContinuousFutureAdjustmentType::ForwardRatio,
        );
        assert!(builder.adjustment_is_ratio);
        builder.update(Price::from("100.00"), Quantity::from(1), 2_000.into());
        assert_eq!(builder.build_now().close, Price::from("110.00"));
    }

    #[rstest]
    fn test_bar_builder_update_applies_backward_spread_adjustment(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.set_adjustment(
            Decimal::new(250, 2), // +2.50
            ContinuousFutureAdjustmentType::BackwardSpread,
        );

        builder.update(Price::from("100.00"), Quantity::from(1), 1_000.into());
        builder.update(Price::from("99.00"), Quantity::from(1), 2_000.into());
        builder.update(Price::from("101.00"), Quantity::from(1), 3_000.into());

        let bar = builder.build_now();
        assert_eq!(bar.open, Price::from("102.50"));
        assert_eq!(bar.high, Price::from("103.50"));
        assert_eq!(bar.low, Price::from("101.50"));
        assert_eq!(bar.close, Price::from("103.50"));
    }

    #[rstest]
    fn test_bar_builder_update_applies_forward_ratio_adjustment(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.set_adjustment(
            Decimal::new(11, 1), // 1.1
            ContinuousFutureAdjustmentType::ForwardRatio,
        );

        builder.update(Price::from("100.00"), Quantity::from(1), 1_000.into());
        builder.update(Price::from("90.00"), Quantity::from(1), 2_000.into());
        builder.update(Price::from("110.00"), Quantity::from(1), 3_000.into());

        let bar = builder.build_now();
        assert_eq!(bar.open, Price::from("110.00"));
        assert_eq!(bar.high, Price::from("121.00"));
        assert_eq!(bar.low, Price::from("99.00"));
        assert_eq!(bar.close, Price::from("121.00"));
    }

    #[rstest]
    fn test_bar_builder_update_bar_applies_adjustment_to_ohlc(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.set_adjustment(
            Decimal::new(-100, 2), // -1.00
            ContinuousFutureAdjustmentType::BackwardSpread,
        );

        let input = Bar::new(
            bar_type,
            Price::from("100.00"),
            Price::from("105.00"),
            Price::from("99.00"),
            Price::from("102.00"),
            Quantity::from(10),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
        );
        builder.update_bar(input, input.volume, input.ts_init);

        let bar = builder.build_now();
        assert_eq!(bar.open, Price::from("99.00"));
        assert_eq!(bar.high, Price::from("104.00"));
        assert_eq!(bar.low, Price::from("98.00"));
        assert_eq!(bar.close, Price::from("101.00"));
    }

    #[rstest]
    fn test_bar_builder_reset_retains_adjustment(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.set_adjustment(
            Decimal::new(500, 2), // +5.00
            ContinuousFutureAdjustmentType::BackwardSpread,
        );
        builder.update(Price::from("100.00"), Quantity::from(1), 1_000.into());
        let bar_one = builder.build_now();
        assert_eq!(bar_one.close, Price::from("105.00"));

        // Adjustment must persist across the reset triggered by build_now.
        assert!(builder.adjustment_active);

        builder.update(Price::from("110.00"), Quantity::from(1), 2_000.into());
        let bar_two = builder.build_now();
        assert_eq!(bar_two.close, Price::from("115.00"));
    }

    #[rstest]
    fn test_bar_builder_update_bar_applies_ratio_adjustment(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.set_adjustment(
            Decimal::new(11, 1), // 1.1
            ContinuousFutureAdjustmentType::ForwardRatio,
        );

        let input = Bar::new(
            bar_type,
            Price::from("100.00"),
            Price::from("110.00"),
            Price::from("90.00"),
            Price::from("105.00"),
            Quantity::from(10),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
        );
        builder.update_bar(input, input.volume, input.ts_init);

        let bar = builder.build_now();
        assert_eq!(bar.open, Price::from("110.00"));
        assert_eq!(bar.high, Price::from("121.00"));
        assert_eq!(bar.low, Price::from("99.00"));
        assert_eq!(bar.close, Price::from("115.50"));
    }

    #[rstest]
    fn test_bar_builder_spread_below_zero_representable(equity_aapl: Equity) {
        // Backward-spread offsets that push prices below zero must stay representable in PriceRaw
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(3, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let mut builder = BarBuilder::new(bar_type, 2, 0);

        builder.set_adjustment(
            Decimal::new(-15000, 2), // -150.00
            ContinuousFutureAdjustmentType::BackwardSpread,
        );

        builder.update(Price::from("100.00"), Quantity::from(1), 1_000.into());
        let bar = builder.build_now();
        assert_eq!(bar.close, Price::from("-50.00"));
        assert!(bar.close.raw < 0);
        assert_eq!(bar.close.precision, 2);
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
        // On `build`, if `close < low` the low is pulled down to close.
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
        let (handler, record) = recording_handler();

        let mut aggregator = TickBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let trade = TradeTick::default();
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 0);
    }

    #[rstest]
    fn test_tick_bar_aggregator_handle_trade_when_step_count_reached(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(3, BarAggregation::Tick, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = TickBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let trade = TradeTick::default();
        aggregator.handle_trade(trade);
        aggregator.handle_trade(trade);
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = TickBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = TickBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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

        let handler_guard = handler.lock();
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
    #[case(PriceType::Bid, Price::from("100.00"), Quantity::from(10))]
    #[case(PriceType::Ask, Price::from("102.00"), Quantity::from(14))]
    #[case(PriceType::Mid, Price::from("101.000"), Quantity::from("12.0"))]
    fn test_bar_aggregator_handle_quote_selects_price_and_size(
        equity_aapl: Equity,
        #[case] price_type: PriceType,
        #[case] expected_price: Price,
        #[case] expected_size: Quantity,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(1, BarAggregation::Tick, price_type),
            AggregationSource::Internal,
        );
        let (handler, record) = recording_handler();
        let mut aggregator = TickBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );
        let ts_init = UnixNanos::from(2_000);
        let quote = QuoteTick::new(
            instrument.id(),
            Price::from("100.00"),
            Price::from("102.00"),
            Quantity::from(10),
            Quantity::from(14),
            UnixNanos::from(1_000),
            ts_init,
        );

        aggregator.handle_quote(quote);

        let bars = handler.lock();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].open, expected_price);
        assert_eq!(bars[0].high, expected_price);
        assert_eq!(bars[0].low, expected_price);
        assert_eq!(bars[0].close, expected_price);
        assert_eq!(bars[0].volume, expected_size);
        assert_eq!(bars[0].ts_event, ts_init);
        assert_eq!(bars[0].ts_init, ts_init);
    }

    #[rstest]
    fn test_bar_aggregator_handle_quote_rejects_last_price(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(1, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        let (handler, record) = recording_handler();
        let mut aggregator = TickBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        aggregator.handle_quote(QuoteTick::new(
            instrument.id(),
            Price::from("100.00"),
            Price::from("102.00"),
            Quantity::from(10),
            Quantity::from(14),
            UnixNanos::from(1_000),
            UnixNanos::from(2_000),
        ));

        assert!(handler.lock().is_empty());
        assert!(!aggregator.core.builder.initialized);
        assert_eq!(aggregator.core.builder.count, 0);
        assert_eq!(aggregator.core.builder.volume, Quantity::zero(0));
    }

    #[rstest]
    fn test_non_time_bar_aggregators_use_historical_handler(
        equity_aapl: Equity,
        audusd_sim: CurrencyPair,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let make_sink = |bars: Arc<Mutex<Vec<Bar>>>| {
            move |bar: Bar| {
                bars.lock().push(bar);
            }
        };
        let make_trade = |price: &str, size: i64, ts: u64| TradeTick {
            instrument_id,
            price: Price::from(price),
            size: Quantity::from(size),
            aggressor_side: AggressorSide::Buy,
            ts_event: UnixNanos::from(ts),
            ts_init: UnixNanos::from(ts),
            ..TradeTick::default()
        };

        macro_rules! assert_historical_sink_receives {
            ($name:expr, $aggregator:expr, $update:expr) => {{
                let initial_bars = Arc::new(Mutex::new(Vec::new()));
                let historical_bars = Arc::new(Mutex::new(Vec::new()));
                let mut aggregator = $aggregator(Arc::clone(&initial_bars));
                aggregator
                    .set_historical_mode(true, Box::new(make_sink(Arc::clone(&historical_bars))));
                {
                    let aggregator: &mut dyn BarAggregator = &mut aggregator;
                    $update(aggregator);
                }

                assert_eq!(initial_bars.lock().len(), 0, "{}", $name,);
                assert_eq!(historical_bars.lock().len(), 1, "{}", $name,);
            }};
        }

        let tick_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
        );
        assert_historical_sink_receives!(
            "TickBarAggregator",
            |bars| TickBarAggregator::new(
                tick_type,
                price_precision,
                size_precision,
                make_sink(bars)
            ),
            |aggregator: &mut dyn BarAggregator| {
                aggregator.handle_trade(make_trade("100.00", 1, 1_000));
            }
        );

        let tick_imbalance_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::TickImbalance, PriceType::Last),
            AggregationSource::Internal,
        );
        assert_historical_sink_receives!(
            "TickImbalanceBarAggregator",
            |bars| TickImbalanceBarAggregator::new(
                tick_imbalance_type,
                price_precision,
                size_precision,
                make_sink(bars),
            ),
            |aggregator: &mut dyn BarAggregator| {
                aggregator.handle_trade(make_trade("100.00", 1, 1_000));
            }
        );

        let tick_runs_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::TickRuns, PriceType::Last),
            AggregationSource::Internal,
        );
        assert_historical_sink_receives!(
            "TickRunsBarAggregator",
            |bars| TickRunsBarAggregator::new(
                tick_runs_type,
                price_precision,
                size_precision,
                make_sink(bars),
            ),
            |aggregator: &mut dyn BarAggregator| {
                aggregator.handle_trade(make_trade("100.00", 1, 1_000));
            }
        );

        let volume_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Volume, PriceType::Last),
            AggregationSource::Internal,
        );
        assert_historical_sink_receives!(
            "VolumeBarAggregator",
            |bars| VolumeBarAggregator::new(
                volume_type,
                price_precision,
                size_precision,
                make_sink(bars),
            ),
            |aggregator: &mut dyn BarAggregator| {
                aggregator.handle_trade(make_trade("100.00", 1, 1_000));
            }
        );

        let volume_imbalance_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::VolumeImbalance, PriceType::Last),
            AggregationSource::Internal,
        );
        assert_historical_sink_receives!(
            "VolumeImbalanceBarAggregator",
            |bars| VolumeImbalanceBarAggregator::new(
                volume_imbalance_type,
                price_precision,
                size_precision,
                make_sink(bars),
            ),
            |aggregator: &mut dyn BarAggregator| {
                aggregator.handle_trade(make_trade("100.00", 1, 1_000));
            }
        );

        let volume_runs_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::VolumeRuns, PriceType::Last),
            AggregationSource::Internal,
        );
        assert_historical_sink_receives!(
            "VolumeRunsBarAggregator",
            |bars| VolumeRunsBarAggregator::new(
                volume_runs_type,
                price_precision,
                size_precision,
                make_sink(bars),
            ),
            |aggregator: &mut dyn BarAggregator| {
                aggregator.handle_trade(make_trade("100.00", 1, 1_000));
            }
        );

        let value_type = BarType::new(
            instrument_id,
            BarSpecification::new(100, BarAggregation::Value, PriceType::Last),
            AggregationSource::Internal,
        );
        assert_historical_sink_receives!(
            "ValueBarAggregator",
            |bars| ValueBarAggregator::new(
                value_type,
                price_precision,
                size_precision,
                make_sink(bars)
            ),
            |aggregator: &mut dyn BarAggregator| {
                aggregator.handle_trade(make_trade("100.00", 1, 1_000));
            }
        );

        let value_imbalance_type = BarType::new(
            instrument_id,
            BarSpecification::new(100, BarAggregation::ValueImbalance, PriceType::Last),
            AggregationSource::Internal,
        );
        assert_historical_sink_receives!(
            "ValueImbalanceBarAggregator",
            |bars| ValueImbalanceBarAggregator::new(
                value_imbalance_type,
                price_precision,
                size_precision,
                make_sink(bars),
            ),
            |aggregator: &mut dyn BarAggregator| {
                aggregator.handle_trade(make_trade("100.00", 1, 1_000));
            }
        );

        let value_runs_type = BarType::new(
            instrument_id,
            BarSpecification::new(100, BarAggregation::ValueRuns, PriceType::Last),
            AggregationSource::Internal,
        );
        assert_historical_sink_receives!(
            "ValueRunsBarAggregator",
            |bars| ValueRunsBarAggregator::new(
                value_runs_type,
                price_precision,
                size_precision,
                make_sink(bars),
            ),
            |aggregator: &mut dyn BarAggregator| {
                aggregator.handle_trade(make_trade("100.00", 1, 1_000));
            }
        );

        let fx = InstrumentAny::CurrencyPair(audusd_sim);
        let renko_type = BarType::new(
            fx.id(),
            BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid),
            AggregationSource::Internal,
        );
        let fx_price_precision = fx.price_precision();
        let fx_size_precision = fx.size_precision();
        let fx_price_increment = fx.price_increment();
        assert_historical_sink_receives!(
            "RenkoBarAggregator",
            |bars| RenkoBarAggregator::new(
                renko_type,
                fx_price_precision,
                fx_size_precision,
                fx_price_increment,
                make_sink(bars),
            ),
            |aggregator: &mut dyn BarAggregator| {
                aggregator.update(
                    Price::from("1.00000"),
                    Quantity::from(1),
                    UnixNanos::from(1_000),
                );
                aggregator.update(
                    Price::from("1.00010"),
                    Quantity::from(1),
                    UnixNanos::from(2_000),
                );
            }
        );
    }

    #[rstest]
    fn test_tick_imbalance_bar_aggregator_emits_at_threshold(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = TickImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let trade = TradeTick::default();
        aggregator.handle_trade(trade);
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.volume, Quantity::from(200000));
    }

    #[rstest]
    fn test_tick_imbalance_bar_aggregator_handles_seller_direction(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::TickImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = TickImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let sell = TradeTick {
            aggressor_side: AggressorSide::Sell,
            ..TradeTick::default()
        };

        aggregator.handle_trade(sell);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
    }

    #[rstest]
    fn test_tick_runs_bar_aggregator_resets_on_side_change(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = TickRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let buy = TradeTick {
            instrument_id: instrument.id(),
            price: Price::from("100.00"),
            size: Quantity::from(1),
            ts_event: UnixNanos::from(1_000),
            ts_init: UnixNanos::from(1_000),
            ..TradeTick::default()
        };
        let sell_one = TradeTick {
            price: Price::from("200.00"),
            size: Quantity::from(2),
            aggressor_side: AggressorSide::Sell,
            ts_event: UnixNanos::from(2_000),
            ts_init: UnixNanos::from(2_000),
            ..buy
        };
        let sell_two = TradeTick {
            price: Price::from("201.00"),
            size: Quantity::from(3),
            ts_event: UnixNanos::from(3_000),
            ts_init: UnixNanos::from(3_000),
            ..sell_one
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell_one);
        aggregator.handle_trade(sell_two);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].open, Price::from("200.00"));
        assert_eq!(handler_guard[0].high, Price::from("201.00"));
        assert_eq!(handler_guard[0].low, Price::from("200.00"));
        assert_eq!(handler_guard[0].close, Price::from("201.00"));
        assert_eq!(handler_guard[0].volume, Quantity::from(5));
        assert_eq!(handler_guard[0].ts_event, UnixNanos::from(3_000));
        assert_eq!(handler_guard[0].ts_init, UnixNanos::from(3_000));
    }

    #[rstest]
    fn test_tick_runs_bar_aggregator_volume_conservation(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = TickRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let buy = TradeTick {
            size: Quantity::from(1),
            ..TradeTick::default()
        };
        let sell = TradeTick {
            aggressor_side: AggressorSide::Sell,
            size: Quantity::from(1),
            ..buy
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);
        aggregator.handle_trade(sell);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
        assert_eq!(handler_guard[0].volume, Quantity::from(2));
        assert_eq!(handler_guard[1].volume, Quantity::from(2));
    }

    #[rstest]
    fn test_volume_bar_aggregator_builds_multiple_bars_from_large_update(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::Volume, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        aggregator.update(
            Price::from("1.00001"),
            Quantity::from(25),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        aggregator.update(
            Price::from("100.00"),
            Quantity::from(0),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 0);
    }

    #[rstest]
    fn test_volume_bar_aggregator_ignores_out_of_order_update(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::Volume, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        aggregator.update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::from(1_000),
        );
        aggregator.update(
            Price::from("200.00"),
            Quantity::from(3),
            UnixNanos::from(500),
        );

        let handler_guard = handler.lock();
        assert!(handler_guard.is_empty());
        assert_eq!(aggregator.core.builder.count, 1);
        assert_eq!(aggregator.core.builder.volume, Quantity::from(1));
        assert_eq!(aggregator.core.builder.close, Some(Price::from("100.00")));
        assert_eq!(aggregator.core.builder.ts_last, UnixNanos::from(1_000));
    }

    #[rstest]
    fn test_volume_bar_aggregator_ignores_out_of_order_bar(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::Volume, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        aggregator.update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::from(1_000),
        );
        let stale_bar = Bar::new(
            bar_type,
            Price::from("200.00"),
            Price::from("201.00"),
            Price::from("199.00"),
            Price::from("200.50"),
            Quantity::from(3),
            UnixNanos::from(500),
            UnixNanos::from(500),
        );
        aggregator.update_bar(stale_bar, stale_bar.volume, stale_bar.ts_init);

        let handler_guard = handler.lock();
        assert!(handler_guard.is_empty());
        assert_eq!(aggregator.core.builder.count, 1);
        assert_eq!(aggregator.core.builder.volume, Quantity::from(1));
        assert_eq!(aggregator.core.builder.close, Some(Price::from("100.00")));
        assert_eq!(aggregator.core.builder.ts_last, UnixNanos::from(1_000));
    }

    #[rstest]
    fn test_volume_imbalance_bar_aggregator_ignores_out_of_order_trade(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::VolumeImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();
        let mut aggregator = VolumeImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );
        let first = TradeTick {
            price: Price::from("100.00"),
            size: Quantity::from(1),
            aggressor_side: AggressorSide::Buy,
            ts_init: UnixNanos::from(1_000),
            ..TradeTick::default()
        };
        let stale = TradeTick {
            price: Price::from("200.00"),
            size: Quantity::from(2),
            aggressor_side: AggressorSide::Buy,
            ts_init: UnixNanos::from(500),
            ..TradeTick::default()
        };

        aggregator.handle_trade(first);
        aggregator.handle_trade(stale);

        assert!(handler.lock().is_empty());
        assert_eq!(aggregator.imbalance_raw, Quantity::from(1).raw as i128);
        assert_eq!(aggregator.core.builder.volume, Quantity::from(1));
        assert_eq!(aggregator.core.builder.ts_last, UnixNanos::from(1_000));
    }

    #[rstest]
    #[case(BarAggregation::TickImbalance)]
    #[case(BarAggregation::TickRuns)]
    #[case(BarAggregation::VolumeRuns)]
    #[case(BarAggregation::ValueImbalance)]
    #[case(BarAggregation::ValueRuns)]
    fn test_stateful_trade_aggregators_ignore_out_of_order_trade(
        equity_aapl: Equity,
        #[case] aggregation: BarAggregation,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let (step, price) = match aggregation {
            BarAggregation::ValueImbalance | BarAggregation::ValueRuns => {
                (100, Price::from("50.00"))
            }
            _ => (2, Price::from("100.00")),
        };
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(step, aggregation, PriceType::Last),
            AggregationSource::Internal,
        );
        let (handler, record) = recording_handler();
        let make_handler = record;
        let mut aggregator: Box<dyn BarAggregator> = match aggregation {
            BarAggregation::TickImbalance => Box::new(TickImbalanceBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                make_handler,
            )),
            BarAggregation::TickRuns => Box::new(TickRunsBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                make_handler,
            )),
            BarAggregation::VolumeRuns => Box::new(VolumeRunsBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                make_handler,
            )),
            BarAggregation::ValueImbalance => Box::new(ValueImbalanceBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                make_handler,
            )),
            BarAggregation::ValueRuns => Box::new(ValueRunsBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                make_handler,
            )),
            _ => unreachable!(),
        };
        let first = TradeTick {
            instrument_id: instrument.id(),
            price,
            size: Quantity::from(1),
            aggressor_side: AggressorSide::Buy,
            ts_event: UnixNanos::from(1_000),
            ts_init: UnixNanos::from(1_000),
            ..TradeTick::default()
        };
        let stale = TradeTick {
            price: Price::from("999.00"),
            ts_event: UnixNanos::from(500),
            ts_init: UnixNanos::from(500),
            ..first
        };
        let second = TradeTick {
            ts_event: UnixNanos::from(2_000),
            ts_init: UnixNanos::from(2_000),
            ..first
        };

        aggregator.handle_trade(first);
        aggregator.handle_trade(stale);
        aggregator.handle_trade(second);

        let bars = handler.lock();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].open, price);
        assert_eq!(bars[0].high, price);
        assert_eq!(bars[0].low, price);
        assert_eq!(bars[0].close, price);
        assert_eq!(bars[0].volume, Quantity::from(2));
        assert_eq!(bars[0].ts_event, UnixNanos::from(2_000));
        assert_eq!(bars[0].ts_init, UnixNanos::from(2_000));
    }

    #[rstest]
    fn test_volume_bar_aggregator_exact_threshold_emits_single_bar(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::Volume, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].volume, Quantity::from(10));
        assert_eq!(handler_guard[0].close, Price::from("101.00"));
    }

    #[rstest]
    fn test_volume_bar_aggregator_step_of_one_emits_per_unit(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Volume, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        aggregator.update(
            Price::from("100.00"),
            Quantity::from(1),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].volume, Quantity::from(1));
    }

    #[rstest]
    fn test_volume_runs_bar_aggregator_side_change_resets(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::VolumeRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let buy = TradeTick {
            instrument_id: instrument.id(),
            price: Price::from("100.00"),
            size: Quantity::from(4),
            ts_event: UnixNanos::from(1_000),
            ts_init: UnixNanos::from(1_000),
            ..TradeTick::default()
        };
        let sell_one = TradeTick {
            price: Price::from("200.00"),
            size: Quantity::from(6),
            aggressor_side: AggressorSide::Sell,
            ts_event: UnixNanos::from(2_000),
            ts_init: UnixNanos::from(2_000),
            ..buy
        };
        let sell_two = TradeTick {
            price: Price::from("201.00"),
            size: Quantity::from(4),
            ts_event: UnixNanos::from(3_000),
            ts_init: UnixNanos::from(3_000),
            ..sell_one
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell_one);
        aggregator.handle_trade(sell_two);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].open, Price::from("200.00"));
        assert_eq!(handler_guard[0].high, Price::from("201.00"));
        assert_eq!(handler_guard[0].low, Price::from("200.00"));
        assert_eq!(handler_guard[0].close, Price::from("201.00"));
        assert_eq!(handler_guard[0].volume, Quantity::from(10));
        assert_eq!(handler_guard[0].ts_event, UnixNanos::from(3_000));
        assert_eq!(handler_guard[0].ts_init, UnixNanos::from(3_000));
    }

    #[rstest]
    fn test_volume_runs_bar_aggregator_handles_large_single_trade(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(3, BarAggregation::VolumeRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let trade = TradeTick {
            instrument_id: instrument.id(),
            price: Price::from("1.0"),
            size: Quantity::from(5),
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
        assert!(!handler_guard.is_empty());
        assert!(handler_guard[0].volume.as_f64() > 0.0);
        assert!(handler_guard[0].volume.as_f64() < trade.size.as_f64());
    }

    #[rstest]
    fn test_volume_imbalance_bar_aggregator_splits_large_trade(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::VolumeImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.volume, Quantity::from(10));
    }

    #[rstest]
    fn test_value_bar_aggregator_handles_large_update(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1000, BarAggregation::Value, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        // Single large update: $100 * 25 = $2500 (should create 2 bars)
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(25),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
        let remaining_value = aggregator.get_cumulative_value();
        assert!(remaining_value < Decimal::from(1_000)); // Should be less than threshold
    }

    #[rstest]
    fn test_value_bar_aggregator_handles_zero_price(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1000, BarAggregation::Value, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        // Update with zero price should not cause division by zero
        aggregator.update(
            Price::from("0.00"),
            Quantity::from(100),
            UnixNanos::default(),
        );

        // No bars should be emitted since value is zero
        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 0);

        // Cumulative value should remain zero
        assert_eq!(aggregator.get_cumulative_value(), Decimal::ZERO);
    }

    #[rstest]
    fn test_value_bar_aggregator_handles_zero_size(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1000, BarAggregation::Value, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        // Update with zero size should not cause issues
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(0),
            UnixNanos::default(),
        );

        // No bars should be emitted
        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 0);

        // Cumulative value should remain zero
        assert_eq!(aggregator.get_cumulative_value(), Decimal::ZERO);
    }

    #[rstest]
    fn test_value_bar_aggregator_conserves_volume_across_rounded_chunks(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::Value, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        // Step 10 at price 3.00 needs fractional 3.33... chunks; the rounded
        // 3-unit chunks must still conserve the 10 input units (3 + 3 + 3 + 1)
        aggregator.update(
            Price::from("3.00"),
            Quantity::from(10),
            UnixNanos::from(1_000),
        );

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 3);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from(3));
        }
        assert_eq!(aggregator.core.builder.volume, Quantity::from(1));
    }

    #[rstest]
    fn test_value_bar_aggregator_update_bar_conserves_volume_across_rounded_chunks(
        equity_aapl: Equity,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::Value, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        // Average price 3.00 with volume 10 mirrors the tick-path conservation case
        let input_bar = Bar::new(
            bar_type,
            Price::from("3.00"),
            Price::from("3.00"),
            Price::from("3.00"),
            Price::from("3.00"),
            Quantity::from(10),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
        );
        aggregator.handle_bar(input_bar);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 3);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from(3));
        }
        assert_eq!(aggregator.core.builder.volume, Quantity::from(1));
    }

    #[rstest]
    fn test_value_bar_aggregator_exact_threshold_emits_one_bar(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1000, BarAggregation::Value, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].volume, Quantity::from(10));
        assert_eq!(aggregator.get_cumulative_value(), Decimal::ZERO);
    }

    #[rstest]
    fn test_value_bar_aggregator_precision_boundary_min_size_clamp(equity_aapl: Equity) {
        // step=100, price=100 per-unit value=100 with size_precision=0 lands the divided
        // size_chunk at the precision floor. Verifies the min-size clamp branch in update()
        // emits one bar per unit rather than looping on zero-volume chunks.
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::Value, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        // 4 units at $100 = $400 value, with step $100 gives 4 bars exactly.
        aggregator.update(
            Price::from("100.00"),
            Quantity::from(4),
            UnixNanos::default(),
        );

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = ValueImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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
            aggressor_side: AggressorSide::Sell,
            instrument_id: instrument.id(),
            ..buy
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_emits_on_consecutive_side(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let trade = TradeTick {
            price: Price::from("10.0"),
            size: Quantity::from(5),
            instrument_id: instrument.id(),
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.volume, Quantity::from(10));
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_resets_on_side_change(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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
            aggressor_side: AggressorSide::Sell,
            ..buy
        }; // value 100

        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].volume, Quantity::from(10));
    }

    #[rstest]
    fn test_tick_runs_bar_aggregator_continues_run_after_bar_emission(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = TickRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let buy = TradeTick::default();

        aggregator.handle_trade(buy);
        aggregator.handle_trade(buy); // Emit bar 1 (run complete)
        aggregator.handle_trade(buy); // Start new run
        aggregator.handle_trade(buy); // Emit bar 2 (new run complete)

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_tick_runs_bar_aggregator_handles_no_aggressor_trades(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = TickRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
    }

    #[rstest]
    fn test_volume_runs_bar_aggregator_continues_run_after_bar_emission(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::VolumeRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
        assert_eq!(handler_guard[0].volume, Quantity::from(2));
        assert_eq!(handler_guard[1].volume, Quantity::from(2));
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_continues_run_after_bar_emission(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        // First test with build_with_no_updates = false
        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            record,
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

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 0); // No bar should be built without updates
        drop(handler_guard);

        // Now test with build_with_no_updates = true
        let (handler, record) = recording_handler();
        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            record,
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

        let handler_guard = handler.lock();
        let bar = handler_guard.first().unwrap();
        assert_eq!(bar.ts_event, UnixNanos::default());
        assert_eq!(bar.ts_init, ts2);
    }

    #[rstest]
    fn test_renko_bar_aggregator_initialization(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (_handler, record) = recording_handler();

        let aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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
        let (handler, record) = recording_handler();

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 0); // No bar created yet
    }

    #[rstest]
    fn test_renko_bar_aggregator_ignores_out_of_order_bar(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();
        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
        );
        let first = Bar::new(
            bar_type,
            Price::from("1.00000"),
            Price::from("1.00000"),
            Price::from("1.00000"),
            Price::from("1.00000"),
            Quantity::from(1),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
        );
        let stale = Bar::new(
            bar_type,
            Price::from("1.00020"),
            Price::from("1.00020"),
            Price::from("1.00020"),
            Price::from("1.00020"),
            Quantity::from(1),
            UnixNanos::from(500),
            UnixNanos::from(500),
        );

        aggregator.update_bar(first, first.volume, first.ts_init);
        aggregator.update_bar(stale, stale.volume, stale.ts_init);

        assert!(handler.lock().is_empty());
        assert_eq!(aggregator.last_close, Some(Price::from("1.00000")));
        assert_eq!(aggregator.core.builder.ts_last, UnixNanos::from(1_000));
    }

    #[rstest]
    fn test_renko_bar_aggregator_update_exceeds_brick_size_creates_bar(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 0); // No bar created yet
    }

    #[rstest]
    fn test_renko_bar_aggregator_handle_bar_exceeds_brick_size(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let bar_spec = BarSpecification::new(10, BarAggregation::Renko, PriceType::Mid); // 10 pip brick size
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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

        let handler_guard = handler.lock();
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
        let (_handler_5, record) = recording_handler();

        let aggregator_5 = RenkoBarAggregator::new(
            bar_type_5,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
        );

        // 5 pips * price_increment.raw (depends on precision mode)
        let expected_brick_size_5 = 5 * instrument.price_increment().raw;
        assert_eq!(aggregator_5.brick_size, expected_brick_size_5);

        let bar_spec_20 = BarSpecification::new(20, BarAggregation::Renko, PriceType::Mid); // 20 pip brick size
        let bar_type_20 = BarType::new(instrument.id(), bar_spec_20, AggregationSource::Internal);
        let (_handler_20, record) = recording_handler();

        let aggregator_20 = RenkoBarAggregator::new(
            bar_type_20,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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
        let (handler, record) = recording_handler();

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = RenkoBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            record,
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

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = TickImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let buy = TradeTick {
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };
        let sell = TradeTick {
            aggressor_side: AggressorSide::Sell,
            ..TradeTick::default()
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);
        aggregator.handle_trade(buy);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 0);
    }

    #[rstest]
    fn test_tick_imbalance_bar_aggregator_no_aggressor_ignored(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = TickImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let buy = TradeTick {
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };
        let no_aggressor = TradeTick {
            aggressor_side: AggressorSide::NoAggressor,
            ..TradeTick::default()
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(no_aggressor);
        aggregator.handle_trade(buy);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
    }

    #[rstest]
    fn test_tick_runs_bar_aggregator_multiple_consecutive_runs(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(2, BarAggregation::TickRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = TickRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let buy = TradeTick {
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };
        let sell = TradeTick {
            aggressor_side: AggressorSide::Sell,
            ..TradeTick::default()
        };

        aggregator.handle_trade(buy);
        aggregator.handle_trade(buy);
        aggregator.handle_trade(sell);
        aggregator.handle_trade(sell);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_volume_imbalance_bar_aggregator_large_trade_spans_bars(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::VolumeImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let large_trade = TradeTick {
            size: Quantity::from(25),
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };

        aggregator.handle_trade(large_trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_volume_imbalance_bar_aggregator_no_aggressor_does_not_affect_imbalance(
        equity_aapl: Equity,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::VolumeImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let buy = TradeTick {
            size: Quantity::from(5),
            aggressor_side: AggressorSide::Buy,
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

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
    }

    #[rstest]
    fn test_volume_runs_bar_aggregator_large_trade_spans_bars(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(10, BarAggregation::VolumeRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let large_trade = TradeTick {
            size: Quantity::from(25),
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };

        aggregator.handle_trade(large_trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_large_trade_spans_bars(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(50, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let large_trade = TradeTick {
            price: Price::from("5.00"),
            size: Quantity::from(25),
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };

        aggregator.handle_trade(large_trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_keeps_leftover_volume_for_same_side_run(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        // First trade spans one bar (value 150 = step 100 + 50 leftover), the
        // leftover 5 units must survive as the start of a new same-side run.
        let first = TradeTick {
            price: Price::from("10.00"),
            size: Quantity::from(15),
            aggressor_side: AggressorSide::Sell,
            ts_event: UnixNanos::from(1_000),
            ts_init: UnixNanos::from(1_000),
            ..TradeTick::default()
        };
        aggregator.handle_trade(first);

        // Second same-side trade completes the run (50 + 50 >= 100).
        let second = TradeTick {
            price: Price::from("10.00"),
            size: Quantity::from(5),
            aggressor_side: AggressorSide::Sell,
            ts_event: UnixNanos::from(2_000),
            ts_init: UnixNanos::from(2_000),
            ..TradeTick::default()
        };
        aggregator.handle_trade(second);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
        assert_eq!(handler_guard[0].volume, Quantity::from(10));
        assert_eq!(handler_guard[1].volume, Quantity::from(10));
    }

    #[rstest]
    fn test_value_bar_high_price_low_step_no_zero_volume_bars(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::Value, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        // price=1000, size=3, value=3000, step=100 → size_chunk=0.1 rounds to 0 at precision 0
        aggregator.update(
            Price::from("1000.00"),
            Quantity::from(3),
            UnixNanos::default(),
        );

        // 3 bars (one per min-size unit), not 30 zero-volume bars
        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = ValueImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let trade = TradeTick {
            price: Price::from("1000.00"),
            size: Quantity::from(3),
            aggressor_side: AggressorSide::Buy,
            instrument_id: instrument.id(),
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = ValueImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        // Build seller imbalance of -50 (below step=100, no bar yet)
        let sell_tick = TradeTick {
            price: Price::from("10.00"),
            size: Quantity::from(5),
            aggressor_side: AggressorSide::Sell,
            instrument_id: instrument.id(),
            ..TradeTick::default()
        };

        // Opposite-side buyer: flatten amount 50/1000=0.05 < min_size (1),
        // clamp overshoots imbalance from -50 to +950, crossing threshold
        let buy_tick = TradeTick {
            price: Price::from("1000.00"),
            size: Quantity::from(1),
            aggressor_side: AggressorSide::Buy,
            instrument_id: instrument.id(),
            ts_init: UnixNanos::from(1),
            ts_event: UnixNanos::from(1),
            ..TradeTick::default()
        };

        aggregator.handle_trade(sell_tick);
        aggregator.handle_trade(buy_tick);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].volume, Quantity::from(6));
    }

    #[rstest]
    fn test_value_runs_high_price_low_step_no_zero_volume_bars(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let trade = TradeTick {
            price: Price::from("1000.00"),
            size: Quantity::from(3),
            aggressor_side: AggressorSide::Buy,
            instrument_id: instrument.id(),
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 3);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from(1));
        }
    }

    #[rstest]
    fn test_value_imbalance_bar_aggregator_exact_below_step_retains_pending() {
        // step=9_007_199_254; a single buy of 9007199253.999999999 @ price 1 has a notional
        // exactly one raw unit below the step. Exact Decimal arithmetic must NOT emit a bar; the
        // prior f64 path rounded the size up to 9007199254.0 and emitted early.
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let bar_spec = BarSpecification::new(
            9_007_199_254,
            BarAggregation::ValueImbalance,
            PriceType::Last,
        );
        let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueImbalanceBarAggregator::new(bar_type, 0, 9, record);

        let below_step = TradeTick {
            instrument_id,
            price: Price::from("1"),
            size: Quantity::from("9007199253.999999999"),
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };
        aggregator.handle_trade(below_step);

        assert!(handler.lock().is_empty());
        assert_eq!(
            aggregator.core.builder.volume,
            Quantity::from("9007199253.999999999"),
        );

        // One additional raw unit lifts the notional to exactly the step, emitting one bar whose
        // volume is the exact total raw input.
        let one_raw_unit = TradeTick {
            instrument_id,
            price: Price::from("1"),
            size: Quantity::from("0.000000001"),
            aggressor_side: AggressorSide::Buy,
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
            ..TradeTick::default()
        };
        aggregator.handle_trade(one_raw_unit);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(
            handler_guard[0].volume,
            Quantity::from("9007199254.000000000")
        );
        assert_eq!(aggregator.core.builder.volume, Quantity::zero(9));
    }

    #[rstest]
    fn test_value_imbalance_bar_aggregator_conserves_volume_across_split_bars() {
        // step=4, price=1: a same-side buy of 10.000000003 splits into two full bars of value 4
        // and leaves a fractional 2.000000003 pending. Emitted plus pending volume must equal the
        // exact input across the several split bars.
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let bar_spec = BarSpecification::new(4, BarAggregation::ValueImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueImbalanceBarAggregator::new(bar_type, 0, 9, record);

        let input = Quantity::from("10.000000003");
        let trade = TradeTick {
            instrument_id,
            price: Price::from("1"),
            size: input,
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from("4.000000000"));
        }
        assert_eq!(
            aggregator.core.builder.volume,
            Quantity::from("2.000000003"),
        );
        let emitted_plus_pending = handler_guard
            .iter()
            .map(|bar| bar.volume.as_decimal())
            .sum::<Decimal>()
            + aggregator.core.builder.volume.as_decimal();
        assert_eq!(emitted_plus_pending, input.as_decimal());
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_exact_below_step_retains_pending() {
        // step=9_007_199_254; a single buy of 9007199253.999999999 @ price 1 sits one raw unit
        // below the step. Exact Decimal arithmetic must NOT emit a bar; the prior f64 path rounded
        // the size up and emitted early.
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let bar_spec =
            BarSpecification::new(9_007_199_254, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueRunsBarAggregator::new(bar_type, 0, 9, record);

        let below_step = TradeTick {
            instrument_id,
            price: Price::from("1"),
            size: Quantity::from("9007199253.999999999"),
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };
        aggregator.handle_trade(below_step);

        assert!(handler.lock().is_empty());
        assert_eq!(
            aggregator.core.builder.volume,
            Quantity::from("9007199253.999999999"),
        );

        // One additional same-side raw unit completes the run at exactly the step, emitting one bar
        // whose volume is the exact total raw input.
        let one_raw_unit = TradeTick {
            instrument_id,
            price: Price::from("1"),
            size: Quantity::from("0.000000001"),
            aggressor_side: AggressorSide::Buy,
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
            ..TradeTick::default()
        };
        aggregator.handle_trade(one_raw_unit);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(
            handler_guard[0].volume,
            Quantity::from("9007199254.000000000")
        );
        assert_eq!(aggregator.core.builder.volume, Quantity::zero(9));
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_conserves_volume_across_split_bars() {
        // step=4, price=1: a same-side buy of 10.000000003 splits into two full bars of value 4 and
        // keeps a fractional 2.000000003 as the leftover of the same-side run. Emitted plus pending
        // volume must equal the exact input across the several split bars.
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let bar_spec = BarSpecification::new(4, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueRunsBarAggregator::new(bar_type, 0, 9, record);

        let input = Quantity::from("10.000000003");
        let trade = TradeTick {
            instrument_id,
            price: Price::from("1"),
            size: input,
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 2);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from("4.000000000"));
        }
        assert_eq!(
            aggregator.core.builder.volume,
            Quantity::from("2.000000003"),
        );
        let emitted_plus_pending = handler_guard
            .iter()
            .map(|bar| bar.volume.as_decimal())
            .sum::<Decimal>()
            + aggregator.core.builder.volume.as_decimal();
        assert_eq!(emitted_plus_pending, input.as_decimal());
    }

    #[rstest]
    fn test_value_imbalance_bar_aggregator_no_aggressor_and_zero_price_fall_back_to_plain_volume() {
        // NoAggressor and zero-price trades carry no usable side signal, so they bypass imbalance
        // splitting and accumulate as plain builder volume without emitting a bar.
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueImbalanceBarAggregator::new(bar_type, 2, 0, record);

        let no_aggressor = TradeTick {
            instrument_id,
            price: Price::from("10.00"),
            size: Quantity::from(3),
            aggressor_side: AggressorSide::NoAggressor,
            ..TradeTick::default()
        };
        let zero_price = TradeTick {
            instrument_id,
            price: Price::from("0.00"),
            size: Quantity::from(4),
            aggressor_side: AggressorSide::Buy,
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
            ..TradeTick::default()
        };
        aggregator.handle_trade(no_aggressor);
        aggregator.handle_trade(zero_price);

        assert!(handler.lock().is_empty());
        assert_eq!(aggregator.core.builder.volume, Quantity::from(7));
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_no_aggressor_and_zero_price_fall_back_to_plain_volume() {
        // NoAggressor and zero-price trades carry no usable side signal, so they bypass the run
        // splitting and accumulate as plain builder volume without emitting a bar or resetting.
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let bar_spec = BarSpecification::new(100, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueRunsBarAggregator::new(bar_type, 2, 0, record);

        let no_aggressor = TradeTick {
            instrument_id,
            price: Price::from("10.00"),
            size: Quantity::from(3),
            aggressor_side: AggressorSide::NoAggressor,
            ..TradeTick::default()
        };
        let zero_price = TradeTick {
            instrument_id,
            price: Price::from("0.00"),
            size: Quantity::from(4),
            aggressor_side: AggressorSide::Buy,
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
            ..TradeTick::default()
        };
        aggregator.handle_trade(no_aggressor);
        aggregator.handle_trade(zero_price);

        assert!(handler.lock().is_empty());
        assert_eq!(aggregator.core.builder.volume, Quantity::from(7));
    }

    #[rstest]
    fn test_value_imbalance_bar_aggregator_conserves_volume_with_indivisible_price() {
        // step=1, price=3, size precision 1: the ideal split 1/3 rounds to 0.3, so each emitted bar
        // carries a notional of 0.9 (below the step) exactly as the reference ValueBarAggregator
        // does with a non-dividing price. Per-bar notional is approximate by design, but total
        // volume (emitted plus pending) must still equal the exact input.
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let bar_spec = BarSpecification::new(1, BarAggregation::ValueImbalance, PriceType::Last);
        let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueImbalanceBarAggregator::new(bar_type, 2, 1, record);

        let input = Quantity::from("1.0");
        let trade = TradeTick {
            instrument_id,
            price: Price::from("3.00"),
            size: input,
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 3);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from("0.3"));
        }
        assert_eq!(aggregator.core.builder.volume, Quantity::from("0.1"));
        let emitted_plus_pending = handler_guard
            .iter()
            .map(|bar| bar.volume.as_decimal())
            .sum::<Decimal>()
            + aggregator.core.builder.volume.as_decimal();
        assert_eq!(emitted_plus_pending, input.as_decimal());
    }

    #[rstest]
    fn test_value_runs_bar_aggregator_conserves_volume_with_indivisible_price() {
        // step=1, price=3, size precision 1: the ideal split 1/3 rounds to 0.3, so each emitted bar
        // carries a notional of 0.9 (below the step) exactly as the reference ValueBarAggregator
        // does with a non-dividing price. Per-bar notional is approximate by design, but total
        // volume (emitted plus pending) must still equal the exact input.
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let bar_spec = BarSpecification::new(1, BarAggregation::ValueRuns, PriceType::Last);
        let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();

        let mut aggregator = ValueRunsBarAggregator::new(bar_type, 2, 1, record);

        let input = Quantity::from("1.0");
        let trade = TradeTick {
            instrument_id,
            price: Price::from("3.00"),
            size: input,
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };
        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 3);
        for bar in handler_guard.iter() {
            assert_eq!(bar.volume, Quantity::from("0.3"));
        }
        assert_eq!(aggregator.core.builder.volume, Quantity::from("0.1"));
        let emitted_plus_pending = handler_guard
            .iter()
            .map(|bar| bar.volume.as_decimal())
            .sum::<Decimal>()
            + aggregator.core.builder.volume.as_decimal();
        assert_eq!(emitted_plus_pending, input.as_decimal());
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
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeImbalanceBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let trade = TradeTick {
            size: Quantity::from(step * 2),
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
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
            let (handler, record) = recording_handler();

            let mut aggregator = VolumeImbalanceBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                record,
            );

            let trade = TradeTick {
                size: Quantity::from(total_volume),
                aggressor_side: AggressorSide::Buy,
                ..TradeTick::default()
            };

            aggregator.handle_trade(trade);

            let handler_guard = handler.lock();
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
        let (handler, record) = recording_handler();

        let mut aggregator = VolumeRunsBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let trade = TradeTick {
            size: Quantity::from(step * 2),
            aggressor_side: AggressorSide::Buy,
            ..TradeTick::default()
        };

        aggregator.handle_trade(trade);

        let handler_guard = handler.lock();
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
            let (handler, record) = recording_handler();

            let mut aggregator = VolumeRunsBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                record,
            );

            let trade = TradeTick {
                size: Quantity::from(total_volume),
                aggressor_side: AggressorSide::Buy,
                ..TradeTick::default()
            };

            aggregator.handle_trade(trade);

            let handler_guard = handler.lock();
            results.push(handler_guard.len());
        }

        assert_eq!(results[0], 3); // 3000 / 1000
        assert_eq!(results[1], 2); // 3000 / 1500
        assert_ne!(results[0], results[1]);
    }

    /// Historical time-bar: event at `ts_init` is deferred until after the update.
    #[rstest]
    fn test_time_bar_historical_defers_event_at_ts_init_until_after_update(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut agg = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            record,
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

        let bars = handler.lock();
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
    #[case(10.03, 10.07, Price::from("10.00"), Price::from("10.10"))]
    #[case(-10.07, -10.03, Price::from("-10.10"), Price::from("-10.00"))]
    fn test_fixed_tick_scheme_rounder_rounds_bid_and_ask_outward(
        #[case] raw_bid: f64,
        #[case] raw_ask: f64,
        #[case] expected_bid: Price,
        #[case] expected_ask: Price,
    ) {
        let rounder = FixedTickSchemeRounder::new(0.05).unwrap();

        let (bid, ask) = rounder.round_prices(raw_bid, raw_ask, 2);

        assert_eq!(bid, expected_bid);
        assert_eq!(ask, expected_ask);
    }

    #[rstest]
    fn test_spread_quote_quote_driven_emits_when_all_legs_received(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(record),
            clock,
            false,
            None,
            0,
            false,
            60,
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
        assert_eq!(handler.lock().len(), 0);

        agg.handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("99.00"),
            Price::from("99.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts,
            ts,
        ));
        let quotes = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(record),
            clock,
            false,
            None,
            0,
            false,
            60,
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
        let quotes = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(record),
            clock,
            false,
            None,
            0,
            false,
            60,
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
        let quotes = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(0));

        let agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(record),
            clock.clone(),
            false,
            Some(1),
            0,
            false,
            60,
            None,
            None,
        );
        let rc = Rc::new(RefCell::new(agg));
        rc.borrow_mut().prepare_for_timer_mode(&rc);
        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

        for event in clock.borrow_mut().advance_time(UnixNanos::from(0), true) {
            rc.borrow_mut().on_timer_fire(event.ts_event);
        }
        assert_eq!(handler.lock().len(), 0);

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
            let quotes = handler.lock();
            assert_eq!(quotes.len(), 1);
            assert_eq!(quotes[0].ts_event, ts1);
            assert_eq!(quotes[0].ts_init, ts1);
        }

        let ts2 = UnixNanos::from(2_000_000_000);
        for event in clock.borrow_mut().advance_time(ts2, true) {
            rc.borrow_mut().on_timer_fire(event.ts_event);
        }

        let quotes = handler.lock();
        assert_eq!(quotes.len(), 1);
    }

    #[rstest]
    fn test_spread_quote_historical_timer_waits_for_all_legs(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(record),
            // need clock for set_clock after
            clock.clone(),
            true,
            Some(1),
            0,
            false,
            60,
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
        assert_eq!(handler.lock().len(), 0);

        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("99.00"),
            Price::from("99.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts2,
            ts2,
        ));
        assert_eq!(handler.lock().len(), 0);

        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("100.00"),
            Price::from("100.10"),
            Quantity::from(10),
            Quantity::from(10),
            ts3,
            ts3,
        ));
        let quotes = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            instrument.price_precision(),
            0,
            Box::new(record),
            // need clock for set_clock after
            clock.clone(),
            true,
            Some(1),
            0,
            false,
            60,
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

        assert_eq!(handler.lock().len(), 0);

        rc.borrow_mut().flush_pending_historical_quote();

        let quotes = handler.lock();
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
        let (handler, record) = recording_handler();
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
            Box::new(record),
            clock,
            false,
            None,
            0,
            false,
            60,
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
        let quotes = handler.lock();
        assert_eq!(quotes.len(), 1);
        let q = &quotes[0];
        assert_eq!(q.instrument_id, spread_id);
        assert_eq!(q.bid_price, Price::from("-1.02"));
        assert_eq!(q.ask_price, Price::from("-0.98"));
        assert_eq!(q.bid_size, Quantity::from(100));
        assert_eq!(q.ask_size, Quantity::from(100));
        assert_eq!(q.ts_event, ts);
        assert_eq!(q.ts_init, ts);
    }

    #[rstest]
    fn test_spread_quote_all_zero_vega_fallback(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut vega_provider = MapVegaProvider::new();
        vega_provider.insert(leg1, 0.0);
        vega_provider.insert(leg2, 0.0);

        let agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            false,
            instrument.price_precision(),
            0,
            Box::new(record),
            clock.clone(),
            false,
            None,
            0,
            false,
            1,
            Some(Box::new(vega_provider)),
            None,
        );
        let rc = Rc::new(RefCell::new(agg));
        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

        let ts = UnixNanos::from(1_000_000_000);
        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("10.00"),
            Price::from("10.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("20.00"),
            Price::from("20.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        {
            let quotes = handler.lock();
            assert_eq!(quotes.len(), 1);
            let q = &quotes[0];
            assert_eq!(q.bid_price, Price::from("-10.10"));
            assert_eq!(q.ask_price, Price::from("-9.90"));
        }
        assert!(rc.borrow().vega_pricing_temporarily_disabled);

        let timeout_name = rc.borrow().vega_pricing_timeout_timer_name.clone();
        assert!(
            clock
                .borrow()
                .timer_names()
                .contains(&timeout_name.as_str())
        );

        let events = clock
            .borrow_mut()
            .advance_time(UnixNanos::from(2_000_000_000), true);

        for handler in clock.borrow().match_handlers(events) {
            handler.run();
        }

        assert!(!rc.borrow().vega_pricing_temporarily_disabled);

        let (_cancel_handler, record) = recording_handler();
        let mut cancel_vega_provider = MapVegaProvider::new();
        cancel_vega_provider.insert(leg1, 0.0);
        cancel_vega_provider.insert(leg2, 0.0);
        let cancel_agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            false,
            instrument.price_precision(),
            0,
            Box::new(record),
            clock.clone(),
            false,
            None,
            0,
            false,
            10,
            Some(Box::new(cancel_vega_provider)),
            None,
        );
        let cancel_rc = Rc::new(RefCell::new(cancel_agg));
        cancel_rc
            .borrow_mut()
            .start_timer(Some(Rc::clone(&cancel_rc)));
        cancel_rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("10.00"),
            Price::from("10.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        cancel_rc.borrow_mut().handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("20.00"),
            Price::from("20.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        let cancel_timeout_name = cancel_rc.borrow().vega_pricing_timeout_timer_name.clone();
        assert!(
            clock
                .borrow()
                .timer_names()
                .contains(&cancel_timeout_name.as_str())
        );
        cancel_rc.borrow_mut().stop_timer();
        assert!(
            !clock
                .borrow()
                .timer_names()
                .contains(&cancel_timeout_name.as_str())
        );

        let (permanent_handler, record) = recording_handler();
        let mut permanent_vega_provider = MapVegaProvider::new();
        permanent_vega_provider.insert(leg1, 0.15);
        permanent_vega_provider.insert(leg2, 0.12);
        let mut permanent_agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            false,
            instrument.price_precision(),
            0,
            Box::new(record),
            Rc::new(RefCell::new(TestClock::new())),
            false,
            None,
            0,
            true,
            1,
            Some(Box::new(permanent_vega_provider)),
            None,
        );

        permanent_agg.handle_quote_tick(QuoteTick::new(
            leg1,
            Price::from("10.00"),
            Price::from("10.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));
        permanent_agg.handle_quote_tick(QuoteTick::new(
            leg2,
            Price::from("20.00"),
            Price::from("20.10"),
            Quantity::from(100),
            Quantity::from(100),
            ts,
            ts,
        ));

        let permanent_quotes = permanent_handler.lock();
        assert_eq!(permanent_quotes.len(), 1);
        assert_eq!(permanent_quotes[0].bid_price, Price::from("-10.10"));
        assert_eq!(permanent_quotes[0].ask_price, Price::from("-9.90"));
        assert!(!permanent_agg.vega_pricing_temporarily_disabled);
    }

    #[rstest]
    fn test_spread_quote_negative_prices_tick_scheme(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let leg1 = instrument.id();
        let leg2 = InstrumentId::from("MSFT.XNAS");
        let spread_id = InstrumentId::from("SPREAD.XNAS");
        let legs = vec![(leg1, 1_i64), (leg2, -1_i64)];
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let rounder = FixedTickSchemeRounder::new(0.01).unwrap();

        let mut agg = SpreadQuoteAggregator::new(
            spread_id,
            &legs,
            true,
            2,
            0,
            Box::new(record),
            clock,
            false,
            None,
            0,
            false,
            60,
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
        let quotes = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(1_000_000_000));
        let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock,
            record,
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

        let bars = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(1_500_000_000));
        let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock,
            record,
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

        let bars = handler.lock();
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
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(5_000_000_000));
        let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock,
            record,
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

        let bars = handler.lock();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].close, Price::from("103.00"));
    }

    #[rstest]
    fn test_time_bar_skip_first_non_full_bar_skips_when_build_delay_shifts_start(
        equity_aapl: Equity,
    ) {
        // When bar_build_delay > 0 pushes start_time past a boundary (even if `now` is on a
        // boundary), first_close_ns is set and the first bar is skipped. A `now > start_time`
        // guard would incorrectly keep this first bar.
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(2_000_000_000));
        let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock,
            record,
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

        let bars = handler.lock();
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
        // stored_open_ns must resolve to one step before start_time (close_time - step)
        // so the first bar's open timestamp marks the true start of the in-progress interval.
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, aggregation, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(start_ns));
        let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock,
            record,
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

        let bars = handler.lock();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].ts_event, UnixNanos::from(expected_stored_open_ns));
        assert_eq!(bars[0].ts_init, UnixNanos::from(start_ns));
    }

    #[rstest]
    fn test_time_bar_historical_prevents_bars_for_timer_before_last_data(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
        let (handler, record) = recording_handler();
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let mut agg = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            record,
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

        let bars = handler.lock();
        assert!(
            !bars.is_empty(),
            "advancing time from ts1 to ts2 should produce at least one bar"
        );
        assert_eq!(bars[0].close, Price::from("100.00"));
    }

    #[rstest]
    #[case(BarAggregation::Tick)]
    #[case(BarAggregation::TickImbalance)]
    #[case(BarAggregation::TickRuns)]
    #[case(BarAggregation::Volume)]
    #[case(BarAggregation::VolumeImbalance)]
    #[case(BarAggregation::VolumeRuns)]
    #[case(BarAggregation::Value)]
    #[case(BarAggregation::ValueImbalance)]
    #[case(BarAggregation::ValueRuns)]
    #[case(BarAggregation::Renko)]
    fn test_aggregators_standardize_composite_bar_type(
        equity_aapl: Equity,
        #[case] aggregation: BarAggregation,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new_composite(
            instrument.id(),
            BarSpecification::new(10, aggregation, PriceType::Last),
            AggregationSource::Internal,
            1,
            BarAggregation::Minute,
            AggregationSource::External,
        );
        let handler = |_: Bar| {};

        let aggregator: Box<dyn BarAggregator> = match aggregation {
            BarAggregation::Tick => Box::new(TickBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                handler,
            )),
            BarAggregation::TickImbalance => Box::new(TickImbalanceBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                handler,
            )),
            BarAggregation::TickRuns => Box::new(TickRunsBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                handler,
            )),
            BarAggregation::Volume => Box::new(VolumeBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                handler,
            )),
            BarAggregation::VolumeImbalance => Box::new(VolumeImbalanceBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                handler,
            )),
            BarAggregation::VolumeRuns => Box::new(VolumeRunsBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                handler,
            )),
            BarAggregation::Value => Box::new(ValueBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                handler,
            )),
            BarAggregation::ValueImbalance => Box::new(ValueImbalanceBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                handler,
            )),
            BarAggregation::ValueRuns => Box::new(ValueRunsBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                handler,
            )),
            BarAggregation::Renko => Box::new(RenkoBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                Price::from("0.01"),
                handler,
            )),
            _ => unreachable!(),
        };

        assert!(aggregator.bar_type().is_standard());
        assert_eq!(aggregator.bar_type(), bar_type.standard());
    }

    #[rstest]
    fn test_composite_tick_bar_aggregator_emits_standard_bar_type(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new_composite(
            instrument.id(),
            BarSpecification::new(1, BarAggregation::Tick, PriceType::Last),
            AggregationSource::Internal,
            1,
            BarAggregation::Minute,
            AggregationSource::External,
        );
        let (handler, record) = recording_handler();

        let mut aggregator = TickBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            record,
        );

        let input_bar = Bar::new(
            bar_type.composite(),
            Price::from("100.00"),
            Price::from("101.00"),
            Price::from("99.00"),
            Price::from("100.50"),
            Quantity::from(10),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
        );
        aggregator.handle_bar(input_bar);

        let handler_guard = handler.lock();
        assert_eq!(handler_guard.len(), 1);
        assert_eq!(handler_guard[0].bar_type, bar_type.standard());
    }

    #[rstest]
    fn test_composite_time_bar_aggregator_uses_standard_timer_name(equity_aapl: Equity) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        let bar_type = BarType::new_composite(
            instrument.id(),
            BarSpecification::new(5, BarAggregation::Minute, PriceType::Last),
            AggregationSource::Internal,
            1,
            BarAggregation::Minute,
            AggregationSource::External,
        );
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let aggregator = TimeBarAggregator::new(
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            clock.clone(),
            |_: Bar| {},
            false,
            true,
            BarIntervalType::LeftOpen,
            None,
            0,
            false,
        );

        let boxed: Box<dyn BarAggregator> = Box::new(aggregator);
        let rc = Rc::new(RefCell::new(boxed));
        rc.borrow_mut().start_timer(Some(Rc::clone(&rc)));

        let expected = format!("TIME_BAR_{}", bar_type.standard());
        assert!(
            clock.borrow().timer_names().contains(&expected.as_str()),
            "timer names {:?} should contain {expected}",
            clock.borrow().timer_names(),
        );
    }

    pub(super) fn recording_handler<T: 'static>() -> (Arc<Mutex<Vec<T>>>, impl FnMut(T)) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded_events = Arc::clone(&events);
        (events, move |event| recorded_events.lock().push(event))
    }
}

#[cfg(test)]
mod property_tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{clock::TestClock, timer::TimeEvent};
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::{Bar, BarSpecification, BarType, TradeTick, bar::get_bar_interval_ns},
        enums::{AggregationSource, AggressorSide, BarAggregation, BarIntervalType, PriceType},
        instruments::{Instrument, InstrumentAny, stubs::equity_aapl},
        types::{Price, Quantity},
    };
    use proptest::prelude::*;
    use rstest::rstest;
    use ustr::Ustr;

    use super::{tests::recording_handler, *};

    fn time_bar_spec_strategy() -> impl Strategy<Value = (BarAggregation, usize)> {
        prop_oneof![
            (Just(BarAggregation::Second), 1usize..=5),
            (Just(BarAggregation::Minute), 1usize..=5),
            (Just(BarAggregation::Hour), 1usize..=4),
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
            (aggregation, step) in time_bar_spec_strategy(),
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

            let (handler, record) = recording_handler();
            let clock = Rc::new(RefCell::new(TestClock::new()));
            clock.borrow_mut().set_time(UnixNanos::from(now_ns));
            let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

            let aggregator = TimeBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                clock,
                record,
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

            let bars = handler.lock();
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
            (aggregation, step) in time_bar_spec_strategy(),
            interval_type in interval_type_strategy(),
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(step, aggregation, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let interval_ns = get_bar_interval_ns(&bar_type).as_u64();

            // Clock exactly on a bar boundary: fire_immediately=true, so the first
            // bar that reaches build_and_send must emit regardless of skip_first.
            let now_ns = interval_ns;
            let (handler, record) = recording_handler();
            let clock = Rc::new(RefCell::new(TestClock::new()));
            clock.borrow_mut().set_time(UnixNanos::from(now_ns));
            let event_name = Ustr::from(&format!("TIME_BAR_{bar_type}"));

            let aggregator = TimeBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                clock,
                record,
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

            let bars = handler.lock();
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
            let (handler, record) = recording_handler();

            let mut aggregator = TickBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                record,
            );

            let mut total_input: u64 = 0;

            for (i, (price_cents, size)) in ticks.iter().enumerate() {
                let price = Price::new((*price_cents as f64) / 100.0, 2);
                let qty = Quantity::new(*size as f64, 0);
                aggregator.update(price, qty, UnixNanos::from((i as u64 + 1) * 1_000));
                total_input += *size;
            }

            let bars = handler.lock();
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
            let (handler, record) = recording_handler();

            let mut aggregator = VolumeBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                record,
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

            let bars = handler.lock();

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
        fn prop_volume_bar_matches_unit_trade_reference(
            updates in prop::collection::vec((1i64..=100_000i64, 1u64..=8u64, 0u64..=30u64), 1..=30),
            step in 1usize..=5,
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(step, BarAggregation::Volume, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let (handler, record) = recording_handler();
            let mut aggregator = VolumeBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                record,
            );
            let price = |cents| {
                Price::from_decimal_dp(Decimal::new(cents, 2), 2)
                    .expect("bounded cents must produce a valid price")
            };
            let mut last_timestamp = UnixNanos::default();
            let mut pending_units = Vec::new();
            let mut expected_bars = Vec::new();

            for (price_cents, size, timestamp) in &updates {
                let timestamp = UnixNanos::from(*timestamp);
                aggregator.update(price(*price_cents), Quantity::from(*size), timestamp);

                if timestamp < last_timestamp {
                    continue;
                }

                last_timestamp = timestamp;
                for _ in 0..*size {
                    pending_units.push((*price_cents, timestamp));
                }

                while pending_units.len() >= step {
                    let units: Vec<_> = pending_units.drain(..step).collect();
                    let first = units.first().unwrap();
                    let last = units.last().unwrap();
                    let low = units.iter().map(|(cents, _)| *cents).min().unwrap();
                    let high = units.iter().map(|(cents, _)| *cents).max().unwrap();
                    expected_bars.push((
                        price(first.0),
                        price(high),
                        price(low),
                        price(last.0),
                        Quantity::from(step as u64),
                        last.1,
                    ));
                }
            }

            let bars = handler.lock();
            prop_assert_eq!(bars.len(), expected_bars.len());
            for (actual, (open, high, low, close, volume, timestamp))
                in bars.iter().zip(expected_bars)
            {
                prop_assert_eq!(actual.open, open);
                prop_assert_eq!(actual.high, high);
                prop_assert_eq!(actual.low, low);
                prop_assert_eq!(actual.close, close);
                prop_assert_eq!(actual.volume, volume);
                prop_assert_eq!(actual.ts_event, timestamp);
                prop_assert_eq!(actual.ts_init, timestamp);
            }

            prop_assert_eq!(aggregator.core.builder.volume, Quantity::from(pending_units.len() as u64));
            prop_assert_eq!(aggregator.core.builder.ts_last, last_timestamp);

            if let Some((first, rest)) = pending_units.split_first() {
                let last = rest.last().unwrap_or(first);
                let low = pending_units.iter().map(|(cents, _)| *cents).min().unwrap();
                let high = pending_units.iter().map(|(cents, _)| *cents).max().unwrap();
                prop_assert_eq!(aggregator.core.builder.open, Some(price(first.0)));
                prop_assert_eq!(aggregator.core.builder.high, Some(price(high)));
                prop_assert_eq!(aggregator.core.builder.low, Some(price(low)));
                prop_assert_eq!(aggregator.core.builder.close, Some(price(last.0)));
            } else {
                prop_assert_eq!(aggregator.core.builder.open, None);
                prop_assert_eq!(aggregator.core.builder.high, None);
                prop_assert_eq!(aggregator.core.builder.low, None);
                prop_assert_eq!(aggregator.core.builder.close, None);
            }
        }

        #[rstest]
        fn prop_bar_builder_spread_adjustment_is_additive(
            updates in prop::collection::vec((10_000i64..=100_000i64, 1u64..=100u64), 1..=20),
            spread_cents in -10_000i64..=10_000i64,
            backward in any::<bool>(),
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(1, BarAggregation::Tick, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let mut builder = BarBuilder::new(bar_type, 2, 0);

            let spread = Decimal::new(spread_cents, 2);
            let mode = if backward {
                ContinuousFutureAdjustmentType::BackwardSpread
            } else {
                ContinuousFutureAdjustmentType::ForwardSpread
            };
            builder.set_adjustment(spread, mode);

            let mut min_cents = i64::MAX;
            let mut max_cents = i64::MIN;

            for (i, (price_cents, size)) in updates.iter().enumerate() {
                if *price_cents < min_cents {
                    min_cents = *price_cents;
                }

                if *price_cents > max_cents {
                    max_cents = *price_cents;
                }

                builder.update(
                    Price::new((*price_cents as f64) / 100.0, 2),
                    Quantity::new(*size as f64, 0),
                    UnixNanos::from((i as u64 + 1) * 1_000),
                );
            }

            let bar = builder.build_now();
            let first_decimal = Decimal::new(updates.first().unwrap().0, 2);
            let last_decimal = Decimal::new(updates.last().unwrap().0, 2);
            let min_decimal = Decimal::new(min_cents, 2);
            let max_decimal = Decimal::new(max_cents, 2);

            prop_assert_eq!(bar.open.as_decimal(), first_decimal + spread);
            prop_assert_eq!(bar.close.as_decimal(), last_decimal + spread);
            prop_assert_eq!(bar.low.as_decimal(), min_decimal + spread);
            prop_assert_eq!(bar.high.as_decimal(), max_decimal + spread);
        }

        #[rstest]
        fn prop_bar_builder_inactive_adjustment_is_identity(
            updates in prop::collection::vec((1i64..=100_000i64, 1u64..=1_000u64), 1..=20),
            use_ratio in any::<bool>(),
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(1, BarAggregation::Tick, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);

            let mut adjusted = BarBuilder::new(bar_type, 2, 0);
            let mut baseline = BarBuilder::new(bar_type, 2, 0);

            // Inactive in either mode: ZERO spread or ONE ratio.
            let (input, mode) = if use_ratio {
                (Decimal::ONE, ContinuousFutureAdjustmentType::BackwardRatio)
            } else {
                (Decimal::ZERO, ContinuousFutureAdjustmentType::BackwardSpread)
            };
            adjusted.set_adjustment(input, mode);

            for (i, (price_cents, size)) in updates.iter().enumerate() {
                let price = Price::new((*price_cents as f64) / 100.0, 2);
                let qty = Quantity::new(*size as f64, 0);
                let ts = UnixNanos::from((i as u64 + 1) * 1_000);
                adjusted.update(price, qty, ts);
                baseline.update(price, qty, ts);
            }

            let bar_adjusted = adjusted.build_now();
            let bar_baseline = baseline.build_now();
            prop_assert_eq!(bar_adjusted.open, bar_baseline.open);
            prop_assert_eq!(bar_adjusted.high, bar_baseline.high);
            prop_assert_eq!(bar_adjusted.low, bar_baseline.low);
            prop_assert_eq!(bar_adjusted.close, bar_baseline.close);
            prop_assert_eq!(bar_adjusted.volume, bar_baseline.volume);
        }

        #[rstest]
        fn prop_bar_builder_spread_preserves_raw_arithmetic(
            updates in prop::collection::vec((10_000i64..=100_000i64, 1u64..=100u64), 1..=20),
            // Sub-precision spread: scale 4 versus price precision 2. Locks in that
            // spread mode performs raw addition without rounding to price precision.
            spread_micro in -10_000i64..=10_000i64,
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(1, BarAggregation::Tick, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let mut builder = BarBuilder::new(bar_type, 2, 0);

            let spread = Decimal::new(spread_micro, 4);
            builder.set_adjustment(spread, ContinuousFutureAdjustmentType::BackwardSpread);

            let adjustment_raw_i128 = mantissa_exponent_to_fixed_i128(
                spread.mantissa(),
                -(spread.scale() as i8),
                FIXED_PRECISION,
            )
            .expect("scale within range");
            #[allow(
                clippy::useless_conversion,
                reason = "i128 to PriceRaw is real when not high-precision"
            )]
            let expected_adjustment_raw: PriceRaw =
                adjustment_raw_i128.try_into().expect("within PriceRaw range");

            let mut min_cents = i64::MAX;
            let mut max_cents = i64::MIN;
            let mut last_price = Price::new(0.0, 2);
            let mut first_price = Price::new(0.0, 2);

            for (i, (price_cents, size)) in updates.iter().enumerate() {
                if *price_cents < min_cents {
                    min_cents = *price_cents;
                }

                if *price_cents > max_cents {
                    max_cents = *price_cents;
                }

                let price = Price::new((*price_cents as f64) / 100.0, 2);

                if i == 0 {
                    first_price = price;
                }

                last_price = price;
                builder.update(
                    price,
                    Quantity::new(*size as f64, 0),
                    UnixNanos::from((i as u64 + 1) * 1_000),
                );
            }

            let bar = builder.build_now();
            let min_price = Price::new((min_cents as f64) / 100.0, 2);
            let max_price = Price::new((max_cents as f64) / 100.0, 2);
            prop_assert_eq!(bar.open.raw, first_price.raw + expected_adjustment_raw);
            prop_assert_eq!(bar.close.raw, last_price.raw + expected_adjustment_raw);
            prop_assert_eq!(bar.low.raw, min_price.raw + expected_adjustment_raw);
            prop_assert_eq!(bar.high.raw, max_price.raw + expected_adjustment_raw);
            prop_assert_eq!(bar.open.precision, 2);
            prop_assert_eq!(bar.high.precision, 2);
            prop_assert_eq!(bar.low.precision, 2);
            prop_assert_eq!(bar.close.precision, 2);
        }

        #[rstest]
        fn prop_bar_builder_active_ratio_scales_each_ohlc(
            updates in prop::collection::vec((1_000i64..=100_000i64, 1u64..=100u64), 1..=20),
            // Ratio in [0.50, 2.00] excluding exactly 1.00 to stay on the active path.
            ratio_centi in prop_oneof![50i64..=99i64, 101i64..=200i64],
            backward in any::<bool>(),
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(1, BarAggregation::Tick, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let mut builder = BarBuilder::new(bar_type, 2, 0);

            let ratio_decimal = Decimal::new(ratio_centi, 2);
            let ratio_f64 = (ratio_centi as f64) / 100.0;
            let mode = if backward {
                ContinuousFutureAdjustmentType::BackwardRatio
            } else {
                ContinuousFutureAdjustmentType::ForwardRatio
            };
            builder.set_adjustment(ratio_decimal, mode);

            let mut min_cents = i64::MAX;
            let mut max_cents = i64::MIN;
            let mut first_cents = 0i64;
            let mut last_cents = 0i64;

            for (i, (price_cents, size)) in updates.iter().enumerate() {
                if *price_cents < min_cents {
                    min_cents = *price_cents;
                }

                if *price_cents > max_cents {
                    max_cents = *price_cents;
                }

                if i == 0 {
                    first_cents = *price_cents;
                }

                last_cents = *price_cents;
                builder.update(
                    Price::new((*price_cents as f64) / 100.0, 2),
                    Quantity::new(*size as f64, 0),
                    UnixNanos::from((i as u64 + 1) * 1_000),
                );
            }

            let bar = builder.build_now();
            // Recompute via the same float math as the hot path so equality is exact.
            let expect = |cents: i64| Price::new((cents as f64) / 100.0 * ratio_f64, 2);
            prop_assert_eq!(bar.open, expect(first_cents));
            prop_assert_eq!(bar.close, expect(last_cents));
            // Ratio with positive ratio_f64 preserves ordering, so min/max map directly.
            prop_assert_eq!(bar.low, expect(min_cents));
            prop_assert_eq!(bar.high, expect(max_cents));
        }

        #[rstest]
        fn prop_bar_builder_spread_mode_direction_is_metadata_only(
            updates in prop::collection::vec((10_000i64..=100_000i64, 1u64..=100u64), 1..=20),
            spread_cents in -10_000i64..=10_000i64,
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(1, BarAggregation::Tick, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);

            let spread = Decimal::new(spread_cents, 2);
            let mut backward = BarBuilder::new(bar_type, 2, 0);
            let mut forward = BarBuilder::new(bar_type, 2, 0);
            backward.set_adjustment(spread, ContinuousFutureAdjustmentType::BackwardSpread);
            forward.set_adjustment(spread, ContinuousFutureAdjustmentType::ForwardSpread);

            for (i, (price_cents, size)) in updates.iter().enumerate() {
                let price = Price::new((*price_cents as f64) / 100.0, 2);
                let qty = Quantity::new(*size as f64, 0);
                let ts = UnixNanos::from((i as u64 + 1) * 1_000);
                backward.update(price, qty, ts);
                forward.update(price, qty, ts);
            }

            let bar_backward = backward.build_now();
            let bar_forward = forward.build_now();
            prop_assert_eq!(bar_backward.open, bar_forward.open);
            prop_assert_eq!(bar_backward.high, bar_forward.high);
            prop_assert_eq!(bar_backward.low, bar_forward.low);
            prop_assert_eq!(bar_backward.close, bar_forward.close);
        }

        #[rstest]
        fn prop_value_bar_aggregator_ohlc_invariants(
            ticks in prop::collection::vec((50i64..=500i64, 1u64..=20u64), 2..=30),
            step in 100u64..=2_000u64,
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(step as usize, BarAggregation::Value, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let (handler, record) = recording_handler();

            let mut aggregator = ValueBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                record,
            );

            for (i, (price_cents, size)) in ticks.iter().enumerate() {
                aggregator.update(
                    Price::new((*price_cents as f64) / 100.0, 2),
                    Quantity::new(*size as f64, 0),
                    UnixNanos::from((i as u64 + 1) * 1_000),
                );
            }

            let bars = handler.lock();
            for bar in bars.iter() {
                prop_assert!(bar.low <= bar.open);
                prop_assert!(bar.low <= bar.close);
                prop_assert!(bar.high >= bar.open);
                prop_assert!(bar.high >= bar.close);
                prop_assert!(bar.volume.as_f64() > 0.0);
            }
        }

        #[rstest]
        fn prop_renko_brick_chain(
            moves in prop::collection::vec(-500i64..=500i64, 1..=60),
            step in 1usize..=10,
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(step, BarAggregation::Renko, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let (handler, record) = recording_handler();

            let price_increment = Price::from("0.01");
            let mut aggregator = RenkoBarAggregator::new(
                bar_type,
                2,
                0,
                price_increment,
                record,
            );
            let brick_size = aggregator.brick_size;

            let base_raw = Price::from("1000.00").raw;
            let mut cum_increments: i64 = 0;
            let mut first_price: Option<Price> = None;

            for (i, delta) in moves.iter().enumerate() {
                cum_increments += delta;
                let price = Price::from_raw(
                    base_raw + PriceRaw::from(cum_increments) * price_increment.raw,
                    2,
                );

                if first_price.is_none() {
                    first_price = Some(price);
                }

                aggregator.update(price, Quantity::from(1), UnixNanos::from((i as u64 + 1) * 1_000));
            }

            let bars = handler.lock();
            let mut expected_open = first_price.unwrap();

            for bar in bars.iter() {
                // Bricks chain: each opens at the previous close.
                prop_assert_eq!(bar.open, expected_open);
                // Every brick spans exactly one brick size.
                prop_assert_eq!((bar.close.raw - bar.open.raw).abs(), brick_size);
                // High/low are the brick endpoints.
                prop_assert_eq!(bar.high, bar.open.max(bar.close));
                prop_assert_eq!(bar.low, bar.open.min(bar.close));
                expected_open = bar.close;
            }
        }

        #[rstest]
        fn prop_volume_imbalance_one_sided_conservation(
            sizes in prop::collection::vec(1u64..=50u64, 1..=40),
            step in 2u64..=10u64,
            buyer in any::<bool>(),
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(
                step as usize,
                BarAggregation::VolumeImbalance,
                PriceType::Last,
            );
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let (handler, record) = recording_handler();

            let mut aggregator = VolumeImbalanceBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                record,
            );

            let side = if buyer { AggressorSide::Buy } else { AggressorSide::Sell };
            let mut total_input: u64 = 0;

            for (i, size) in sizes.iter().enumerate() {
                let trade = TradeTick {
                    instrument_id: instrument.id(),
                    price: Price::from("100.00"),
                    size: Quantity::from(*size),
                    aggressor_side: side,
                    ts_event: UnixNanos::from((i as u64 + 1) * 1_000),
                    ts_init: UnixNanos::from((i as u64 + 1) * 1_000),
                    ..TradeTick::default()
                };
                aggregator.handle_trade(trade);
                total_input += *size;
            }

            let bars = handler.lock();

            // One-sided flow: every emitted bar carries exactly `step` volume.
            for bar in bars.iter() {
                prop_assert_eq!(bar.volume, Quantity::from(step));
            }

            // Conservation: emitted volume plus pending builder volume equals input.
            let emitted: u64 = bars.len() as u64 * step;
            let pending = aggregator.core.builder.volume.as_f64();
            prop_assert!((emitted as f64 + pending - total_input as f64).abs() < 1e-9);
        }

        #[rstest]
        fn prop_volume_runs_one_sided_conservation(
            sizes in prop::collection::vec(1u64..=50u64, 1..=40),
            step in 2u64..=10u64,
            buyer in any::<bool>(),
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(
                step as usize,
                BarAggregation::VolumeRuns,
                PriceType::Last,
            );
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let (handler, record) = recording_handler();

            let mut aggregator = VolumeRunsBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                record,
            );

            let side = if buyer { AggressorSide::Buy } else { AggressorSide::Sell };
            let mut total_input: u64 = 0;

            for (i, size) in sizes.iter().enumerate() {
                let trade = TradeTick {
                    instrument_id: instrument.id(),
                    price: Price::from("100.00"),
                    size: Quantity::from(*size),
                    aggressor_side: side,
                    ts_event: UnixNanos::from((i as u64 + 1) * 1_000),
                    ts_init: UnixNanos::from((i as u64 + 1) * 1_000),
                    ..TradeTick::default()
                };
                aggregator.handle_trade(trade);
                total_input += *size;
            }

            let bars = handler.lock();

            // A single-sided run never resets, so every bar carries exactly `step` volume.
            for bar in bars.iter() {
                prop_assert_eq!(bar.volume, Quantity::from(step));
            }

            let emitted: u64 = bars.len() as u64 * step;
            let pending = aggregator.core.builder.volume.as_f64();
            prop_assert!((emitted as f64 + pending - total_input as f64).abs() < 1e-9);
        }

        #[rstest]
        fn prop_value_bar_cum_value_stays_below_step(
            ticks in prop::collection::vec((50i64..=500i64, 1u64..=20u64), 1..=30),
            step in 100u64..=2_000u64,
        ) {
            let instrument = InstrumentAny::Equity(equity_aapl());
            let bar_spec = BarSpecification::new(step as usize, BarAggregation::Value, PriceType::Last);
            let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::Internal);
            let step_decimal = Decimal::from(step);

            let mut aggregator = ValueBarAggregator::new(
                bar_type,
                instrument.price_precision(),
                instrument.size_precision(),
                |_: Bar| {},
            );

            for (i, (price_cents, size)) in ticks.iter().enumerate() {
                aggregator.update(
                    Price::new((*price_cents as f64) / 100.0, 2),
                    Quantity::new(*size as f64, 0),
                    UnixNanos::from((i as u64 + 1) * 1_000),
                );

                // Invariant: the accumulator is always strictly below the step threshold,
                // which also guarantees the loop division never sees a zero divisor.
                prop_assert!(aggregator.get_cumulative_value() < step_decimal);
            }
        }
    }
}
