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

use nautilus_model::data::Bar;

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct KlingerVolumeOscillator {
    pub fast_period: usize,
    pub slow_period: usize,
    pub signal_period: usize,
    pub ma_type: MovingAverageType,
    pub value: f64,
    pub initialized: bool,
    fast_ma: Box<dyn MovingAverage + Send + 'static>,
    slow_ma: Box<dyn MovingAverage + Send + 'static>,
    signal_ma: Box<dyn MovingAverage + Send + 'static>,
    has_inputs: bool,
    hlc3: f64,
    previous_hlc3: f64,
}

impl Display for KlingerVolumeOscillator {
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
        self.hlc3 = 0.0;
        self.previous_hlc3 = 0.0;
        self.fast_ma.reset();
        self.slow_ma.reset();
        self.signal_ma.reset();
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl KlingerVolumeOscillator {
    /// Creates a new [`KlingerVolumeOscillator`] instance.
    #[must_use]
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        signal_period: usize,
        ma_type: Option<MovingAverageType>,
    ) -> Self {
        Self {
            fast_period,
            slow_period,
            signal_period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            value: 0.0,
            fast_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Simple),
                fast_period,
            ),
            slow_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Simple),
                slow_period,
            ),
            signal_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Simple),
                signal_period,
            ),
            has_inputs: false,
            hlc3: 0.0,
            previous_hlc3: 0.0,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64, volume: f64) {
        self.hlc3 = (high + low + close) / 3.0;
        if self.hlc3 > self.previous_hlc3 {
            self.fast_ma.update_raw(volume);
            self.slow_ma.update_raw(volume);
        } else if self.hlc3 < self.previous_hlc3 {
            self.fast_ma.update_raw(-volume);
            self.slow_ma.update_raw(-volume);
        } else {
            self.fast_ma.update_raw(0.0);
            self.slow_ma.update_raw(0.0);
        }

        if self.slow_ma.initialized() {
            self.signal_ma
                .update_raw(self.fast_ma.value() - self.slow_ma.value());
            self.value = self.signal_ma.value();
        }

        // initialization logic
        if !self.initialized {
            self.has_inputs = true;
            if self.signal_ma.initialized() {
                self.initialized = true;
            }
        }

        self.previous_hlc3 = self.hlc3;
    }

    pub fn _check_initialized(&mut self) {
        if !self.initialized {
            self.has_inputs = true;
            if self.signal_ma.initialized() {
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
    use crate::stubs::kvo_345;

    #[rstest]
    fn test_name_returns_expected_string(kvo_345: KlingerVolumeOscillator) {
        assert_eq!(kvo_345.name(), "KlingerVolumeOscillator");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(kvo_345: KlingerVolumeOscillator) {
        assert_eq!(
            format!("{kvo_345}"),
            "KlingerVolumeOscillator(3,4,5,SIMPLE)"
        );
    }

    #[rstest]
    fn test_period_returns_expected_value(kvo_345: KlingerVolumeOscillator) {
        assert_eq!(kvo_345.fast_period, 3);
        assert_eq!(kvo_345.slow_period, 4);
        assert_eq!(kvo_345.signal_period, 5);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(kvo_345: KlingerVolumeOscillator) {
        assert!(!kvo_345.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(
        mut kvo_345: KlingerVolumeOscillator,
    ) {
        let high_values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let low_values = [0.9, 1.9, 2.9, 3.9, 4.9, 5.9, 6.9, 7.9, 8.9, 9.9];
        let close_values = [1.1, 2.1, 3.1, 4.1, 5.1, 6.1, 7.1, 8.1, 9.1, 10.1];
        let volume_values = [
            100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0,
        ];

        for i in 0..10 {
            kvo_345.update_raw(
                high_values[i],
                low_values[i],
                close_values[i],
                volume_values[i],
            );
        }

        assert!(kvo_345.initialized());
        assert_eq!(kvo_345.value, 50.0);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(
        mut kvo_345: KlingerVolumeOscillator,
    ) {
        kvo_345.update_raw(1.00020, 1.00030, 1.00040, 1.00050);
        kvo_345.update_raw(1.00030, 1.00040, 1.00050, 1.00060);
        kvo_345.update_raw(1.00050, 1.00060, 1.00070, 1.00080);

        kvo_345.reset();

        assert!(!kvo_345.initialized());
        assert_eq!(kvo_345.value, 0.0);
        assert_eq!(kvo_345.hlc3, 0.0);
        assert_eq!(kvo_345.previous_hlc3, 0.0);
    }
}
