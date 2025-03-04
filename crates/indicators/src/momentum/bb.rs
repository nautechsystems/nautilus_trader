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

use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
};

use nautilus_model::data::{Bar, QuoteTick, TradeTick};

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
pub struct BollingerBands {
    pub period: usize,
    pub k: f64,
    pub ma_type: MovingAverageType,
    pub upper: f64,
    pub middle: f64,
    pub lower: f64,
    pub initialized: bool,
    ma: Box<dyn MovingAverage + Send + 'static>,
    prices: VecDeque<f64>,
    has_inputs: bool,
}

impl Display for BollingerBands {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{})",
            self.name(),
            self.period,
            self.k,
            self.ma_type,
        )
    }
}

impl Indicator for BollingerBands {
    fn name(&self) -> String {
        stringify!(BollingerBands).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) {
        let bid = quote.bid_price.raw as f64;
        let ask = quote.ask_price.raw as f64;
        let mid = f64::midpoint(bid, ask);
        self.update_raw(ask, bid, mid);
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        let price = trade.price.raw as f64;
        self.update_raw(price, price, price);
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into(), (&bar.close).into());
    }

    fn reset(&mut self) {
        self.ma.reset();
        self.prices.clear();
        self.upper = 0.0;
        self.middle = 0.0;
        self.lower = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl BollingerBands {
    /// Creates a new [`BollingerBands`] instance.
    #[must_use]
    pub fn new(period: usize, k: f64, ma_type: Option<MovingAverageType>) -> Self {
        Self {
            period,
            k,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            has_inputs: false,
            initialized: false,
            upper: 0.0,
            middle: 0.0,
            lower: 0.0,
            ma: MovingAverageFactory::create(ma_type.unwrap_or(MovingAverageType::Simple), period),
            prices: VecDeque::with_capacity(period),
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        let typical = (high + low + close) / 3.0;
        self.prices.push_back(typical);
        self.ma.update_raw(typical);

        // Initialization logic
        if !self.initialized {
            self.has_inputs = true;
            if self.prices.len() >= self.period {
                self.initialized = true;
            }
        }

        // Calculate values
        let std = fast_std_with_mean(self.prices.clone(), self.ma.value());

        self.upper = self.k.mul_add(std, self.ma.value());
        self.middle = self.ma.value();
        self.lower = self.k.mul_add(-std, self.ma.value());
    }
}

#[must_use]
pub fn fast_std_with_mean(values: VecDeque<f64>, mean: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let mut std_dev = 0.0;
    for v in &values {
        let diff = v - mean;
        std_dev += diff * diff;
    }

    (std_dev / values.len() as f64).sqrt()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::stubs::bb_10;

    #[rstest]
    fn test_name_returns_expected_string(bb_10: BollingerBands) {
        assert_eq!(bb_10.name(), "BollingerBands");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(bb_10: BollingerBands) {
        assert_eq!(format!("{bb_10}"), "BollingerBands(10,0.1,SIMPLE)");
    }

    #[rstest]
    fn test_period_returns_expected_value(bb_10: BollingerBands) {
        assert_eq!(bb_10.period, 10);
        assert_eq!(bb_10.k, 0.1);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(bb_10: BollingerBands) {
        assert!(!bb_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut bb_10: BollingerBands) {
        let high_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let low_values = [
            0.9, 1.9, 2.9, 3.9, 4.9, 5.9, 6.9, 7.9, 8.9, 9.9, 10.1, 10.2, 10.3, 11.1, 11.4,
        ];

        let close_values = [
            0.95, 1.95, 2.95, 3.95, 4.95, 5.95, 6.95, 7.95, 8.95, 9.95, 10.05, 10.15, 10.25, 11.05,
            11.45,
        ];

        for i in 0..15 {
            bb_10.update_raw(high_values[i], low_values[i], close_values[i]);
        }

        assert!(bb_10.initialized());
        assert_eq!(bb_10.upper, 10.108_266_446_984_462);
        assert_eq!(bb_10.middle, 9.676_666_666_666_666);
        assert_eq!(bb_10.lower, 9.245_066_886_348_87);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut bb_10: BollingerBands) {
        bb_10.update_raw(1.00020, 1.00050, 1.00030);
        bb_10.update_raw(1.00030, 1.00060, 1.00040);
        bb_10.update_raw(1.00070, 1.00080, 1.00075);

        bb_10.reset();

        assert!(!bb_10.initialized());
        assert_eq!(bb_10.upper, 0.0);
        assert_eq!(bb_10.middle, 0.0);
        assert_eq!(bb_10.lower, 0.0);
        assert_eq!(bb_10.prices.len(), 0);
    }
}
