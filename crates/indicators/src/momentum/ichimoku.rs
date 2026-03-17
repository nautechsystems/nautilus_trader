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

//! Ichimoku Cloud (Kinko Hyo) indicator.

use std::fmt::Display;

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::data::Bar;

use crate::indicator::Indicator;

const MAX_PERIOD: usize = 128;
const MAX_DISPLACEMENT: usize = 64;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
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
    pub initialized: bool,
    has_inputs: bool,
    highs: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    lows: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    senkou_a: ArrayDeque<f64, MAX_DISPLACEMENT, Wrapping>,
    senkou_b: ArrayDeque<f64, MAX_DISPLACEMENT, Wrapping>,
    chikou: ArrayDeque<f64, MAX_DISPLACEMENT, Wrapping>,
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
        self.senkou_a.clear();
        self.senkou_b.clear();
        self.chikou.clear();
        self.tenkan_sen = 0.0;
        self.kijun_sen = 0.0;
        self.senkou_span_a = 0.0;
        self.senkou_span_b = 0.0;
        self.chikou_span = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl IchimokuCloud {
    /// Creates a new [`IchimokuCloud`] instance.
    ///
    /// The indicator becomes `initialized` after `senkou_period` bars,
    /// at which point `tenkan_sen` and `kijun_sen` are valid. The displaced
    /// outputs (`senkou_span_a`, `senkou_span_b`, `chikou_span`) require an
    /// additional `displacement` bars before they become non-zero.
    ///
    /// # Panics
    ///
    /// Panics if periods are invalid: `tenkan_period` and others must be positive,
    /// `kijun_period >= tenkan_period`, `senkou_period >= kijun_period`,
    /// and all within allowed maximums.
    #[must_use]
    pub fn new(
        tenkan_period: usize,
        kijun_period: usize,
        senkou_period: usize,
        displacement: usize,
    ) -> Self {
        assert!(
            tenkan_period > 0 && tenkan_period <= MAX_PERIOD,
            "IchimokuCloud: tenkan_period must be in 1..={MAX_PERIOD}"
        );
        assert!(
            kijun_period > 0 && kijun_period <= MAX_PERIOD,
            "IchimokuCloud: kijun_period must be in 1..={MAX_PERIOD}"
        );
        assert!(
            senkou_period > 0 && senkou_period <= MAX_PERIOD,
            "IchimokuCloud: senkou_period must be in 1..={MAX_PERIOD}"
        );
        assert!(
            displacement > 0 && displacement <= MAX_DISPLACEMENT,
            "IchimokuCloud: displacement must be in 1..={MAX_DISPLACEMENT}"
        );
        assert!(
            kijun_period >= tenkan_period,
            "IchimokuCloud: kijun_period must be >= tenkan_period"
        );
        assert!(
            senkou_period >= kijun_period,
            "IchimokuCloud: senkou_period must be >= kijun_period"
        );

        Self {
            tenkan_period,
            kijun_period,
            senkou_period,
            displacement,
            tenkan_sen: 0.0,
            kijun_sen: 0.0,
            senkou_span_a: 0.0,
            senkou_span_b: 0.0,
            chikou_span: 0.0,
            initialized: false,
            has_inputs: false,
            highs: ArrayDeque::new(),
            lows: ArrayDeque::new(),
            senkou_a: ArrayDeque::new(),
            senkou_b: ArrayDeque::new(),
            chikou: ArrayDeque::new(),
        }
    }

    /// Updates the indicator with OHLC values.
    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        let _ = self.highs.push_back(high);
        let _ = self.lows.push_back(low);

        if !self.initialized {
            self.has_inputs = true;
            let n = self.highs.len();
            if n >= self.tenkan_period && n >= self.kijun_period && n >= self.senkou_period {
                self.initialized = true;
            }
        }

        self.tenkan_sen = Self::midpoint_over(&self.highs, &self.lows, self.tenkan_period);
        self.kijun_sen = Self::midpoint_over(&self.highs, &self.lows, self.kijun_period);
        let mid52 = Self::midpoint_over(&self.highs, &self.lows, self.senkou_period);

        if self.initialized {
            if self.senkou_a.len() == self.displacement {
                self.senkou_span_a = self.senkou_a.pop_front().unwrap_or(0.0);
            }
            let _ = self
                .senkou_a
                .push_back((self.tenkan_sen + self.kijun_sen) / 2.0);

            if self.senkou_b.len() == self.displacement {
                self.senkou_span_b = self.senkou_b.pop_front().unwrap_or(0.0);
            }
            let _ = self.senkou_b.push_back(mid52);

            if self.chikou.len() == self.displacement {
                self.chikou_span = self.chikou.pop_front().unwrap_or(0.0);
            }
            let _ = self.chikou.push_back(close);
        }
    }

    fn midpoint_over(
        highs: &ArrayDeque<f64, MAX_PERIOD, Wrapping>,
        lows: &ArrayDeque<f64, MAX_PERIOD, Wrapping>,
        period: usize,
    ) -> f64 {
        if highs.len() < period || lows.len() < period {
            return 0.0;
        }
        let high_max = highs
            .iter()
            .rev()
            .take(period)
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let low_min = lows
            .iter()
            .rev()
            .take(period)
            .copied()
            .fold(f64::INFINITY, f64::min);
        (high_max + low_min) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;
    use crate::indicator::Indicator;

    #[fixture]
    fn ich_default() -> IchimokuCloud {
        IchimokuCloud::new(9, 26, 52, 26)
    }

    #[rstest]
    fn test_name(ich_default: IchimokuCloud) {
        assert_eq!(ich_default.name(), "IchimokuCloud");
    }

    #[rstest]
    fn test_display(ich_default: IchimokuCloud) {
        assert_eq!(format!("{ich_default}"), "IchimokuCloud(9,26,52,26)");
    }

    #[rstest]
    fn test_initialized_without_inputs(ich_default: IchimokuCloud) {
        assert!(!ich_default.initialized());
        assert!(!ich_default.has_inputs());
    }

    #[rstest]
    fn test_tenkan_after_nine_bars(mut ich_default: IchimokuCloud) {
        for _ in 0..9 {
            ich_default.update_raw(12.0, 8.0, 10.0);
        }
        assert_eq!(ich_default.tenkan_sen, 10.0);
    }

    #[rstest]
    fn test_kijun_after_twenty_six_bars(mut ich_default: IchimokuCloud) {
        for _ in 0..26 {
            ich_default.update_raw(12.0, 8.0, 10.0);
        }
        assert_eq!(ich_default.kijun_sen, 10.0);
    }

    #[rstest]
    fn test_initialized_after_fifty_two_bars(mut ich_default: IchimokuCloud) {
        for _ in 0..52 {
            ich_default.update_raw(10.0, 8.0, 9.0);
        }
        assert!(ich_default.initialized());
    }

    #[rstest]
    fn test_senkou_chikou_after_displacement_bars(mut ich_default: IchimokuCloud) {
        for _ in 0..(52 + 26) {
            ich_default.update_raw(12.0, 8.0, 10.0);
        }
        assert_eq!(ich_default.senkou_span_a, 10.0);
        assert_eq!(ich_default.senkou_span_b, 10.0);
        assert_eq!(ich_default.chikou_span, 10.0);
    }

    #[rstest]
    fn test_reset(mut ich_default: IchimokuCloud) {
        for _ in 0..20 {
            ich_default.update_raw(10.0, 8.0, 9.0);
        }
        ich_default.reset();
        assert!(!ich_default.initialized());
        assert_eq!(ich_default.tenkan_sen, 0.0);
        assert_eq!(ich_default.kijun_sen, 0.0);
        assert_eq!(ich_default.senkou_span_a, 0.0);
        assert_eq!(ich_default.senkou_span_b, 0.0);
        assert_eq!(ich_default.chikou_span, 0.0);
    }

    #[rstest]
    fn test_tenkan_sen_updates_with_varying_data() {
        let mut ich = IchimokuCloud::new(3, 3, 3, 2);

        // Fill the window: highs=[10, 12, 14], lows=[5, 6, 7]
        ich.update_raw(10.0, 5.0, 8.0);
        ich.update_raw(12.0, 6.0, 9.0);
        ich.update_raw(14.0, 7.0, 10.0);
        assert_eq!(ich.tenkan_sen, (14.0 + 5.0) / 2.0); // 9.5

        // Push a new bar that evicts the (10, 5) pair: highs=[12, 14, 8], lows=[6, 7, 3]
        ich.update_raw(8.0, 3.0, 6.0);
        assert_eq!(ich.tenkan_sen, (14.0 + 3.0) / 2.0); // 8.5

        // Push another bar that evicts the (12, 6) pair: highs=[14, 8, 20], lows=[7, 3, 4]
        ich.update_raw(20.0, 4.0, 12.0);
        assert_eq!(ich.tenkan_sen, (20.0 + 3.0) / 2.0); // 11.5
    }

    #[rstest]
    #[should_panic(expected = "kijun_period must be >= tenkan_period")]
    fn test_new_panics_invalid_kijun() {
        let _ = IchimokuCloud::new(9, 5, 52, 26);
    }

    #[rstest]
    #[should_panic(expected = "senkou_period must be >= kijun_period")]
    fn test_new_panics_invalid_senkou() {
        let _ = IchimokuCloud::new(9, 26, 20, 26);
    }

    #[rstest]
    #[should_panic(expected = "displacement must be in 1..=")]
    fn test_new_panics_invalid_displacement() {
        let _ = IchimokuCloud::new(9, 26, 52, 0);
    }

    #[rstest]
    fn test_custom_periods_initialization() {
        let mut ich = IchimokuCloud::new(5, 10, 20, 10);
        assert_eq!(ich.tenkan_period, 5);
        assert_eq!(ich.kijun_period, 10);
        assert_eq!(ich.senkou_period, 20);
        assert_eq!(ich.displacement, 10);
        for _ in 0..20 {
            ich.update_raw(1.0, 1.0, 1.0);
        }
        assert!(ich.initialized());
        assert_eq!(ich.tenkan_sen, 1.0);
        assert_eq!(ich.kijun_sen, 1.0);
    }
}
