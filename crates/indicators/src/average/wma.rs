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

use std::fmt::Display;

use nautilus_core::correctness::{FAILED, check_predicate_true};
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::indicator::{Indicator, MovingAverage};

/// An indicator which calculates a weighted moving average across a rolling window.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct WeightedMovingAverage {
    /// The rolling window period for the indicator (> 0).
    pub period: usize,
    /// The weights for the moving average calculation
    pub weights: Vec<f64>,
    /// Price type
    pub price_type: PriceType,
    /// The last indicator value.
    pub value: f64,
    /// Whether the indicator is initialized.
    pub initialized: bool,
    /// Inputs
    pub inputs: Vec<f64>,
    has_inputs: bool,
}

impl Display for WeightedMovingAverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{:?})", self.name(), self.period, self.weights)
    }
}

impl WeightedMovingAverage {
    /// Creates a new [`WeightedMovingAverage`] instance.
    #[must_use]
    pub fn new(period: usize, weights: Vec<f64>, price_type: Option<PriceType>) -> Self {
        Self::new_checked(period, weights, price_type).expect(FAILED)
    }

    pub fn new_checked(
        period: usize,
        weights: Vec<f64>,
        price_type: Option<PriceType>,
    ) -> anyhow::Result<Self> {
        check_predicate_true(
            period == weights.len(),
            "`period` must be equal to `weights` length",
        )?;

        Ok(Self {
            period,
            weights,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            inputs: Vec::with_capacity(period),
            initialized: false,
            has_inputs: false,
        })
    }

    fn weighted_average(&self) -> f64 {
        let mut sum = 0.0;
        let mut weight_sum = 0.0;
        let reverse_weights: Vec<f64> = self.weights.iter().copied().rev().collect();
        for (index, input) in self.inputs.iter().rev().enumerate() {
            let weight = reverse_weights.get(index).unwrap();
            sum += input * weight;
            weight_sum += weight;
        }
        sum / weight_sum
    }
}

impl Indicator for WeightedMovingAverage {
    fn name(&self) -> String {
        stringify!(WeightedMovingAverage).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }
    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) {
        self.update_raw(quote.extract_price(self.price_type).into());
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.update_raw((&trade.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
        self.inputs.clear();
    }
}

impl MovingAverage for WeightedMovingAverage {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.inputs.len()
    }
    fn update_raw(&mut self, value: f64) {
        if !self.has_inputs {
            self.has_inputs = true;
            self.inputs.push(value);
            self.value = value;
            return;
        }
        if self.inputs.len() == self.period {
            self.inputs.remove(0);
        }
        self.inputs.push(value);
        self.value = self.weighted_average();
        if !self.initialized && self.count() >= self.period {
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

    use crate::{
        average::wma::WeightedMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn test_wma_initialized(indicator_wma_10: WeightedMovingAverage) {
        let display_str = format!("{indicator_wma_10}");
        assert_eq!(
            display_str,
            "WeightedMovingAverage(10,[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0])"
        );
        assert_eq!(indicator_wma_10.name(), "WeightedMovingAverage");
        assert!(!indicator_wma_10.has_inputs());
        assert!(!indicator_wma_10.initialized());
    }

    #[rstest]
    #[should_panic]
    fn test_different_weights_len_and_period_error() {
        let _ = WeightedMovingAverage::new(10, vec![0.5, 0.5, 0.5], None);
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_wma_10: WeightedMovingAverage) {
        indicator_wma_10.update_raw(1.0);
        assert_eq!(indicator_wma_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_two_inputs_equal_weights() {
        let mut wma = WeightedMovingAverage::new(2, vec![0.5, 0.5], None);
        wma.update_raw(1.0);
        wma.update_raw(2.0);
        assert_eq!(wma.value, 1.5);
    }

    #[rstest]
    fn test_value_with_four_inputs_equal_weights() {
        let mut wma = WeightedMovingAverage::new(4, vec![0.25, 0.25, 0.25, 0.25], None);
        wma.update_raw(1.0);
        wma.update_raw(2.0);
        wma.update_raw(3.0);
        wma.update_raw(4.0);
        assert_eq!(wma.value, 2.5);
    }

    #[rstest]
    fn test_value_with_two_inputs(mut indicator_wma_10: WeightedMovingAverage) {
        indicator_wma_10.update_raw(1.0);
        indicator_wma_10.update_raw(2.0);
        let result = 2.0f64.mul_add(1.0, 1.0 * 0.9) / 1.9;
        assert_eq!(indicator_wma_10.value, result);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut indicator_wma_10: WeightedMovingAverage) {
        indicator_wma_10.update_raw(1.0);
        indicator_wma_10.update_raw(2.0);
        indicator_wma_10.update_raw(3.0);
        let result = 1.0f64.mul_add(0.8, 3.0f64.mul_add(1.0, 2.0 * 0.9)) / (1.0 + 0.9 + 0.8);
        assert_eq!(indicator_wma_10.value, result);
    }

    #[rstest]
    fn test_value_expected_with_exact_period(mut indicator_wma_10: WeightedMovingAverage) {
        for i in 1..11 {
            indicator_wma_10.update_raw(f64::from(i));
        }
        assert_eq!(indicator_wma_10.value, 7.0);
    }

    #[rstest]
    fn test_value_expected_with_more_inputs(mut indicator_wma_10: WeightedMovingAverage) {
        for i in 1..=11 {
            indicator_wma_10.update_raw(f64::from(i));
        }
        assert_eq!(indicator_wma_10.value(), 8.000_000_000_000_002);
    }

    #[rstest]
    fn test_reset(mut indicator_wma_10: WeightedMovingAverage) {
        indicator_wma_10.update_raw(1.0);
        indicator_wma_10.update_raw(2.0);
        indicator_wma_10.reset();
        assert_eq!(indicator_wma_10.value, 0.0);
        assert_eq!(indicator_wma_10.count(), 0);
        assert!(!indicator_wma_10.has_inputs);
        assert!(!indicator_wma_10.initialized);
    }
}
