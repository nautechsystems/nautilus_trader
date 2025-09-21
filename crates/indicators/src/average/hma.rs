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
    average::wma::WeightedMovingAverage,
    indicator::{Indicator, MovingAverage},
};

/// An indicator which calculates a Hull Moving Average (HMA) across a rolling
/// window. The HMA, developed by Alan Hull, is an extremely fast and smooth
/// moving average.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct HullMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    ma1: WeightedMovingAverage,
    ma2: WeightedMovingAverage,
    ma3: WeightedMovingAverage,
}

impl Display for HullMovingAverage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for HullMovingAverage {
    fn name(&self) -> String {
        stringify!(HullMovingAverage).to_string()
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
        self.ma1.reset();
        self.ma2.reset();
        self.ma3.reset();
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

fn get_weights(size: usize) -> Vec<f64> {
    let mut w: Vec<f64> = (1..=size).map(|x| x as f64).collect();
    let divisor: f64 = w.iter().sum();
    for v in &mut w {
        *v /= divisor;
    }
    w
}

impl HullMovingAverage {
    /// Creates a new [`HullMovingAverage`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is not a positive integer (> 0).
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        assert!(
            period > 0,
            "HullMovingAverage: period must be > 0 (received {period})"
        );

        let half = usize::max(1, period / 2);
        let root = usize::max(1, (period as f64).sqrt() as usize);

        let pt = price_type.unwrap_or(PriceType::Last);

        let ma1 = WeightedMovingAverage::new(half, get_weights(half), Some(pt));
        let ma2 = WeightedMovingAverage::new(period, get_weights(period), Some(pt));
        let ma3 = WeightedMovingAverage::new(root, get_weights(root), Some(pt));

        Self {
            period,
            price_type: pt,
            value: 0.0,
            count: 0,
            has_inputs: false,
            initialized: false,
            ma1,
            ma2,
            ma3,
        }
    }
}

impl MovingAverage for HullMovingAverage {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.count
    }

    fn update_raw(&mut self, value: f64) {
        if !self.has_inputs {
            self.has_inputs = true;
            self.value = value;
        }

        self.ma1.update_raw(value);
        self.ma2.update_raw(value);
        self.ma3
            .update_raw(2.0f64.mul_add(self.ma1.value, -self.ma2.value));

        self.value = self.ma3.value;
        self.count += 1;

        if !self.initialized && self.count >= self.period {
            self.initialized = true;
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::{Bar, QuoteTick, TradeTick},
        enums::PriceType,
    };
    use rstest::rstest;

    use crate::{
        average::hma::HullMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn test_hma_initialized(indicator_hma_10: HullMovingAverage) {
        let display_str = format!("{indicator_hma_10}");
        assert_eq!(display_str, "HullMovingAverage(10)");
        assert_eq!(indicator_hma_10.period, 10);
        assert!(!indicator_hma_10.initialized);
        assert!(!indicator_hma_10.has_inputs);
    }

    #[rstest]
    fn test_initialized_with_required_input(mut indicator_hma_10: HullMovingAverage) {
        for i in 1..10 {
            indicator_hma_10.update_raw(f64::from(i));
        }
        assert!(!indicator_hma_10.initialized);
        indicator_hma_10.update_raw(10.0);
        assert!(indicator_hma_10.initialized);
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_hma_10: HullMovingAverage) {
        indicator_hma_10.update_raw(1.0);
        assert_eq!(indicator_hma_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut indicator_hma_10: HullMovingAverage) {
        indicator_hma_10.update_raw(1.0);
        indicator_hma_10.update_raw(2.0);
        indicator_hma_10.update_raw(3.0);
        assert_eq!(indicator_hma_10.value, 1.824_561_403_508_772);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut indicator_hma_10: HullMovingAverage) {
        indicator_hma_10.update_raw(1.00000);
        indicator_hma_10.update_raw(1.00010);
        indicator_hma_10.update_raw(1.00020);
        indicator_hma_10.update_raw(1.00030);
        indicator_hma_10.update_raw(1.00040);
        indicator_hma_10.update_raw(1.00050);
        indicator_hma_10.update_raw(1.00040);
        indicator_hma_10.update_raw(1.00030);
        indicator_hma_10.update_raw(1.00020);
        indicator_hma_10.update_raw(1.00010);
        indicator_hma_10.update_raw(1.00000);
        assert_eq!(indicator_hma_10.value, 1.000_140_392_817_059_8);
    }

    #[rstest]
    fn test_handle_quote_tick(mut indicator_hma_10: HullMovingAverage, stub_quote: QuoteTick) {
        indicator_hma_10.handle_quote(&stub_quote);
        assert_eq!(indicator_hma_10.value, 1501.0);
    }

    #[rstest]
    fn test_handle_trade_tick(mut indicator_hma_10: HullMovingAverage, stub_trade: TradeTick) {
        indicator_hma_10.handle_trade(&stub_trade);
        assert_eq!(indicator_hma_10.value, 1500.0);
    }

    #[rstest]
    fn test_handle_bar(
        mut indicator_hma_10: HullMovingAverage,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_hma_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(indicator_hma_10.value, 1522.0);
        assert!(indicator_hma_10.has_inputs);
        assert!(!indicator_hma_10.initialized);
    }

    #[rstest]
    fn test_reset(mut indicator_hma_10: HullMovingAverage) {
        indicator_hma_10.update_raw(1.0);
        assert_eq!(indicator_hma_10.count, 1);
        assert_eq!(indicator_hma_10.value, 1.0);
        assert_eq!(indicator_hma_10.ma1.value, 1.0);
        assert_eq!(indicator_hma_10.ma2.value, 1.0);
        assert_eq!(indicator_hma_10.ma3.value, 1.0);
        indicator_hma_10.reset();
        assert_eq!(indicator_hma_10.value, 0.0);
        assert_eq!(indicator_hma_10.count, 0);
        assert_eq!(indicator_hma_10.ma1.value, 0.0);
        assert_eq!(indicator_hma_10.ma2.value, 0.0);
        assert_eq!(indicator_hma_10.ma3.value, 0.0);
        assert!(!indicator_hma_10.has_inputs);
        assert!(!indicator_hma_10.initialized);
    }

    #[rstest]
    #[should_panic(expected = "HullMovingAverage: period must be > 0")]
    fn test_new_with_zero_period_panics() {
        let _ = HullMovingAverage::new(0, None);
    }

    #[rstest]
    #[case(1)]
    #[case(5)]
    #[case(128)]
    #[case(10_000)]
    fn test_new_with_positive_period_constructs(#[case] period: usize) {
        let hma = HullMovingAverage::new(period, None);
        assert_eq!(hma.period, period);
        assert_eq!(hma.count(), 0);
        assert!(!hma.initialized());
    }

    #[rstest]
    #[case(PriceType::Bid)]
    #[case(PriceType::Ask)]
    #[case(PriceType::Last)]
    fn test_price_type_propagates_to_inner_wmas(#[case] pt: PriceType) {
        let hma = HullMovingAverage::new(10, Some(pt));
        assert_eq!(hma.price_type, pt);
        assert_eq!(hma.ma1.price_type, pt);
        assert_eq!(hma.ma2.price_type, pt);
        assert_eq!(hma.ma3.price_type, pt);
    }

    #[rstest]
    fn test_price_type_defaults_to_last() {
        let hma = HullMovingAverage::new(10, None);
        assert_eq!(hma.price_type, PriceType::Last);
        assert_eq!(hma.ma1.price_type, PriceType::Last);
        assert_eq!(hma.ma2.price_type, PriceType::Last);
        assert_eq!(hma.ma3.price_type, PriceType::Last);
    }

    #[rstest]
    #[case(10.0)]
    #[case(-5.5)]
    #[case(42.42)]
    #[case(0.0)]
    fn period_one_degenerates_to_price(#[case] price: f64) {
        let mut hma = HullMovingAverage::new(1, None);

        for _ in 0..5 {
            hma.update_raw(price);
            assert!(
                (hma.value() - price).abs() < f64::EPSILON,
                "HMA(1) should equal last price {price}, was {}",
                hma.value()
            );
            assert!(hma.initialized(), "HMA(1) must initialise immediately");
        }
    }

    #[rstest]
    #[case(3, 123.456_f64)]
    #[case(13, 0.001_f64)]
    fn constant_series_yields_constant_value(#[case] period: usize, #[case] constant: f64) {
        let mut hma = HullMovingAverage::new(period, None);

        for _ in 0..(period * 4) {
            hma.update_raw(constant);
            assert!(
                (hma.value() - constant).abs() < 1e-12,
                "Expected {constant}, was {}",
                hma.value()
            );
        }
        assert!(hma.initialized());
    }

    #[rstest]
    fn alternating_extremes_bounded() {
        let mut hma = HullMovingAverage::new(50, None);
        let lows_highs = [0.0_f64, 1_000.0_f64];

        for i in 0..200 {
            let price = lows_highs[i & 1];
            hma.update_raw(price);

            let v = hma.value();
            assert!((0.0..=1_000.0).contains(&v), "HMA out of bounds: {v}");
        }
    }

    #[rstest]
    #[case(2)]
    #[case(17)]
    #[case(128)]
    fn initialized_boundary(#[case] period: usize) {
        let mut hma = HullMovingAverage::new(period, None);

        for i in 0..(period - 1) {
            hma.update_raw(i as f64);
            assert!(!hma.initialized(), "HMA wrongly initialised at count {i}");
        }

        hma.update_raw(0.0);
        assert!(
            hma.initialized(),
            "HMA should initialise at exactly {period} ticks"
        );
    }

    #[rstest]
    #[case(2)]
    #[case(3)]
    fn small_periods_do_not_panic(#[case] period: usize) {
        let mut hma = HullMovingAverage::new(period, None);
        for i in 0..(period * 5) {
            hma.update_raw(i as f64);
        }
        assert!(hma.initialized());
    }

    #[rstest]
    fn negative_prices_supported() {
        let mut hma = HullMovingAverage::new(10, None);
        let prices = [-5.0, -4.0, -3.0, -2.5, -2.0, -1.5, -1.0, -0.5, 0.0, 0.5];

        for &p in &prices {
            hma.update_raw(p);
            let v = hma.value();
            assert!(
                v.is_finite(),
                "HMA produced a non-finite value {v} from negative prices"
            );
        }
    }
}
