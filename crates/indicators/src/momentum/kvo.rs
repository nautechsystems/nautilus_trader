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

use std::fmt::{Debug, Display};

use nautilus_core::correctness::FAILED;
use nautilus_model::data::Bar;

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
    support::{MAX_PERIOD, is_valid_hlc},
};

/// Stephen J. Klinger's Volume Oscillator: a fast/slow moving-average
/// difference over the per-bar "volume force".
///
/// ```text
/// dm_t   = high_t - low_t                           (the daily measurement)
/// trend  = sign(hlc_t - hlc_{t-1}), carried over when equal, with hlc = high + low + close
/// cm_t   = cm_{t-1} + dm_t        while the trend holds
/// cm_t   = dm_{t-1} + dm_t        when the trend flips
/// vf_t   = volume_t * |2 * (dm_t / cm_t - 1)| * trend * 100
/// KVO_t  = MA(vf, fast) - MA(vf, slow)
/// ```
///
/// Klinger's textbook configuration is `fast = 34, slow = 55` with exponential
/// averages, which is the default `ma_type`.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.indicators", unsendable)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.indicators")
)]
pub struct KlingerVolumeOscillator {
    pub fast_period: usize,
    pub slow_period: usize,
    pub ma_type: MovingAverageType,
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    fast_ma: Box<dyn MovingAverage + Send + 'static>,
    slow_ma: Box<dyn MovingAverage + Send + 'static>,
    previous_hlc: Option<f64>,
    previous_dm: f64,
    trend: i8,
    cm: f64,
}

impl Display for KlingerVolumeOscillator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{})",
            self.name(),
            self.fast_period,
            self.slow_period,
            self.ma_type,
        )
    }
}

impl Indicator for KlingerVolumeOscillator {
    fn name(&self) -> String {
        stringify!(KlingerVolumeOscillator).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw(
            (&bar.high).into(),
            (&bar.low).into(),
            (&bar.close).into(),
            (&bar.volume).into(),
        );
    }

    fn reset(&mut self) {
        self.fast_ma.reset();
        self.slow_ma.reset();
        self.previous_hlc = None;
        self.previous_dm = 0.0;
        self.trend = 0;
        self.cm = 0.0;
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl KlingerVolumeOscillator {
    /// Creates a new [`KlingerVolumeOscillator`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `fast_period` is zero or not less than `slow_period`.
    #[must_use]
    pub fn new(fast_period: usize, slow_period: usize, ma_type: Option<MovingAverageType>) -> Self {
        Self::new_checked(fast_period, slow_period, ma_type).expect(FAILED)
    }

    pub(crate) fn new_checked(
        fast_period: usize,
        slow_period: usize,
        ma_type: Option<MovingAverageType>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            fast_period <= MAX_PERIOD,
            "fast_period must not exceed {MAX_PERIOD}"
        );
        anyhow::ensure!(
            slow_period <= MAX_PERIOD,
            "slow_period must not exceed {MAX_PERIOD}"
        );

        anyhow::ensure!(
            fast_period > 0 && fast_period < slow_period,
            "KlingerVolumeOscillator: fast_period must be > 0 and < slow_period (received fast {fast_period}, slow {slow_period})"
        );
        let ma_type = ma_type.unwrap_or(MovingAverageType::Exponential);

        Ok(Self {
            fast_period,
            slow_period,
            ma_type,
            value: 0.0,
            fast_ma: MovingAverageFactory::create(ma_type, fast_period),
            slow_ma: MovingAverageFactory::create(ma_type, slow_period),
            previous_hlc: None,
            previous_dm: 0.0,
            trend: 0,
            cm: 0.0,
            has_inputs: false,
            initialized: false,
        })
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64, volume: f64) {
        if !is_valid_hlc(high, low, close) || !volume.is_finite() || volume < 0.0 {
            return;
        }

        self.has_inputs = true;
        let hlc = high + low + close;
        let dm = high - low;

        let Some(previous_hlc) = self.previous_hlc else {
            // The first bar only establishes the previous trend sum and daily measurement
            self.previous_hlc = Some(hlc);
            self.previous_dm = dm;
            return;
        };

        let new_trend: i8 = if hlc > previous_hlc {
            1
        } else if hlc < previous_hlc {
            -1
        } else {
            self.trend
        };

        // The cumulative measurement resets to (previous_dm + dm) whenever the
        // trend flips, and seeds the same way on the first sign read
        if new_trend == self.trend && self.trend != 0 {
            self.cm += dm;
        } else {
            self.cm = self.previous_dm + dm;
        }
        self.trend = new_trend;

        let volume_force = if self.cm == 0.0 {
            // Zero-range bars since the last trend flip: no force to register
            0.0
        } else {
            volume * (2.0 * (dm / self.cm - 1.0)).abs() * f64::from(new_trend) * 100.0
        };

        self.previous_hlc = Some(hlc);
        self.previous_dm = dm;

        self.fast_ma.update_raw(volume_force);
        self.slow_ma.update_raw(volume_force);
        if !self.initialized && self.fast_ma.initialized() && self.slow_ma.initialized() {
            self.initialized = true;
        }

        if self.initialized {
            self.value = self.fast_ma.value() - self.slow_ma.value();
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn kvo_34() -> KlingerVolumeOscillator {
        KlingerVolumeOscillator::new(3, 4, Some(MovingAverageType::Simple))
    }

    #[rstest]
    fn test_name_returns_expected_string(kvo_34: KlingerVolumeOscillator) {
        assert_eq!(kvo_34.name(), "KlingerVolumeOscillator");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(kvo_34: KlingerVolumeOscillator) {
        assert_eq!(format!("{kvo_34}"), "KlingerVolumeOscillator(3,4,SIMPLE)");
    }

    #[rstest]
    fn test_period_returns_expected_value(kvo_34: KlingerVolumeOscillator) {
        assert_eq!(kvo_34.fast_period, 3);
        assert_eq!(kvo_34.slow_period, 4);
    }

    #[rstest]
    #[should_panic(expected = "fast_period must be > 0 and < slow_period")]
    fn test_fast_not_less_than_slow_panics() {
        let _ = KlingerVolumeOscillator::new(4, 4, None);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(kvo_34: KlingerVolumeOscillator) {
        assert!(!kvo_34.initialized());
    }

    #[rstest]
    fn test_volume_force_matches_reference(mut kvo_34: KlingerVolumeOscillator) {
        // (high, low, close) with hlc = high + low + close and dm = high - low:
        // hlc = [5, 10, 11, 18, 13], dm = [1, 2, 1, 3, 2], volume 10.
        // Bar 2: trend +1, cm = 1 + 2 = 3, vf = 10 * |2 * (2/3 - 1)| * 100 = 2000/3
        // Bar 3: trend +1, cm = 3 + 1 = 4, vf = 10 * |2 * (1/4 - 1)| * 100 = 1500
        // Bar 4: trend +1, cm = 4 + 3 = 7, vf = 10 * |2 * (3/7 - 1)| * 100 = 8000/7
        // Bar 5: trend -1, cm = 3 + 2 = 5, vf = -10 * |2 * (2/5 - 1)| * 100 = -1200
        let bars = [
            (2.0, 1.0, 2.0),
            (4.0, 2.0, 4.0),
            (4.0, 3.0, 4.0),
            (7.0, 4.0, 7.0),
            (5.0, 3.0, 5.0),
        ];

        for (high, low, close) in bars {
            kvo_34.update_raw(high, low, close, 10.0);
        }
        assert!(kvo_34.initialized());
        let expected = (1500.0 + 8000.0 / 7.0 - 1200.0) / 3.0
            - (2000.0 / 3.0 + 1500.0 + 8000.0 / 7.0 - 1200.0) / 4.0;
        assert!((kvo_34.value - expected).abs() < 1e-9);
        assert_eq!(kvo_34.trend, -1);
        assert_eq!(kvo_34.cm, 5.0);
    }

    #[rstest]
    fn test_flat_dm_carries_trend_forward(mut kvo_34: KlingerVolumeOscillator) {
        kvo_34.update_raw(1.0, 1.0, 1.0, 10.0);
        kvo_34.update_raw(2.0, 2.0, 2.0, 10.0);
        let trend_after_up = kvo_34.trend;
        kvo_34.update_raw(2.0, 2.0, 2.0, 10.0);
        assert_eq!(kvo_34.trend, trend_after_up);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(
        mut kvo_34: KlingerVolumeOscillator,
    ) {
        kvo_34.update_raw(1.00020, 1.00030, 1.00040, 1.00050);
        kvo_34.update_raw(1.00030, 1.00040, 1.00050, 1.00060);
        kvo_34.update_raw(1.00050, 1.00060, 1.00070, 1.00080);

        kvo_34.reset();

        assert!(!kvo_34.initialized());
        assert_eq!(kvo_34.value, 0.0);
        assert_eq!(kvo_34.trend, 0);
        assert_eq!(kvo_34.cm, 0.0);
        assert!(kvo_34.previous_hlc.is_none());
        assert_eq!(kvo_34.previous_dm, 0.0);
    }
}
