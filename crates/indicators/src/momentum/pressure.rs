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
    #[must_use]
    pub fn new(period: usize, ma_type: Option<MovingAverageType>, atr_floor: Option<f64>) -> Self {
        Self {
            period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            atr_floor: atr_floor.unwrap_or(0.0),
            value: 0.0,
            value_cumulative: 0.0,
            atr: AverageTrueRange::new(
                period,
                Some(MovingAverageType::Exponential),
                Some(false),
                atr_floor,
            ),
            average_volume: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Simple),
                period,
            ),
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64, volume: f64) {
        self.atr.update_raw(high, low, close);
        self.average_volume.update_raw(volume);

        if !self.initialized {
            self.has_inputs = true;
            if self.atr.initialized {
                self.initialized = true;
            }
        }

        if self.average_volume.value() == 0.0 || self.atr.value == 0.0 {
            self.value = 0.0;
            return;
        }

        let relative_volume = volume / self.average_volume.value();
        let buy_pressure = ((close - low) / self.atr.value) * relative_volume;
        let sell_pressure = ((high - close) / self.atr.value) * relative_volume;

        self.value = buy_pressure - sell_pressure;
        self.value_cumulative += self.value;
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

    #[rstest]
    fn test_str_repr_returns_expected_string(pressure_10: Pressure) {
        assert_eq!(format!("{pressure_10}"), "Pressure(10,SIMPLE)");
    }

    #[rstest]
    fn test_period_returns_expected_value(pressure_10: Pressure) {
        assert_eq!(pressure_10.period, 10);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(pressure_10: Pressure) {
        assert!(!pressure_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut pressure_10: Pressure) {
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

        for i in 0..15 {
            pressure_10.update_raw(
                high_values[i],
                low_values[i],
                close_values[i],
                volume_values[i],
            );
        }

        assert!(pressure_10.initialized());
        assert_eq!(pressure_10.value, 4.377_880_184_331_797);
        assert_eq!(pressure_10.value_cumulative, 23.231_207_409_222_474);
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
}
