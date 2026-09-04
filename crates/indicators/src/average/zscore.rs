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

//! Rolling z-score over a fixed window.

use std::fmt::Display;

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_core::correctness::{FAILED, check_predicate_true};
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::indicator::Indicator;

const MAX_PERIOD: usize = 1_024;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.indicators")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.indicators")
)]
pub struct ZScore {
    pub period: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub mean: f64,
    pub std: f64,
    pub count: usize,
    inputs: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    pub initialized: bool,
}

impl Display for ZScore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for ZScore {
    fn name(&self) -> String {
        stringify!(ZScore).into()
    }

    fn has_inputs(&self) -> bool {
        self.count > 0
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.process_raw(quote.extract_price(self.price_type)?.into());
        Ok(())
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.process_raw(trade.price.into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.process_raw(bar.close.into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.mean = 0.0;
        self.std = 0.0;
        self.count = 0;
        self.inputs.clear();
        self.initialized = false;
    }
}

impl ZScore {
    /// Creates a new [`ZScore`] instance.
    ///
    /// The z-score is `(x - mean) / std` over the current window, using sample
    /// standard deviation (`n - 1`). Until `period` observations have arrived the
    /// window is expanding (`n = count`); afterwards it slides at length `period`.
    /// `price_type` is used only by `handle_quote`; `update_raw` accepts any `f64`
    /// series. When the current window is constant or `std` is 0, `value` is 0.
    ///
    /// # Panics
    ///
    /// Panics if `period` is less than 2 or greater than `MAX_PERIOD`.
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self::new_checked(period, price_type).expect(FAILED)
    }

    /// Creates a new [`ZScore`] instance with the given period.
    ///
    /// # Errors
    ///
    /// Returns an error if `period` is less than 2 or greater than `MAX_PERIOD`.
    pub fn new_checked(period: usize, price_type: Option<PriceType>) -> anyhow::Result<Self> {
        check_predicate_true(period >= 2, "`period` must be at least 2")?;
        check_predicate_true(period <= MAX_PERIOD, "`period` exceeds MAX_PERIOD")?;

        Ok(Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            mean: 0.0,
            std: 0.0,
            count: 0,
            inputs: ArrayDeque::new(),
            initialized: false,
        })
    }

    /// Updates the indicator with a raw observation.
    pub fn update_raw(&mut self, value: f64) {
        self.process_raw(value);
    }

    fn process_raw(&mut self, value: f64) {
        if self.inputs.len() == self.period {
            let _ = self.inputs.pop_front();
        } else {
            self.count += 1;
        }

        let _ = self.inputs.push_back(value);

        let n = self.count as f64;
        self.mean = self.inputs.iter().sum::<f64>() / n;
        self.initialized = self.count >= self.period;

        if self.count < 2 {
            self.std = 0.0;
            self.value = 0.0;
            return;
        }

        let mean = self.mean;
        let (m2, is_constant) = self
            .inputs
            .iter()
            .fold((0.0, true), |(m2, is_constant), &x| {
                let d = x - mean;
                (
                    m2 + d * d,
                    is_constant && x.is_finite() && x.to_bits() == value.to_bits(),
                )
            });
        self.std = (m2 / (n - 1.0)).sqrt();
        self.value = if is_constant || self.std == 0.0 {
            0.0
        } else {
            (value - self.mean) / self.std
        };
    }
}

#[cfg(test)]
mod tests {
    use arraydeque::{ArrayDeque, Wrapping};
    use nautilus_model::{
        data::{Bar, QuoteTick, TradeTick},
        enums::PriceType,
    };
    use proptest::prelude::*;
    use rstest::rstest;

    use super::{MAX_PERIOD, ZScore};
    use crate::{
        indicator::Indicator,
        stubs::*,
        testing::{approx_equal_with, assert_approx_equal},
    };

    /// Batch z-score of `window` using sample std (`n - 1`).
    fn batch_zscore(window: &[f64]) -> (f64, f64, f64) {
        let n = window.len() as f64;
        let mean = window.iter().sum::<f64>() / n;
        let m2: f64 = window
            .iter()
            .map(|x| {
                let d = x - mean;
                d * d
            })
            .sum();
        let std = (m2 / (n - 1.0)).sqrt();
        let x = *window.last().unwrap();
        let is_constant = window
            .iter()
            .all(|&value| value.is_finite() && value.to_bits() == x.to_bits());
        let z = if is_constant || std == 0.0 {
            0.0
        } else {
            (x - mean) / std
        };
        (mean, std, z)
    }

    #[rstest]
    fn zscore_initialized_state(indicator_zscore_10: ZScore) {
        assert_eq!(format!("{indicator_zscore_10}"), "ZScore(10)");
        assert_eq!(indicator_zscore_10.period, 10);
        assert_eq!(indicator_zscore_10.price_type, PriceType::Mid);
        assert_eq!(indicator_zscore_10.value, 0.0);
        assert_eq!(indicator_zscore_10.mean, 0.0);
        assert_eq!(indicator_zscore_10.std, 0.0);
        assert_eq!(indicator_zscore_10.count, 0);
        assert!(!indicator_zscore_10.initialized());
        assert!(!indicator_zscore_10.has_inputs());
    }

    #[rstest]
    fn zscore_default_price_type_is_last() {
        let z = ZScore::new(5, None);
        assert_eq!(z.price_type, PriceType::Last);
    }

    #[rstest]
    fn zscore_initializes_at_period() {
        let mut z = ZScore::new(5, None);
        for i in 1..5 {
            z.update_raw(f64::from(i));
            assert!(!z.initialized());
        }
        z.update_raw(5.0);
        assert!(z.initialized());
        assert_eq!(z.count, 5);
        assert!(z.has_inputs());
    }

    #[rstest]
    fn zscore_constant_series_is_zero() {
        let mut z = ZScore::new(4, None);
        for _ in 0..8 {
            z.update_raw(3.0);
        }
        assert_eq!(z.std, 0.0);
        assert_eq!(z.value, 0.0);
        assert_eq!(z.mean, 3.0);
    }

    #[rstest]
    #[case(1.000_03, 10)]
    #[case(0.1, 20)]
    fn zscore_constant_series_with_rounding_error_is_zero(
        #[case] value: f64,
        #[case] period: usize,
    ) {
        let mut z = ZScore::new(period, None);
        for _ in 0..period {
            z.update_raw(value);
        }

        assert_eq!(z.value, 0.0);
    }

    #[rstest]
    fn zscore_preserves_non_finite_value() {
        let mut z = ZScore::new(2, None);
        z.update_raw(1.0);
        z.update_raw(f64::NAN);

        assert!(z.std.is_nan());
        assert!(z.value.is_nan());
    }

    #[rstest]
    fn zscore_propagates_non_finite_arithmetic() {
        let mut z = ZScore::new(2, None);
        z.update_raw(f64::MAX);
        z.update_raw(f64::MAX / 2.0);

        assert!(z.std.is_infinite());
        assert!(z.value.is_nan());
    }

    #[rstest]
    fn zscore_expanding_window_before_period() {
        let mut z = ZScore::new(5, None);

        z.update_raw(2.0);
        assert!(!z.initialized());
        assert_eq!(z.count, 1);
        assert_eq!(z.mean, 2.0);
        assert_eq!(z.std, 0.0);
        assert_eq!(z.value, 0.0);

        z.update_raw(4.0);
        assert!(!z.initialized());
        assert_eq!(z.count, 2);
        assert_eq!(z.mean, 3.0);
        assert_approx_equal(z.std, 2.0_f64.sqrt());
        assert_approx_equal(z.value, 1.0 / 2.0_f64.sqrt());
    }

    #[rstest]
    fn zscore_transitions_from_expanding_to_rolling() {
        let mut z = ZScore::new(3, None);
        z.update_raw(2.0);
        z.update_raw(4.0);
        z.update_raw(6.0);

        assert!(z.initialized());
        assert_eq!(z.count, 3);
        assert_eq!(z.mean, 4.0);
        assert_eq!(z.std, 2.0);
        assert_eq!(z.value, 1.0);

        z.update_raw(8.0);
        assert_eq!(z.count, 3);
        assert_eq!(z.mean, 6.0);
        assert_eq!(z.std, 2.0);
        assert_eq!(z.value, 1.0);
    }

    #[rstest]
    fn zscore_matches_batch_window() {
        let mut z = ZScore::new(5, None);
        let inputs = [3.0, 5.0, 7.0, 8.0, 1.0, 9.0, 12.0, 4.0, 6.0, 7.0];
        let mut window: ArrayDeque<f64, 5, Wrapping> = ArrayDeque::new();

        for &x in &inputs {
            if window.len() == 5 {
                let _ = window.pop_front();
            }
            let _ = window.push_back(x);
            z.update_raw(x);

            if window.len() >= 2 {
                let w: Vec<f64> = window.iter().copied().collect();
                let (mean, std, batch_z) = batch_zscore(&w);
                assert_approx_equal(z.mean, mean);
                assert_approx_equal(z.std, std);
                assert_approx_equal(z.value, batch_z);
            }
        }
    }

    #[rstest]
    fn zscore_handle_bar_uses_close(bar_ethusdt_binance_minute_bid: Bar) {
        let mut z = ZScore::new(2, None);
        z.handle_bar(&bar_ethusdt_binance_minute_bid);
        z.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert!(z.has_inputs());
        let close: f64 = bar_ethusdt_binance_minute_bid.close.into();
        assert_eq!(z.mean, close);
        assert_eq!(z.value, 0.0);
    }

    #[rstest]
    fn zscore_handle_quote_uses_price_type(indicator_zscore_10: ZScore, stub_quote: QuoteTick) {
        let mut z = indicator_zscore_10;
        z.handle_quote(&stub_quote).unwrap();
        assert_eq!(z.count, 1);
        assert_eq!(z.mean, 1501.0);
        assert_eq!(z.value, 0.0);
    }

    #[rstest]
    fn zscore_handle_trade_uses_price(indicator_zscore_10: ZScore, stub_trade: TradeTick) {
        let mut z = indicator_zscore_10;
        z.handle_trade(&stub_trade);
        assert_eq!(z.count, 1);
        assert_eq!(z.mean, 1500.0);
        assert_eq!(z.value, 0.0);
    }

    #[rstest]
    fn zscore_reset_returns_to_fresh_state(indicator_zscore_10: ZScore) {
        let mut z = indicator_zscore_10;
        for i in 0..20 {
            z.update_raw(f64::from(i));
        }
        z.reset();
        assert!(!z.initialized());
        assert!(!z.has_inputs());
        assert_eq!(z.value, 0.0);
        assert_eq!(z.mean, 0.0);
        assert_eq!(z.std, 0.0);
        assert_eq!(z.count, 0);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed")]
    fn zscore_new_with_period_one_panics() {
        let _ = ZScore::new(1, None);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed")]
    fn zscore_new_with_zero_period_panics() {
        let _ = ZScore::new(0, None);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed")]
    fn zscore_new_with_period_above_max_panics() {
        let _ = ZScore::new(MAX_PERIOD + 1, None);
    }

    #[rstest]
    fn zscore_new_checked_rejects_invalid_period() {
        assert!(ZScore::new_checked(0, None).is_err());
        assert!(ZScore::new_checked(1, None).is_err());
        assert!(ZScore::new_checked(MAX_PERIOD + 1, None).is_err());
        assert!(ZScore::new_checked(2, None).is_ok());
    }

    #[rstest]
    fn zscore_near_equal_large_magnitude_matches_batch() {
        let inputs = [-814.051_168_710_620_9, -813.996_166_896_107_9];
        let mut z = ZScore::new(2, None);
        for &x in &inputs {
            z.update_raw(x);
        }
        let (mean, std, batch_z) = batch_zscore(&inputs);
        assert_approx_equal(z.mean, mean);
        assert_approx_equal(z.std, std);
        assert_approx_equal(z.value, batch_z);
    }

    #[rstest]
    fn zscore_slide_from_large_values_to_zeros_matches_batch() {
        let inputs = [
            858.223_114_833_198,
            -299.638_657_482_500_7,
            -377.208_520_869_421_76,
            -394.324_913_206_254_8,
            406.662_086_491_207_45,
            -912.384_594_640_612_4,
            0.0,
            0.0,
            0.0,
        ];
        let mut z = ZScore::new(2, None);
        let mut window: Vec<f64> = Vec::new();

        for &x in &inputs {
            window.push(x);

            if window.len() > 2 {
                window.remove(0);
            }

            z.update_raw(x);

            if window.len() >= 2 {
                let (mean, std, batch_z) = batch_zscore(&window);
                assert_approx_equal(z.mean, mean);
                assert_approx_equal(z.std, std);
                assert_approx_equal(z.value, batch_z);
            }
        }
    }

    #[rstest]
    fn zscore_slide_from_zeros_to_near_equal_large_matches_batch() {
        let inputs = [
            0.0,
            0.0,
            -665.301_640_322_359_3,
            -786.149_294_354_941_7,
            592.982_187_831_149,
            592.422_790_241_439_3,
        ];
        let mut z = ZScore::new(2, None);
        let mut window: Vec<f64> = Vec::new();

        for &x in &inputs {
            window.push(x);

            if window.len() > 2 {
                window.remove(0);
            }

            z.update_raw(x);

            if window.len() >= 2 {
                let (mean, std, batch_z) = batch_zscore(&window);
                assert_approx_equal(z.mean, mean);
                assert_approx_equal(z.std, std);
                assert_approx_equal(z.value, batch_z);
            }
        }
    }

    proptest! {
        #[rstest]
        fn zscore_streaming_matches_batch_window(
            values in prop::collection::vec(-1_000.0f64..1_000.0, 2..40),
            period in 2usize..16,
        ) {
            let mut z = ZScore::new(period, None);
            let mut window: Vec<f64> = Vec::new();

            for &x in &values {
                window.push(x);
                if window.len() > period {
                    window.remove(0);
                }
                z.update_raw(x);

                if window.len() >= 2 {
                    let (mean, std, batch_z) = batch_zscore(&window);
                    prop_assert!(approx_equal_with(z.mean, mean, 1e-9, 1e-12));
                    prop_assert!(approx_equal_with(z.std, std, 1e-9, 1e-12));
                    prop_assert!(approx_equal_with(z.value, batch_z, 1e-9, 1e-12));
                }
            }
        }
    }
}
