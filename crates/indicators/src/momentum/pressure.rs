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
    volatility::atr::AverageTrueRange,
};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct Pressure {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub atr_floor: f64,
    pub value: f64,
    pub value_cumulative: f64,
    pub initialized: bool,
    atr: AverageTrueRange,
    average_volume: Box<dyn MovingAverage + Send + 'static>,
    has_inputs: bool,
}

impl Display for Pressure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{})", self.name(), self.period, self.ma_type,)
    }
}

impl Indicator for Pressure {
    fn name(&self) -> String {
        stringify!(Pressure).to_string()
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
        self.atr.reset();
        self.average_volume.reset();
        self.value = 0.0;
        self.value_cumulative = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl Pressure {
    /// Creates a new [`Pressure`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is not positive (> 0).
    #[must_use]
    pub fn new(period: usize, ma_type: Option<MovingAverageType>, atr_floor: Option<f64>) -> Self {
        assert!(period > 0, "Pressure: period must be > 0");
        let ma_type = ma_type.unwrap_or(MovingAverageType::Exponential);
        Self {
            period,
            ma_type,
            atr_floor: atr_floor.unwrap_or(0.0),
            value: 0.0,
            value_cumulative: 0.0,
            atr: AverageTrueRange::new(period, Some(ma_type), Some(false), atr_floor),
            average_volume: MovingAverageFactory::create(ma_type, period),
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64, volume: f64) {
        self.atr.update_raw(high, low, close);
        self.average_volume.update_raw(volume);

        self.has_inputs = true;

        let avg_vol = self.average_volume.value();
        if avg_vol == 0.0 {
            self.value = 0.0;
            return;
        }

        let atr_val = if self.atr.value > 0.0 {
            self.atr.value
        } else {
            (high - low).abs().max(self.atr_floor)
        };

        if atr_val == 0.0 {
            self.value = 0.0;
            return;
        }

        let relative_volume = volume / avg_vol;
        let buy_pressure = ((close - low) / atr_val) * relative_volume;
        let sell_pressure = ((high - close) / atr_val) * relative_volume;

        self.value = buy_pressure - sell_pressure;
        self.value_cumulative += self.value;

        if self.atr.initialized && self.average_volume.initialized() && !self.initialized {
            self.initialized = true;
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
    use crate::stubs::{bar_ethusdt_binance_minute_bid, pressure_10};

    #[rstest]
    fn test_name_returns_expected_string(pressure_10: Pressure) {
        assert_eq!(pressure_10.name(), "Pressure");
    }

    #[test]
    fn test_str_repr_returns_expected_string() {
        let pressure = Pressure::new(10, Some(MovingAverageType::Exponential), None);
        assert_eq!(format!("{pressure}"), "Pressure(10,EXPONENTIAL)");
    }

    #[rstest]
    fn test_period_returns_expected_value(pressure_10: Pressure) {
        assert_eq!(pressure_10.period, 10);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(pressure_10: Pressure) {
        assert!(!pressure_10.initialized());
    }

    #[test]
    fn test_value_with_all_higher_inputs_returns_expected_value() {
        let mut pressure = Pressure::new(10, Some(MovingAverageType::Exponential), None);

        let high_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let low_values = [
            0.9, 1.9, 2.9, 3.9, 4.9, 5.9, 6.9, 7.9, 8.9, 9.9, 10.1, 10.2, 10.3, 11.1, 11.4,
        ];
        let close_values = [
            1.1, 2.1, 3.1, 4.1, 5.1, 6.1, 7.1, 8.1, 9.1, 10.1, 11.1, 12.1, 13.1, 14.1, 15.1,
        ];
        let volume_values = [
            100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0, 1100.0, 1200.0,
            1300.0, 1400.0, 1500.0,
        ];

        let mut expected_cumulative = 0.0;
        let mut expected_last = 0.0;

        for i in 0..15 {
            pressure.update_raw(
                high_values[i],
                low_values[i],
                close_values[i],
                volume_values[i],
            );

            let atr_val = if pressure.atr.value > 0.0 {
                pressure.atr.value
            } else {
                (high_values[i] - low_values[i])
                    .abs()
                    .max(pressure.atr_floor)
            };
            let avg_vol = pressure.average_volume.value();
            if avg_vol != 0.0 && atr_val != 0.0 {
                let relative_volume = volume_values[i] / avg_vol;
                let buy_pressure = ((close_values[i] - low_values[i]) / atr_val) * relative_volume;
                let sell_pressure =
                    ((high_values[i] - close_values[i]) / atr_val) * relative_volume;
                let bar_value = buy_pressure - sell_pressure;
                expected_cumulative += bar_value;
                expected_last = bar_value;
            }
        }

        assert!(pressure.initialized());
        assert!((pressure.value - expected_last).abs() < 1e-6);
        assert!((pressure.value_cumulative - expected_cumulative).abs() < 1e-6);
    }

    #[rstest]
    fn test_handle_bar(mut pressure_10: Pressure, bar_ethusdt_binance_minute_bid: Bar) {
        pressure_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(pressure_10.value, -0.018_181_818_181_818_132);
        assert_eq!(pressure_10.value_cumulative, -0.018_181_818_181_818_132);
        assert!(pressure_10.has_inputs);
        assert!(!pressure_10.initialized);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut pressure_10: Pressure) {
        pressure_10.update_raw(1.00020, 1.00050, 1.00070, 100.0);
        pressure_10.update_raw(1.00030, 1.00060, 1.00080, 200.0);
        pressure_10.update_raw(1.00070, 1.00080, 1.00090, 300.0);

        pressure_10.reset();

        assert!(!pressure_10.initialized());
        assert_eq!(pressure_10.value, 0.0);
        assert_eq!(pressure_10.value_cumulative, 0.0);
        assert!(!pressure_10.has_inputs);
    }

    #[test]
    fn test_ma_type_default_and_override() {
        let pressure_default = Pressure::new(10, None, None);
        assert_eq!(pressure_default.ma_type, MovingAverageType::Exponential);

        let pressure_simple = Pressure::new(10, Some(MovingAverageType::Simple), None);
        assert_eq!(pressure_simple.ma_type, MovingAverageType::Simple);
    }

    #[test]
    fn test_initialized_after_enough_inputs() {
        let mut pressure = Pressure::new(3, Some(MovingAverageType::Exponential), None);
        for _ in 0..3 {
            pressure.update_raw(1.3, 1.0, 1.1, 100.0);
        }
        assert!(pressure.initialized());
    }

    #[test]
    fn test_atr_floor_applied_to_zero_range() {
        let mut pressure = Pressure::new(1, Some(MovingAverageType::Simple), Some(0.5));
        pressure.update_raw(1.5, 1.0, 1.2, 100.0);
        assert!((pressure.value + 0.2).abs() < 1e-6);
        assert!(!pressure.value_cumulative.is_nan());
    }
}
