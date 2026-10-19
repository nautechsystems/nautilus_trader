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

use std::{collections::VecDeque, fmt::Display};

use nautilus_core::correctness::FAILED;
use nautilus_model::data::Bar;

use crate::{
    indicator::Indicator,
    support::{MAX_PERIOD, is_valid_hlc},
};

/// Ichimoku Kinko Hyo: the five-line cloud chart.
///
/// ```text
/// tenkan_sen    = midpoint(high, low over tenkan_period)
/// kijun_sen     = midpoint(high, low over kijun_period)
/// senkou_span_a = (tenkan_sen + kijun_sen) / 2      as computed `displacement - 1` bars ago
/// senkou_span_b = midpoint(high, low over senkou_period) as computed `displacement - 1` bars ago
/// chikou_span   = close from `displacement - 1` bars ago
/// ```
///
/// The two Senkou spans form the Kumo (cloud). Charts draw them `displacement`
/// bars ahead and the Chikou span `displacement` bars behind; streaming in
/// chronological order, the values visible at bar `n` are the ones buffered
/// `displacement` updates ago, that is bar `n - displacement + 1`.
///
/// Each line becomes available at its own bar: `tenkan_sen` after `tenkan_period`
/// bars, `kijun_sen` after `kijun_period`, `chikou_span` after `displacement`,
/// `senkou_span_a` after `kijun_period + displacement - 1`, and `senkou_span_b`
/// after `senkou_period + displacement - 1` (77 bars at the classic
/// `(9, 26, 52, 26)`). The matching `has_*` flag reports whether each field
/// holds a value, and `initialized` gates on all five.
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
pub struct IchimokuCloud {
    pub tenkan_period: usize,
    pub kijun_period: usize,
    pub senkou_period: usize,
    pub displacement: usize,
    pub tenkan_sen: f64,
    pub kijun_sen: f64,
    pub senkou_span_a: f64,
    pub senkou_span_b: f64,
    pub chikou_span: f64,
    pub has_tenkan: bool,
    pub has_kijun: bool,
    pub has_senkou_a: bool,
    pub has_senkou_b: bool,
    pub has_chikou: bool,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    highs: VecDeque<f64>,
    lows: VecDeque<f64>,
    senkou_a_history: VecDeque<f64>,
    senkou_b_history: VecDeque<f64>,
    close_history: VecDeque<f64>,
}

impl Display for IchimokuCloud {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{},{})",
            self.name(),
            self.tenkan_period,
            self.kijun_period,
            self.senkou_period,
            self.displacement,
        )
    }
}

impl Indicator for IchimokuCloud {
    fn name(&self) -> String {
        stringify!(IchimokuCloud).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into(), (&bar.close).into());
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.senkou_a_history.clear();
        self.senkou_b_history.clear();
        self.close_history.clear();
        self.tenkan_sen = 0.0;
        self.kijun_sen = 0.0;
        self.senkou_span_a = 0.0;
        self.senkou_span_b = 0.0;
        self.chikou_span = 0.0;
        self.has_tenkan = false;
        self.has_kijun = false;
        self.has_senkou_a = false;
        self.has_senkou_b = false;
        self.has_chikou = false;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl IchimokuCloud {
    /// Creates a new [`IchimokuCloud`] instance.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - any of `tenkan_period`, `kijun_period`, `senkou_period` or
    ///   `displacement` is zero or exceeds 16,777,216.
    /// - the periods are not non-decreasing
    ///   (`tenkan_period <= kijun_period <= senkou_period`).
    #[must_use]
    pub fn new(
        tenkan_period: usize,
        kijun_period: usize,
        senkou_period: usize,
        displacement: usize,
    ) -> Self {
        Self::new_checked(tenkan_period, kijun_period, senkou_period, displacement).expect(FAILED)
    }

    pub(crate) fn new_checked(
        tenkan_period: usize,
        kijun_period: usize,
        senkou_period: usize,
        displacement: usize,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            tenkan_period <= MAX_PERIOD,
            "tenkan_period must not exceed {MAX_PERIOD}"
        );
        anyhow::ensure!(
            kijun_period <= MAX_PERIOD,
            "kijun_period must not exceed {MAX_PERIOD}"
        );
        anyhow::ensure!(
            senkou_period <= MAX_PERIOD,
            "senkou_period must not exceed {MAX_PERIOD}"
        );
        anyhow::ensure!(
            displacement <= MAX_PERIOD,
            "displacement must not exceed {MAX_PERIOD}"
        );

        anyhow::ensure!(
            tenkan_period > 0,
            "IchimokuCloud: tenkan_period must be > 0 (received {tenkan_period})"
        );
        anyhow::ensure!(
            kijun_period > 0,
            "IchimokuCloud: kijun_period must be > 0 (received {kijun_period})"
        );
        anyhow::ensure!(
            senkou_period > 0,
            "IchimokuCloud: senkou_period must be > 0 (received {senkou_period})"
        );
        anyhow::ensure!(
            displacement > 0,
            "IchimokuCloud: displacement must be > 0 (received {displacement})"
        );
        anyhow::ensure!(
            kijun_period >= tenkan_period,
            "IchimokuCloud: kijun_period must be >= tenkan_period"
        );
        anyhow::ensure!(
            senkou_period >= kijun_period,
            "IchimokuCloud: senkou_period must be >= kijun_period"
        );
        Ok(Self {
            tenkan_period,
            kijun_period,
            senkou_period,
            displacement,
            tenkan_sen: 0.0,
            kijun_sen: 0.0,
            senkou_span_a: 0.0,
            senkou_span_b: 0.0,
            chikou_span: 0.0,
            has_tenkan: false,
            has_kijun: false,
            has_senkou_a: false,
            has_senkou_b: false,
            has_chikou: false,
            count: 0,
            initialized: false,
            has_inputs: false,
            highs: VecDeque::with_capacity(senkou_period),
            lows: VecDeque::with_capacity(senkou_period),
            senkou_a_history: VecDeque::with_capacity(displacement),
            senkou_b_history: VecDeque::with_capacity(displacement),
            close_history: VecDeque::with_capacity(displacement),
        })
    }

    /// Updates the indicator with the given high, low and close.
    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        if !is_valid_hlc(high, low, close) {
            return;
        }

        self.count += 1;
        self.has_inputs = true;

        if self.highs.len() == self.senkou_period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        self.highs.push_back(high);
        self.lows.push_back(low);

        if self.highs.len() >= self.tenkan_period {
            self.tenkan_sen = self.midpoint(self.tenkan_period);
            self.has_tenkan = true;
        }

        if self.highs.len() >= self.kijun_period {
            self.kijun_sen = self.midpoint(self.kijun_period);
            self.has_kijun = true;
        }
        let senkou_b_now = if self.highs.len() >= self.senkou_period {
            self.midpoint(self.senkou_period)
        } else {
            f64::NAN
        };
        let senkou_a_now = if self.has_tenkan && self.has_kijun {
            f64::midpoint(self.tenkan_sen, self.kijun_sen)
        } else {
            f64::NAN
        };

        // Push every bar (NaN encodes "no value yet") so the buffers stay
        // aligned 1:1 with bars and the displaced read is a plain front peek.
        Self::push_capped(&mut self.senkou_a_history, senkou_a_now, self.displacement);
        Self::push_capped(&mut self.senkou_b_history, senkou_b_now, self.displacement);
        Self::push_capped(&mut self.close_history, close, self.displacement);

        if let Some(value) = Self::displaced(&self.senkou_a_history, self.displacement) {
            self.senkou_span_a = value;
            self.has_senkou_a = true;
        }

        if let Some(value) = Self::displaced(&self.senkou_b_history, self.displacement) {
            self.senkou_span_b = value;
            self.has_senkou_b = true;
        }

        if let Some(value) = Self::displaced(&self.close_history, self.displacement) {
            self.chikou_span = value;
            self.has_chikou = true;
        }

        self.initialized = self.has_tenkan
            && self.has_kijun
            && self.has_senkou_a
            && self.has_senkou_b
            && self.has_chikou;
    }

    // Midpoint of the last `n` highs and lows; the caller guarantees `n` bars exist.
    fn midpoint(&self, n: usize) -> f64 {
        let len = self.highs.len();
        let start = len - n;
        let mut hi = f64::NEG_INFINITY;
        let mut lo = f64::INFINITY;

        for i in start..len {
            hi = hi.max(self.highs[i]);
            lo = lo.min(self.lows[i]);
        }
        f64::midpoint(hi, lo)
    }

    fn push_capped(queue: &mut VecDeque<f64>, value: f64, cap: usize) {
        if queue.len() == cap {
            queue.pop_front();
        }
        queue.push_back(value);
    }

    // The value buffered `displacement` updates ago, once the buffer is full.
    fn displaced(queue: &VecDeque<f64>, cap: usize) -> Option<f64> {
        if queue.len() == cap && !queue[0].is_nan() {
            Some(queue[0])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::indicator::Indicator;

    fn classic() -> IchimokuCloud {
        IchimokuCloud::new(9, 26, 52, 26)
    }

    fn ramp(n: i32) -> Vec<(f64, f64, f64)> {
        (0..n)
            .map(|i| {
                let p = 100.0 + f64::from(i);
                (p + 2.0, p - 2.0, p + 1.0)
            })
            .collect()
    }

    #[rstest]
    fn test_name_and_display() {
        let ichi = classic();
        assert_eq!(ichi.name(), "IchimokuCloud");
        assert_eq!(format!("{ichi}"), "IchimokuCloud(9,26,52,26)");
        assert_eq!(ichi.tenkan_period, 9);
        assert_eq!(ichi.kijun_period, 26);
        assert_eq!(ichi.senkou_period, 52);
        assert_eq!(ichi.displacement, 26);
        assert!(!ichi.initialized());
        assert!(!ichi.has_inputs());
    }

    #[rstest]
    #[should_panic(expected = "tenkan_period must be > 0")]
    fn test_zero_tenkan_period_panics() {
        let _ = IchimokuCloud::new(0, 26, 52, 26);
    }

    #[rstest]
    #[should_panic(expected = "kijun_period must be > 0")]
    fn test_zero_kijun_period_panics() {
        let _ = IchimokuCloud::new(9, 0, 52, 26);
    }

    #[rstest]
    #[should_panic(expected = "senkou_period must be > 0")]
    fn test_zero_senkou_period_panics() {
        let _ = IchimokuCloud::new(9, 26, 0, 26);
    }

    #[rstest]
    #[should_panic(expected = "displacement must be > 0")]
    fn test_zero_displacement_panics() {
        let _ = IchimokuCloud::new(9, 26, 52, 0);
    }

    #[rstest]
    #[should_panic(expected = "kijun_period must be >= tenkan_period")]
    fn test_kijun_below_tenkan_panics() {
        let _ = IchimokuCloud::new(27, 26, 52, 26);
    }

    #[rstest]
    #[should_panic(expected = "senkou_period must be >= kijun_period")]
    fn test_senkou_below_kijun_panics() {
        let _ = IchimokuCloud::new(9, 26, 25, 26);
    }

    #[rstest]
    fn test_warmup_boundaries_per_line() {
        let mut ichi = classic();
        for (i, &(h, l, c)) in ramp(80).iter().enumerate() {
            ichi.update_raw(h, l, c);
            let bars = i + 1;
            assert_eq!(ichi.has_tenkan, bars >= 9, "tenkan at bar {bars}");
            assert_eq!(ichi.has_kijun, bars >= 26, "kijun at bar {bars}");
            assert_eq!(ichi.has_chikou, bars >= 26, "chikou at bar {bars}");
            // senkou_a first reads a real value at kijun_period + displacement - 1.
            assert_eq!(ichi.has_senkou_a, bars >= 51, "senkou_a at bar {bars}");
            // senkou_b first reads a real value at senkou_period + displacement - 1.
            assert_eq!(ichi.has_senkou_b, bars >= 77, "senkou_b at bar {bars}");
            assert_eq!(ichi.initialized(), bars >= 77, "initialized at bar {bars}");
        }
    }

    #[rstest]
    fn test_ramp_tenkan_equals_window_midpoint() {
        // On a strict ramp the 9-bar window at bar 9 spans highs 102..110 and
        // lows 98..106, so the midpoint is (110 + 98) / 2 = 104.
        let mut ichi = classic();
        for &(h, l, c) in &ramp(9) {
            ichi.update_raw(h, l, c);
        }
        assert!(ichi.has_tenkan);
        assert_eq!(ichi.tenkan_sen, 104.0);
    }

    #[rstest]
    fn test_chikou_is_close_displacement_bars_back() {
        let candles = ramp(60);
        let mut ichi = classic();
        for (i, &(h, l, c)) in candles.iter().enumerate() {
            ichi.update_raw(h, l, c);

            if i + 1 >= 26 {
                // At bar n the visible chikou is the close from bar n - 25.
                assert_eq!(ichi.chikou_span, candles[i + 1 - 26].2, "bar {}", i + 1);
            }
        }
    }

    #[rstest]
    fn test_senkou_a_is_midpoint_of_lines_displacement_bars_back() {
        let candles = ramp(80);
        let mut reference = classic();
        let mut history = Vec::new();

        for &(h, l, c) in &candles {
            reference.update_raw(h, l, c);
            history.push(if reference.has_tenkan && reference.has_kijun {
                Some(f64::midpoint(reference.tenkan_sen, reference.kijun_sen))
            } else {
                None
            });
        }
        let mut ichi = classic();
        for (i, &(h, l, c)) in candles.iter().enumerate() {
            ichi.update_raw(h, l, c);

            if i + 1 >= 26
                && let Some(want) = history[i + 1 - 26]
            {
                assert_eq!(ichi.senkou_span_a, want, "bar {}", i + 1);
            }
        }
    }

    #[rstest]
    fn test_reset() {
        let mut ichi = classic();
        for &(h, l, c) in &ramp(100) {
            ichi.update_raw(h, l, c);
        }
        assert!(ichi.initialized());
        ichi.reset();
        assert!(!ichi.initialized());
        assert!(!ichi.has_inputs());
        assert_eq!(ichi.tenkan_sen, 0.0);
        assert_eq!(ichi.kijun_sen, 0.0);
        assert_eq!(ichi.senkou_span_a, 0.0);
        assert_eq!(ichi.senkou_span_b, 0.0);
        assert_eq!(ichi.chikou_span, 0.0);
        assert!(!ichi.has_tenkan);
        assert!(!ichi.has_kijun);
        assert!(!ichi.has_senkou_a);
        assert!(!ichi.has_senkou_b);
        assert!(!ichi.has_chikou);
        assert_eq!(ichi.count, 0);

        // A fresh instance and the reset instance must agree thereafter.
        let mut fresh = classic();

        for &(h, l, c) in &ramp(90) {
            ichi.update_raw(h, l, c);
            fresh.update_raw(h, l, c);
        }
        assert_eq!(ichi.tenkan_sen, fresh.tenkan_sen);
        assert_eq!(ichi.kijun_sen, fresh.kijun_sen);
        assert_eq!(ichi.senkou_span_a, fresh.senkou_span_a);
        assert_eq!(ichi.senkou_span_b, fresh.senkou_span_b);
        assert_eq!(ichi.chikou_span, fresh.chikou_span);
    }

    #[rstest]
    fn test_custom_periods_accepted() {
        let mut ichi = IchimokuCloud::new(5, 10, 20, 10);
        for &(h, l, c) in &ramp(40) {
            ichi.update_raw(h, l, c);
        }
        assert!(ichi.initialized());
    }
}
