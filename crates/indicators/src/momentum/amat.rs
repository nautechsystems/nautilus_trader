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

use std::fmt::{Debug, Display};

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::data::Bar;

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

const DEFAULT_MA_TYPE: MovingAverageType = MovingAverageType::Exponential;
const MAX_SIGNAL: usize = 1_024;

type SignalBuf = ArrayDeque<f64, { MAX_SIGNAL + 1 }, Wrapping>;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct ArcherMovingAveragesTrends {
    pub fast_period: usize,
    pub slow_period: usize,
    pub signal_period: usize,
    pub ma_type: MovingAverageType,
    pub long_run: bool,
    pub short_run: bool,
    pub initialized: bool,
    fast_ma: Box<dyn MovingAverage + Send + 'static>,
    slow_ma: Box<dyn MovingAverage + Send + 'static>,
    fast_ma_price: SignalBuf,
    slow_ma_price: SignalBuf,
    has_inputs: bool,
}

impl Display for ArcherMovingAveragesTrends {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{},{})",
            self.name(),
            self.fast_period,
            self.slow_period,
            self.signal_period,
            self.ma_type,
        )
    }
}

impl Indicator for ArcherMovingAveragesTrends {
    fn name(&self) -> String {
        stringify!(ArcherMovingAveragesTrends).into()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw(bar.close.into());
    }

    fn reset(&mut self) {
        self.fast_ma.reset();
        self.slow_ma.reset();
        self.long_run = false;
        self.short_run = false;
        self.fast_ma_price.clear();
        self.slow_ma_price.clear();
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl ArcherMovingAveragesTrends {
    /// Creates a new [`ArcherMovingAveragesTrends`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// `fast_period`*, *`slow_period`* or *`signal_period`* is 0.
    /// `slow_period`* ≤ *`fast_period`*.
    /// `signal_period`* > `MAX_SIGNAL`.
    #[must_use]
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        signal_period: usize,
        ma_type: Option<MovingAverageType>,
    ) -> Self {
        assert!(
            fast_period > 0,
            "fast_period must be positive (got {fast_period})"
        );
        assert!(
            slow_period > 0,
            "slow_period must be positive (got {slow_period})"
        );
        assert!(
            signal_period > 0,
            "signal_period must be positive (got {signal_period})"
        );
        assert!(
            slow_period > fast_period,
            "slow_period ({slow_period}) must be greater than fast_period ({fast_period})"
        );
        assert!(
            signal_period <= MAX_SIGNAL,
            "signal_period ({signal_period}) must not exceed MAX_SIGNAL ({MAX_SIGNAL})"
        );

        let ma_type = ma_type.unwrap_or(DEFAULT_MA_TYPE);

        Self {
            fast_period,
            slow_period,
            signal_period,
            ma_type,
            long_run: false,
            short_run: false,
            fast_ma: MovingAverageFactory::create(ma_type, fast_period),
            slow_ma: MovingAverageFactory::create(ma_type, slow_period),
            fast_ma_price: SignalBuf::new(),
            slow_ma_price: SignalBuf::new(),
            has_inputs: false,
            initialized: false,
        }
    }

    /// Updates the indicator with a new raw price value.
    ///
    /// # Panics
    /// This method will panic if the `slow_ma` is not initialized yet.
    pub fn update_raw(&mut self, close: f64) {
        self.fast_ma.update_raw(close);
        self.slow_ma.update_raw(close);

        if self.slow_ma.initialized() {
            self.fast_ma_price.push_back(self.fast_ma.value());
            self.slow_ma_price.push_back(self.slow_ma.value());

            let max_len = self.signal_period + 1;
            if self.fast_ma_price.len() > max_len {
                self.fast_ma_price.pop_front();
                self.slow_ma_price.pop_front();
            }

            let fast_back = self.fast_ma.value();
            let fast_front = *self
                .fast_ma_price
                .front()
                .expect("buffer has at least one element");

            let fast_diff = fast_back - fast_front;
            self.long_run = fast_diff > 0.0 || self.long_run;
            self.short_run = fast_diff < 0.0 || self.short_run;
        }

        if !self.initialized {
            self.has_inputs = true;
            let max_len = self.signal_period + 1;
            if self.slow_ma_price.len() == max_len && self.slow_ma.initialized() {
                self.initialized = true;
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::stubs::amat_345;

    fn make(fast: usize, slow: usize, signal: usize) {
        let _ = ArcherMovingAveragesTrends::new(fast, slow, signal, None);
    }

    #[rstest]
    fn default_ma_type_is_exponential() {
        let ind = ArcherMovingAveragesTrends::new(3, 4, 5, None);
        assert_eq!(ind.ma_type, MovingAverageType::Exponential);
    }

    #[rstest]
    fn test_name_returns_expected_string(amat_345: ArcherMovingAveragesTrends) {
        assert_eq!(amat_345.name(), "ArcherMovingAveragesTrends");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(amat_345: ArcherMovingAveragesTrends) {
        assert_eq!(
            format!("{amat_345}"),
            "ArcherMovingAveragesTrends(3,4,5,SIMPLE)"
        );
    }

    #[rstest]
    fn test_period_returns_expected_value(amat_345: ArcherMovingAveragesTrends) {
        assert_eq!(amat_345.fast_period, 3);
        assert_eq!(amat_345.slow_period, 4);
        assert_eq!(amat_345.signal_period, 5);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(amat_345: ArcherMovingAveragesTrends) {
        assert!(!amat_345.initialized());
    }

    #[rstest]
    #[should_panic(expected = "fast_period must be positive")]
    fn new_panics_on_zero_fast_period() {
        make(0, 4, 5);
    }

    #[rstest]
    #[should_panic(expected = "slow_period must be positive")]
    fn new_panics_on_zero_slow_period() {
        make(3, 0, 5);
    }

    #[rstest]
    #[should_panic(expected = "signal_period must be positive")]
    fn new_panics_on_zero_signal_period() {
        make(3, 5, 0);
    }

    #[rstest]
    #[should_panic(expected = "slow_period (3) must be greater than fast_period (3)")]
    fn new_panics_when_slow_not_greater_than_fast() {
        make(3, 3, 5);
    }

    #[rstest]
    #[should_panic(expected = "slow_period (2) must be greater than fast_period (3)")]
    fn new_panics_when_slow_less_than_fast() {
        make(3, 2, 5);
    }

    fn feed_sequence(ind: &mut ArcherMovingAveragesTrends, start: i64, count: usize, step: i64) {
        (0..count).for_each(|i| ind.update_raw((start + i as i64 * step) as f64));
    }

    #[rstest]
    fn buffer_len_never_exceeds_signal_plus_one() {
        let mut ind = ArcherMovingAveragesTrends::new(3, 4, 5, None);
        feed_sequence(&mut ind, 0, 100, 1);
        assert_eq!(ind.fast_ma_price.len(), ind.signal_period + 1);
        assert_eq!(ind.slow_ma_price.len(), ind.signal_period + 1);
    }

    #[rstest]
    fn initialized_becomes_true_after_slow_ready_and_buffer_full() {
        let mut ind = ArcherMovingAveragesTrends::new(3, 4, 5, None);
        feed_sequence(&mut ind, 0, 11, 1); // 11 > 4+6
        assert!(ind.initialized());
    }

    #[rstest]
    fn long_run_flag_sets_on_bullish_trend() {
        let mut ind = ArcherMovingAveragesTrends::new(3, 4, 5, None);
        feed_sequence(&mut ind, 0, 60, 1);
        assert!(ind.long_run, "Expected long_run=TRUE on up-trend");
        assert!(!ind.short_run, "short_run should remain FALSE here");
    }

    #[rstest]
    fn short_run_flag_sets_on_bearish_trend() {
        let mut ind = ArcherMovingAveragesTrends::new(3, 4, 5, None);
        feed_sequence(&mut ind, 100, 60, -1);
        assert!(ind.short_run, "Expected short_run=TRUE on down-trend");
        assert!(!ind.long_run, "long_run should remain FALSE here");
    }

    #[rstest]
    fn reset_clears_internal_state() {
        let mut ind = ArcherMovingAveragesTrends::new(3, 4, 5, None);
        feed_sequence(&mut ind, 0, 50, 1);
        assert!(ind.long_run || ind.short_run);
        assert!(!ind.fast_ma_price.is_empty());

        ind.reset();

        assert!(!ind.long_run && !ind.short_run);
        assert_eq!(ind.fast_ma_price.len(), 0);
        assert_eq!(ind.slow_ma_price.len(), 0);
        assert!(!ind.initialized());
        assert!(!ind.has_inputs());
    }

    #[rstest]
    #[should_panic(expected = "signal_period (1025) must not exceed MAX_SIGNAL (1024)")]
    fn new_panics_when_signal_exceeds_max() {
        let _ = ArcherMovingAveragesTrends::new(3, 4, MAX_SIGNAL + 1, None);
    }

    #[rstest]
    fn ma_type_override_is_respected() {
        let ind = ArcherMovingAveragesTrends::new(3, 4, 5, Some(MovingAverageType::Simple));
        assert_eq!(ind.ma_type, MovingAverageType::Simple);
    }
}
