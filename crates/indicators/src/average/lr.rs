// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//   You may not use this file except in compliance with the License.
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

use crate::indicator::Indicator;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct LinearRegression {
    pub period: usize,
    pub slope: f64,
    pub intercept: f64,
    pub degree: f64,
    pub cfo: f64,
    pub r2: f64,
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    inputs: Vec<f64>,
}

impl Display for LinearRegression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period,)
    }
}

impl Indicator for LinearRegression {
    fn name(&self) -> String {
        stringify!(LinearRegression).to_string()
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
        self.slope = 0.0;
        self.intercept = 0.0;
        self.degree = 0.0;
        self.cfo = 0.0;
        self.r2 = 0.0;
        self.inputs.clear();
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl LinearRegression {
    /// Creates a new [`LinearRegression`] instance.
    #[must_use]
    pub fn new(period: usize) -> Self {
        Self {
            period,
            slope: 0.0,
            intercept: 0.0,
            degree: 0.0,
            cfo: 0.0,
            r2: 0.0,
            value: 0.0,
            inputs: Vec::with_capacity(period),
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, close: f64) {
        self.inputs.push(close);

        if !self.initialized {
            self.has_inputs = true;
            if self.inputs.len() >= self.period {
                self.initialized = true;
            } else {
                return;
            }
        }

        // let x_arr
        let x_arr: Vec<f64> = (1..=self.period).map(|x| x as f64).collect();
        let y_arr: Vec<f64> = self.inputs.clone();
        let x_sum: f64 = 0.5 * self.period as f64 * (self.period as f64 + 1.0);
        let x_mul_sum: f64 = x_sum * 2.0f64.mul_add(self.period as f64, 1.0) / 3.0;
        let divisor: f64 = (self.period as f64).mul_add(x_mul_sum, -(x_sum * x_sum));
        let y_sum: f64 = y_arr.iter().sum::<f64>();
        let sum_x_y: f64 = x_arr
            .iter()
            .zip(y_arr.iter())
            .map(|(x, y)| x * y)
            .sum::<f64>();

        self.slope = (self.period as f64).mul_add(sum_x_y, -(x_sum * y_sum)) / divisor;
        self.intercept = y_sum.mul_add(x_mul_sum, -(x_sum * sum_x_y)) / divisor;

        let residuals: Vec<f64> = x_arr
            .into_iter()
            .zip(y_arr.clone())
            .map(|(x, y)| self.slope.mul_add(x, self.intercept) - y)
            .collect();

        self.value = residuals.last().unwrap() + y_arr.last().unwrap();
        self.degree = 180.0 / std::f64::consts::PI * self.slope.atan();
        self.cfo = 100.0 * residuals.last().unwrap() / y_arr.last().unwrap();
        let mean: f64 = y_arr.iter().sum::<f64>() / y_arr.len() as f64;
        self.r2 = 1.0
            - residuals.iter().map(|r| r * r).sum::<f64>()
                / y_arr.iter().map(|y| (y - mean) * (y - mean)).sum::<f64>();
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::rstest;

    use crate::{
        average::lr::LinearRegression,
        indicator::Indicator,
        stubs::{bar_ethusdt_binance_minute_bid, indicator_lr_10},
    };

    #[rstest]
    fn test_psl_initialized(indicator_lr_10: LinearRegression) {
        let display_str = format!("{indicator_lr_10}");
        assert_eq!(display_str, "LinearRegression(10)");
        assert_eq!(indicator_lr_10.period, 10);
        assert!(!indicator_lr_10.initialized);
        assert!(!indicator_lr_10.has_inputs);
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_lr_10: LinearRegression) {
        indicator_lr_10.update_raw(1.0);
        assert_eq!(indicator_lr_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut indicator_lr_10: LinearRegression) {
        indicator_lr_10.update_raw(1.0);
        indicator_lr_10.update_raw(2.0);
        indicator_lr_10.update_raw(3.0);
        assert_eq!(indicator_lr_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut indicator_lr_10: LinearRegression) {
        indicator_lr_10.update_raw(1.00000);
        indicator_lr_10.update_raw(1.00010);
        indicator_lr_10.update_raw(1.00030);
        indicator_lr_10.update_raw(1.00040);
        indicator_lr_10.update_raw(1.00050);
        indicator_lr_10.update_raw(1.00060);
        indicator_lr_10.update_raw(1.00050);
        indicator_lr_10.update_raw(1.00040);
        indicator_lr_10.update_raw(1.00030);
        indicator_lr_10.update_raw(1.00010);
        indicator_lr_10.update_raw(1.00000);
        assert_eq!(indicator_lr_10.value, 0.800_307_272_727_272_2);
    }

    #[rstest]
    fn test_initialized_with_required_input(mut indicator_lr_10: LinearRegression) {
        for i in 1..10 {
            indicator_lr_10.update_raw(f64::from(i));
        }
        assert!(!indicator_lr_10.initialized);
        indicator_lr_10.update_raw(10.0);
        assert!(indicator_lr_10.initialized);
    }

    #[rstest]
    fn test_handle_bar(mut indicator_lr_10: LinearRegression, bar_ethusdt_binance_minute_bid: Bar) {
        indicator_lr_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(indicator_lr_10.value, 0.0);
        assert!(indicator_lr_10.has_inputs);
        assert!(!indicator_lr_10.initialized);
    }

    #[rstest]
    fn test_reset(mut indicator_lr_10: LinearRegression) {
        indicator_lr_10.update_raw(1.0);
        indicator_lr_10.reset();
        assert_eq!(indicator_lr_10.value, 0.0);
        assert_eq!(indicator_lr_10.inputs.len(), 0);
        assert_eq!(indicator_lr_10.slope, 0.0);
        assert_eq!(indicator_lr_10.intercept, 0.0);
        assert_eq!(indicator_lr_10.degree, 0.0);
        assert_eq!(indicator_lr_10.cfo, 0.0);
        assert_eq!(indicator_lr_10.r2, 0.0);
        assert!(!indicator_lr_10.has_inputs);
        assert!(!indicator_lr_10.initialized);
    }
}
