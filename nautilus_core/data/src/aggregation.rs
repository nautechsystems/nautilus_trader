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

use std::{ops::Add, sync::Arc};

use nautilus_core::{correctness, nanos::UnixNanos};
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        quote::QuoteTick,
        trade::TradeTick,
    },
    instruments::any::InstrumentAny,
    types::{fixed::FIXED_SCALAR, price::Price, quantity::Quantity},
};

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
    #[must_use]
    pub fn new(instrument: &InstrumentAny, bar_type: BarType) -> Self {
        correctness::check_equal(
            instrument.id(),
            bar_type.instrument_id,
            "instrument.id",
            "bar_type.instrument_id",
        )
        .unwrap();

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

    pub fn reset(&mut self) {
        self.open = None;
        self.high = None;
        self.low = None;
        self.volume = Quantity::zero(self.size_precision);
        self.count = 0;
    }

    pub fn build_now(&mut self) -> Bar {
        self.build(self.ts_last, self.ts_last)
    }

    pub fn build(&mut self, ts_event: UnixNanos, ts_init: UnixNanos) -> Bar {
        if self.open.is_none() {
            self.open = self.last_close;
            self.high = self.last_close;
            self.low = self.last_close;
            self.close = self.last_close;
        }

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

pub struct BarAggregator {
    builder: BarBuilder,
    handler: Arc<dyn Fn(Bar) + Send + Sync>,
    await_partial: bool,
    bar_type: BarType,
}

impl BarAggregator {
    pub fn new(
        instrument: &InstrumentAny,
        bar_type: BarType,
        handler: Arc<dyn Fn(Bar) + Send + Sync>,
        await_partial: bool,
    ) -> Self {
        correctness::check_equal(
            instrument.id(),
            bar_type.instrument_id,
            "instrument.id",
            "bar_type.instrument_id",
        )
        .unwrap();

        Self {
            builder: BarBuilder::new(instrument, bar_type),
            handler,
            await_partial,
            bar_type,
        }
    }

    pub fn set_await_partial(&mut self, value: bool) {
        self.await_partial = value;
    }

    pub fn handle_quote_tick(&mut self, tick: QuoteTick) {
        if !self.await_partial {
            self.apply_update(
                tick.extract_price(self.bar_type.spec.price_type),
                tick.extract_volume(self.bar_type.spec.price_type),
                tick.ts_event,
            );
        }
    }

    pub fn handle_trade_tick(&mut self, tick: TradeTick) {
        if !self.await_partial {
            self.apply_update(tick.price, tick.size, tick.ts_event);
        }
    }

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

pub struct TickBarAggregator {
    aggregator: BarAggregator,
}

impl TickBarAggregator {
    pub fn new(
        instrument: &InstrumentAny,
        bar_type: BarType,
        handler: Arc<dyn Fn(Bar) + Send + Sync>,
    ) -> Self {
        Self {
            aggregator: BarAggregator::new(instrument, bar_type, handler, false),
        }
    }

    fn apply_update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        self.aggregator.apply_update(price, size, ts_event);

        if self.aggregator.builder.count == self.aggregator.bar_type.spec.step {
            self.aggregator.build_now_and_send();
        }
    }
}

pub struct VolumeBarAggregator {
    aggregator: BarAggregator,
}

impl VolumeBarAggregator {
    pub fn new(
        instrument: &InstrumentAny,
        bar_type: BarType,
        handler: Arc<dyn Fn(Bar) + Send + Sync>,
    ) -> Self {
        Self {
            aggregator: BarAggregator::new(instrument, bar_type, handler, false),
        }
    }

    #[allow(unused_assignments)] // Temp for development
    fn apply_update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        let mut raw_size_update = size.raw;
        let raw_step = (self.aggregator.bar_type.spec.step as f64 * FIXED_SCALAR) as u64;
        let mut raw_size_diff = 0;

        while raw_size_update > 0 {
            if self.aggregator.builder.volume.raw + raw_size_update < raw_step {
                self.aggregator.apply_update(
                    price,
                    Quantity::from_raw(raw_size_update, size.precision).unwrap(),
                    ts_event,
                );
                break;
            }

            raw_size_diff = raw_step - self.aggregator.builder.volume.raw;
            self.aggregator.apply_update(
                price,
                Quantity::from_raw(raw_size_update, size.precision).unwrap(),
                ts_event,
            );

            self.aggregator.build_now_and_send();
            raw_size_update -= raw_size_diff;
        }
    }
}

pub struct ValueBarAggregator {
    aggregator: BarAggregator,
    cum_value: f64,
}

impl ValueBarAggregator {
    pub fn new(
        instrument: &InstrumentAny,
        bar_type: BarType,
        handler: Arc<dyn Fn(Bar) + Send + Sync>,
    ) -> Self {
        Self {
            aggregator: BarAggregator::new(instrument, bar_type, handler, false),
            cum_value: 0.0,
        }
    }

    #[must_use]
    pub const fn get_cumulative_value(&self) -> f64 {
        self.cum_value
    }

    fn apply_update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
        let mut size_update = size.as_f64();

        while size_update > 0.0 {
            let value_update = price.as_f64() * size_update;
            if self.cum_value + value_update < self.aggregator.bar_type.spec.step as f64 {
                self.cum_value += value_update;
                self.aggregator.apply_update(
                    price,
                    Quantity::new(size_update, size.precision).unwrap(),
                    ts_event,
                );
                break;
            }

            let value_diff = self.aggregator.bar_type.spec.step as f64 - self.cum_value;
            let size_diff = size_update * (value_diff / value_update);
            self.aggregator.apply_update(
                price,
                Quantity::new(size_diff, size.precision).unwrap(),
                ts_event,
            );

            self.aggregator.build_now_and_send();
            self.cum_value = 0.0;
            size_update -= size_diff;
        }
    }
}

// pub struct TimeBarAggregator {
//     aggregator: BarAggregator,
//     clock: Arc<dyn Clock>,
//     build_on_next_tick: bool,
//     stored_open_ns: u64,
//     stored_close_ns: u64,
//     cached_update: Option<(Price, Quantity, u64)>,
//     timer_name: String,
//     build_with_no_updates: bool,
//     timestamp_on_close: bool,
//     is_left_open: bool,
//     interval: Duration,
//     interval_ns: u64,
//     next_close_ns: u64,
// }
//
// impl TimeBarAggregator {
//     pub fn new(
//         instrument: &InstrumentAny,
//         bar_type: BarType,
//         handler: Arc<dyn Fn(Bar) + Send + Sync>,
//         clock: Arc<dyn Clock>,
//         build_with_no_updates: bool,
//         timestamp_on_close: bool,
//         interval_type: &str,
//     ) -> Self {
//         let mut aggregator = BarAggregator::new(instrument, bar_type.clone(), handler, false);
//         let interval = Self::get_interval(&bar_type);
//         let interval_ns = Self::get_interval_ns(&bar_type);
//
//         let mut instance = Self {
//             aggregator,
//             clock,
//             build_on_next_tick: false,
//             stored_open_ns: UnixNanos::from(Self::get_start_time(&clock, &bar_type)),
//             stored_close_ns: 0,
//             cached_update: None,
//             timer_name: bar_type.to_string(),
//             build_with_no_updates,
//             timestamp_on_close,
//             is_left_open: interval_type == "left-open",
//             interval,
//             interval_ns,
//             next_close_ns: 0,
//         };
//
//         instance.set_build_timer();
//         instance.next_close_ns = instance.clock.next_time_ns(&instance.timer_name);
//         instance
//     }
//
//     fn get_start_time(clock: &Arc<dyn Clock>, bar_type: &BarType) -> DateTime<Utc> {
//         let now = clock.utc_now();
//         let step = bar_type.spec.step;
//
//         match bar_type.spec.aggregation {
//             BarAggregation::Millisecond => {
//                 let diff_microseconds = now.timestamp_subsec_micros() % (step * 1000);
//                 let diff_seconds = if diff_microseconds == 0 {
//                     0
//                 } else {
//                     (step * 1000) as i64 - 1
//                 };
//                 now - Duration::from_seconds(diff_seconds)
//                     - Duration::from_micros(now.timestamp_subsec_micros() as i64)
//             }
//             BarAggregation::Second => {
//                 let diff_seconds = now.timestamp() % step as i64;
//                 let diff_minutes = if diff_seconds == 0 {
//                     0
//                 } else {
//                     (step / 60) as i64 - 1
//                 };
//                 now - Duration::from_mins(diff_minutes) - Duration::from_secs(diff_seconds)
//             }
//             BarAggregation::Minute => {
//                 let diff_minutes = now.minute() % step;
//                 let diff_hours = if diff_minutes == 0 {
//                     0
//                 } else {
//                     (step / 60) - 1
//                 };
//                 now - Duration::hours(diff_hours as i64)
//                     - Duration::minutes(diff_minutes as i64)
//                     - Duration::seconds(now.second() as i64)
//             }
//             BarAggregation::Hour => {
//                 let diff_hours = now.hour() % step;
//                 let diff_days = if diff_hours == 0 { 0 } else { (step / 24) - 1 };
//                 now - Duration::from_days(diff_days as i64)
//                     - Duration::hours(diff_hours as i64)
//                     - Duration::minutes(now.minute() as i64)
//                     - Duration::seconds(now.second() as i64)
//             }
//             BarAggregation::Day => {
//                 now - Duration::days(now.day() as i64 % step as i64)
//                     - Duration::hours(now.hour() as i64)
//                     - Duration::minutes(now.minute() as i64)
//                     - Duration::seconds(now.second() as i64)
//             }
//             _ => panic!("Aggregation type not supported for time bars"),
//         }
//     }
//
//     fn get_interval(bar_type: &BarType) -> Duration {
//         match bar_type.spec.aggregation {
//             BarAggregation::Millisecond => Duration::from_millis(bar_type.spec.step as u64),
//             BarAggregation::Second => Duration::from_secs(bar_type.spec.step as u64),
//             BarAggregation::Minute => Duration::from_secs((bar_type.spec.step * 60) as u64),
//             BarAggregation::Hour => Duration::from_secs((bar_type.spec.step * 60 * 60) as u64),
//             BarAggregation::Day => Duration::from_secs((bar_type.spec.step * 60 * 60 * 24) as u64),
//             _ => panic!("Aggregation not time based"),
//         }
//     }
//
//     fn get_interval_ns(bar_type: &BarType) -> u64 {
//         match bar_type.spec.aggregation {
//             BarAggregation::Millisecond => millis_to_nanos(bar_type.spec.step),
//             BarAggregation::Second => secs_to_nanos(bar_type.spec.step),
//             BarAggregation::Minute => secs_to_nanos(bar_type.spec.step) * 60,
//             BarAggregation::Hour => secs_to_nanos(bar_type.spec.step) * 60 * 60,
//             BarAggregation::Day => secs_to_nanos(bar_type.spec.step) * 60 * 60 * 24,
//             _ => panic!("Aggregation not time based"),
//         }
//     }
//
//     fn set_build_timer(&mut self) {
//         self.clock.set_timer_ns(
//             &self.timer_name,
//             self.interval,
//             Self::get_start_time(&self.clock, &self.aggregator.bar_type),
//             None,
//             Box::new(move |event| self.build_bar(event)),
//         );
//
//         log::debug!("Started timer {}", self.timer_name);
//     }
//
//     fn apply_update(&mut self, price: Price, size: Quantity, ts_event: UnixNanos) {
//         self.aggregator.apply_update(price, size, ts_event);
//         if self.build_on_next_tick {
//             let ts_init = ts_event;
//
//             let ts_event = if self.is_left_open {
//                 if self.timestamp_on_close {
//                     self.stored_close_ns
//                 } else {
//                     self.stored_open_ns
//                 }
//             } else {
//                 self.stored_open_ns
//             };
//
//             self.aggregator.build_and_send(ts_event, ts_init);
//             self.build_on_next_tick = false;
//             self.stored_close_ns = 0;
//         }
//     }
//
//     fn build_bar(&mut self, event: TimeEvent) {
//         if !self.aggregator.builder.initialized {
//             self.build_on_next_tick = true;
//             self.stored_close_ns = self.next_close_ns;
//             return;
//         }
//
//         if !self.build_with_no_updates && self.aggregator.builder.count == 0 {
//             return;
//         }
//
//         let ts_init = event.ts_event;
//         let ts_event = if self.is_left_open {
//             if self.timestamp_on_close {
//                 event.ts_event
//             } else {
//                 self.stored_open_ns
//             }
//         } else {
//             self.stored_open_ns
//         };
//
//         self.aggregator.build_and_send(ts_event, ts_init);
//         self.stored_open_ns = event.ts_event;
//         self.next_close_ns = self.clock.next_time_ns(&self.timer_name);
//     }
//
//     pub fn stop(&self) {
//         self.clock.cancel_timer(&self.timer_name);
//     }
// }
