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
    pub cmo: ChandeMomentumOscillator,
    pub cmo_pct: f64,
    has_inputs: bool,
}

impl Display for VariableIndexDynamicAverage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for VariableIndexDynamicAverage {
    fn name(&self) -> String {
        stringify!(VariableIndexDynamicAverage).into()
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
        self.alpha = 2.0 / (self.period as f64 + 1.0);
        self.has_inputs = false;
        self.initialized = false;
        self.cmo.reset();
    }
}

impl VariableIndexDynamicAverage {
    /// Creates a new [`VariableIndexDynamicAverage`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is not positive (> 0).
    #[must_use]
    pub fn new(
        period: usize,
        price_type: Option<PriceType>,
        cmo_ma_type: Option<MovingAverageType>,
    ) -> Self {
        assert!(
            period > 0,
            "VariableIndexDynamicAverage: period must be > 0 (received {period})"
        );

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

    fn update_raw(&mut self, price: f64) {
        self.cmo.update_raw(price);
        self.cmo_pct = (self.cmo.value / 100.0).abs();

        if self.initialized {
            self.value = (self.alpha * self.cmo_pct)
                .mul_add(price, self.alpha.mul_add(-self.cmo_pct, 1.0) * self.value);
        }

        if !self.initialized && self.cmo.initialized {
            self.initialized = true;
        }
        self.has_inputs = true;
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
        average::{sma::SimpleMovingAverage, vidya::VariableIndexDynamicAverage},
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
    #[should_panic(expected = "period must be > 0")]
    fn sma_new_with_zero_period_panics() {
        let _ = VariableIndexDynamicAverage::new(0, None, None);
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

    fn reference_ma(prices: &[f64], period: usize) -> Vec<f64> {
        let mut buf = Vec::with_capacity(period);
        prices
            .iter()
            .map(|&p| {
                buf.push(p);
                if buf.len() > period {
                    buf.remove(0);
                }
                buf.iter().copied().sum::<f64>() / buf.len() as f64
            })
            .collect()
    }

    #[rstest]
    #[case(3, vec![1.0, 2.0, 3.0, 4.0, 5.0])]
    #[case(4, vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0])]
    #[case(2, vec![0.1, 0.2, 0.3, 0.4])]
    fn test_sma_exact_rolling_mean(#[case] period: usize, #[case] prices: Vec<f64>) {
        let mut sma = SimpleMovingAverage::new(period, None);
        let expected = reference_ma(&prices, period);

        for (ix, (&price, &exp)) in prices.iter().zip(expected.iter()).enumerate() {
            sma.update_raw(price);
            assert_eq!(sma.count(), std::cmp::min(ix + 1, period));

            let got = sma.value();
            assert!(
                (got - exp).abs() < 1e-12,
                "tick {ix}: expected {exp}, was {got}"
            );
        }
    }

    #[rstest]
    fn test_sma_matches_reference_series() {
        const PERIOD: usize = 5;

        let prices: Vec<f64> = (1u32..=15)
            .map(|n| f64::from(n * (n + 1) / 2) * 0.37)
            .collect();

        let reference = reference_ma(&prices, PERIOD);

        let mut sma = SimpleMovingAverage::new(PERIOD, None);

        for (ix, (&price, &exp)) in prices.iter().zip(reference.iter()).enumerate() {
            sma.update_raw(price);

            let got = sma.value();
            assert!(
                (got - exp).abs() < 1e-12,
                "tick {ix}: expected {exp}, was {got}"
            );
        }
    }

    #[rstest]
    fn test_vidya_alpha_bounds() {
        let vidya_min = VariableIndexDynamicAverage::new(1, None, None);
        assert_eq!(vidya_min.alpha, 1.0);

        let vidya_large = VariableIndexDynamicAverage::new(1_000, None, None);
        assert!(vidya_large.alpha > 0.0 && vidya_large.alpha < 0.01);
    }

    #[rstest]
    fn test_vidya_value_constant_when_cmo_zero() {
        let mut vidya = VariableIndexDynamicAverage::new(3, None, None);

        for _ in 0..10 {
            vidya.update_raw(100.0);
        }

        let baseline = vidya.value;
        for _ in 0..5 {
            vidya.update_raw(100.0);
            assert!((vidya.value - baseline).abs() < 1e-12);
        }
    }

    #[rstest]
    fn test_vidya_handles_negative_prices() {
        let mut vidya = VariableIndexDynamicAverage::new(5, None, None);
        let negative_prices = [-1.0, -1.2, -0.8, -1.5, -1.3, -1.1];

        for p in negative_prices {
            vidya.update_raw(p);
            assert!(vidya.value.is_finite());
            assert!((0.0..=1.0).contains(&vidya.cmo_pct));
        }

        assert!(vidya.value < 0.0);
    }
}
