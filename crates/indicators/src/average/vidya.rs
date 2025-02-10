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

use std::fmt::{Display, Formatter};

use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::{
    average::MovingAverageType,
    indicator::{Indicator, MovingAverage},
    momentum::cmo::ChandeMomentumOscillator,
};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct VariableIndexDynamicAverage {
    pub period: usize,
    pub alpha: f64,
    pub price_type: PriceType,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    pub cmo: ChandeMomentumOscillator,
    pub cmo_pct: f64,
}

impl Display for VariableIndexDynamicAverage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for VariableIndexDynamicAverage {
    fn name(&self) -> String {
        stringify!(VariableIndexDynamicAverage).to_string()
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
        self.count = 0;
        self.cmo_pct = 0.0;
        self.alpha = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl VariableIndexDynamicAverage {
    /// Creates a new [`VariableIndexDynamicAverage`] instance.
    #[must_use]
    pub fn new(
        period: usize,
        price_type: Option<PriceType>,
        cmo_ma_type: Option<MovingAverageType>,
    ) -> Self {
        Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            count: 0,
            has_inputs: false,
            initialized: false,
            alpha: 2.0 / (period as f64 + 1.0),
            cmo: ChandeMomentumOscillator::new(period, cmo_ma_type),
            cmo_pct: 0.0,
        }
    }
}

impl MovingAverage for VariableIndexDynamicAverage {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.count
    }

    fn update_raw(&mut self, value: f64) {
        self.cmo.update_raw(value);
        self.cmo_pct = (self.cmo.value / 100.0).abs();

        if self.initialized {
            self.value = (self.alpha * self.cmo_pct)
                .mul_add(value, self.alpha.mul_add(-self.cmo_pct, 1.0) * self.value);
        }

        if !self.initialized && self.cmo.initialized {
            self.initialized = true;
        }

        self.count += 1;
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::{Bar, QuoteTick, TradeTick};
    use rstest::rstest;

    use crate::{
        average::vidya::VariableIndexDynamicAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn test_vidya_initialized(indicator_vidya_10: VariableIndexDynamicAverage) {
        let display_st = format!("{indicator_vidya_10}");
        assert_eq!(display_st, "VariableIndexDynamicAverage(10)");
        assert_eq!(indicator_vidya_10.period, 10);
        assert!(!indicator_vidya_10.initialized());
        assert!(!indicator_vidya_10.has_inputs());
    }

    #[rstest]
    fn test_initialized_with_required_input(mut indicator_vidya_10: VariableIndexDynamicAverage) {
        for i in 1..10 {
            indicator_vidya_10.update_raw(f64::from(i));
        }
        assert!(!indicator_vidya_10.initialized);
        indicator_vidya_10.update_raw(10.0);
        assert!(indicator_vidya_10.initialized);
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_vidya_10: VariableIndexDynamicAverage) {
        indicator_vidya_10.update_raw(1.0);
        assert_eq!(indicator_vidya_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut indicator_vidya_10: VariableIndexDynamicAverage) {
        indicator_vidya_10.update_raw(1.0);
        indicator_vidya_10.update_raw(2.0);
        indicator_vidya_10.update_raw(3.0);
        assert_eq!(indicator_vidya_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut indicator_vidya_10: VariableIndexDynamicAverage) {
        indicator_vidya_10.update_raw(1.00000);
        indicator_vidya_10.update_raw(1.00010);
        indicator_vidya_10.update_raw(1.00020);
        indicator_vidya_10.update_raw(1.00030);
        indicator_vidya_10.update_raw(1.00040);
        indicator_vidya_10.update_raw(1.00050);
        indicator_vidya_10.update_raw(1.00040);
        indicator_vidya_10.update_raw(1.00030);
        indicator_vidya_10.update_raw(1.00020);
        indicator_vidya_10.update_raw(1.00010);
        indicator_vidya_10.update_raw(1.00000);
        assert_eq!(indicator_vidya_10.value, 0.046_813_474_863_949_87);
    }

    #[rstest]
    fn test_handle_quote_tick(
        mut indicator_vidya_10: VariableIndexDynamicAverage,
        stub_quote: QuoteTick,
    ) {
        indicator_vidya_10.handle_quote(&stub_quote);
        assert_eq!(indicator_vidya_10.value, 0.0);
    }

    #[rstest]
    fn test_handle_trade_tick(
        mut indicator_vidya_10: VariableIndexDynamicAverage,
        stub_trade: TradeTick,
    ) {
        indicator_vidya_10.handle_trade(&stub_trade);
        assert_eq!(indicator_vidya_10.value, 0.0);
    }

    #[rstest]
    fn test_handle_bar(
        mut indicator_vidya_10: VariableIndexDynamicAverage,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_vidya_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(indicator_vidya_10.value, 0.0);
        assert!(!indicator_vidya_10.initialized);
    }

    #[rstest]
    fn test_reset(mut indicator_vidya_10: VariableIndexDynamicAverage) {
        indicator_vidya_10.update_raw(1.0);
        assert_eq!(indicator_vidya_10.count, 1);
        assert_eq!(indicator_vidya_10.value, 0.0);
        indicator_vidya_10.reset();
        assert_eq!(indicator_vidya_10.value, 0.0);
        assert_eq!(indicator_vidya_10.count, 0);
        assert!(!indicator_vidya_10.has_inputs);
        assert!(!indicator_vidya_10.initialized);
    }
}
