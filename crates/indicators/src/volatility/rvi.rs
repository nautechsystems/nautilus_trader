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

/// An indicator which calculates a Relative Volatility Index (RVI) across a rolling window.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct RelativeVolatilityIndex {
    pub period: usize,
    pub scalar: f64,
    pub ma_type: MovingAverageType,
    pub value: f64,
    pub initialized: bool,
    prices: ArrayDeque<f64, 1024, Wrapping>,
    ma: Box<dyn MovingAverage + Send + 'static>,
    pos_ma: Box<dyn MovingAverage + Send + 'static>,
    neg_ma: Box<dyn MovingAverage + Send + 'static>,
    previous_close: f64,
    std: f64,
    has_inputs: bool,
}

impl Display for RelativeVolatilityIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{})",
            self.name(),
            self.period,
            self.scalar,
            self.ma_type,
        )
    }
}

impl Indicator for RelativeVolatilityIndex {
    fn name(&self) -> String {
        stringify!(RelativeVolatilityIndex).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.previous_close = 0.0;
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
        self.std = 0.0;
        self.prices.clear();
        self.ma.reset();
        self.pos_ma.reset();
        self.neg_ma.reset();
    }
}

impl RelativeVolatilityIndex {
    /// Creates a new [`RelativeVolatilityIndex`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `period` is not in the range of 1 to 1024 (inclusive).
    /// - `scalar` is not in the range of 0.0 to 100.0 (inclusive).
    /// - `ma_type` is not a valid [`MovingAverageType`].
    #[must_use]
    pub fn new(period: usize, scalar: Option<f64>, ma_type: Option<MovingAverageType>) -> Self {
        assert!(
            period <= 1024,
            "period {period} exceeds maximum capacity of price deque"
        );

        Self {
            period,
            scalar: scalar.unwrap_or(100.0),
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            value: 0.0,
            initialized: false,
            prices: ArrayDeque::new(),
            ma: MovingAverageFactory::create(ma_type.unwrap_or(MovingAverageType::Simple), period),
            pos_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Simple),
                period,
            ),
            neg_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Simple),
                period,
            ),
            previous_close: 0.0,
            std: 0.0,
            has_inputs: false,
        }
    }

    pub fn update_raw(&mut self, close: f64) {
        self.prices.push_back(close);
        self.ma.update_raw(close);

        if self.prices.is_empty() {
            self.std = 0.0;
        } else {
            let mean = self.ma.value();
            let mut var_sum = 0.0;
            for &price in &self.prices {
                let diff = price - mean;
                var_sum += diff * diff;
            }
            self.std = (var_sum / self.prices.len() as f64).sqrt();
            self.std = self.std * (self.period as f64).sqrt() / ((self.period - 1) as f64).sqrt();
        }

        if self.ma.initialized() {
            if close > self.previous_close {
                self.pos_ma.update_raw(self.std);
                self.neg_ma.update_raw(0.0);
            } else if close < self.previous_close {
                self.pos_ma.update_raw(0.0);
                self.neg_ma.update_raw(self.std);
            } else {
                self.pos_ma.update_raw(0.0);
                self.neg_ma.update_raw(0.0);
            }

            self.value = self.scalar * self.pos_ma.value();
            self.value /= self.pos_ma.value() + self.neg_ma.value();
        }

        self.previous_close = close;

        if !self.initialized {
            self.has_inputs = true;
            if self.pos_ma.initialized() {
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
    use crate::stubs::rvi_10;

    #[rstest]
    fn test_name_returns_expected_string(rvi_10: RelativeVolatilityIndex) {
        assert_eq!(rvi_10.name(), "RelativeVolatilityIndex");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(rvi_10: RelativeVolatilityIndex) {
        assert_eq!(format!("{rvi_10}"), "RelativeVolatilityIndex(10,10,SIMPLE)");
    }

    #[rstest]
    fn test_period_returns_expected_value(rvi_10: RelativeVolatilityIndex) {
        assert_eq!(rvi_10.period, 10);
        assert_eq!(rvi_10.scalar, 10.0);
        assert_eq!(rvi_10.ma_type, MovingAverageType::Simple);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(rvi_10: RelativeVolatilityIndex) {
        assert!(!rvi_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(
        mut rvi_10: RelativeVolatilityIndex,
    ) {
        let close_values = [
            105.25, 107.50, 109.75, 112.00, 114.25, 116.50, 118.75, 121.00, 123.25, 125.50, 127.75,
            130.00, 132.25, 134.50, 136.75, 139.00, 141.25, 143.50, 145.75, 148.00, 150.25, 152.50,
            154.75, 157.00, 159.25, 161.50, 163.75, 166.00, 168.25, 170.50,
        ];

        for close in close_values {
            rvi_10.update_raw(close);
        }

        assert!(rvi_10.initialized());
        assert_eq!(rvi_10.value, 10.0);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(
        mut rvi_10: RelativeVolatilityIndex,
    ) {
        rvi_10.update_raw(1.00020);
        rvi_10.update_raw(1.00030);
        rvi_10.update_raw(1.00070);

        rvi_10.reset();

        assert!(!rvi_10.initialized());
        assert_eq!(rvi_10.value, 0.0);
        assert!(!rvi_10.initialized);
        assert!(!rvi_10.has_inputs);
        assert_eq!(rvi_10.std, 0.0);
        assert_eq!(rvi_10.prices.len(), 0);
        assert_eq!(rvi_10.ma.value(), 0.0);
        assert_eq!(rvi_10.pos_ma.value(), 0.0);
        assert_eq!(rvi_10.neg_ma.value(), 0.0);
    }
}
