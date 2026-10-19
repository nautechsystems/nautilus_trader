// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::{
    indicator::{Indicator, MovingAverage},
    support::MAX_PERIOD,
};

/// Wilder moving average.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.indicators")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.indicators")
)]
pub struct WilderMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub alpha: f64,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    seed_sum: f64,
}

impl Display for WilderMovingAverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for WilderMovingAverage {
    fn name(&self) -> String {
        stringify!(WilderMovingAverage).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }
    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.update_raw(quote.extract_price(self.price_type)?.into());
        Ok(())
    }

    fn handle_trade(&mut self, t: &TradeTick) {
        self.update_raw((&t.price).into());
    }

    fn handle_bar(&mut self, b: &Bar) {
        self.update_raw((&b.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
        self.seed_sum = 0.0;
    }
}

impl WilderMovingAverage {
    /// Creates a new [`WilderMovingAverage`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero or exceeds 16,777,216.
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self::new_checked(period, price_type).expect(FAILED)
    }

    /// Creates a Wilder moving average with a validated period.
    ///
    /// # Errors
    ///
    /// Returns an error if `period` is zero or exceeds 16,777,216.
    pub fn new_checked(period: usize, price_type: Option<PriceType>) -> anyhow::Result<Self> {
        check_predicate_true(
            period > 0,
            &format!("WilderMovingAverage: period must be > 0 (received {period})"),
        )?;
        check_predicate_true(
            period <= MAX_PERIOD,
            &format!("period cannot exceed {MAX_PERIOD}"),
        )?;
        Ok(Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            alpha: 1.0 / period as f64,
            value: 0.0,
            count: 0,
            initialized: false,
            has_inputs: false,
            seed_sum: 0.0,
        })
    }
}

impl MovingAverage for WilderMovingAverage {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.count
    }

    fn update_raw(&mut self, price: f64) {
        if !price.is_finite() {
            return;
        }

        self.has_inputs = true;
        self.count += 1;
        if self.count < self.period {
            self.seed_sum += price;
            return;
        }

        if self.count == self.period {
            self.seed_sum += price;
            self.value = self.seed_sum / self.period as f64;
            self.initialized = true;
            return;
        }

        self.value = (self.value * (self.period as f64 - 1.0) + price) / self.period as f64;
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::{Bar, QuoteTick, TradeTick},
        enums::PriceType,
    };
    use rstest::rstest;

    use super::MAX_PERIOD;
    use crate::{
        average::rma::WilderMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    #[case(0)]
    #[case(MAX_PERIOD + 1)]
    #[case(usize::MAX)]
    fn test_checked_constructor_rejects_invalid_period(#[case] period: usize) {
        assert!(WilderMovingAverage::new_checked(period, None).is_err());
    }

    #[rstest]
    fn test_rma_initialized(indicator_rma_10: WilderMovingAverage) {
        let rma = indicator_rma_10;
        let display_str = format!("{rma}");
        assert_eq!(display_str, "WilderMovingAverage(10)");
        assert_eq!(rma.period, 10);
        assert_eq!(rma.price_type, PriceType::Mid);
        assert_eq!(rma.alpha, 0.1);
        assert!(!rma.initialized);
    }

    #[rstest]
    #[should_panic(expected = "WilderMovingAverage: period must be > 0")]
    fn test_new_with_zero_period_panics() {
        let _ = WilderMovingAverage::new(0, None);
    }

    #[rstest]
    fn test_one_value_input(indicator_rma_10: WilderMovingAverage) {
        let mut rma = indicator_rma_10;
        rma.update_raw(1.0);
        assert_eq!(rma.count, 1);
        assert_eq!(rma.value, 0.0);
        assert!(!rma.initialized());
    }

    #[rstest]
    fn test_rma_update_raw(indicator_rma_10: WilderMovingAverage) {
        let mut rma = indicator_rma_10;
        for value in 1..=10 {
            rma.update_raw(f64::from(value));
        }

        assert!(rma.has_inputs());
        assert!(rma.initialized());
        assert_eq!(rma.count, 10);
        assert_eq!(rma.value, 5.5);
    }

    #[rstest]
    fn test_reset(indicator_rma_10: WilderMovingAverage) {
        let mut rma = indicator_rma_10;
        rma.update_raw(1.0);
        assert_eq!(rma.count, 1);
        rma.reset();
        assert_eq!(rma.count, 0);
        assert_eq!(rma.value, 0.0);
        assert!(!rma.initialized);
    }

    #[rstest]
    fn test_handle_quote_tick_single(indicator_rma_10: WilderMovingAverage, stub_quote: QuoteTick) {
        let mut rma = indicator_rma_10;
        rma.handle_quote(&stub_quote).unwrap();

        assert!(rma.has_inputs());
        assert_eq!(rma.count, 1);
        assert_eq!(rma.value, 0.0);
        assert!(!rma.initialized());
    }

    #[rstest]
    fn test_handle_quote_tick_multi(mut indicator_rma_10: WilderMovingAverage) {
        let tick1 = stub_quote("1500.0", "1502.0");
        let tick2 = stub_quote("1502.0", "1504.0");
        indicator_rma_10.handle_quote(&tick1).unwrap();
        indicator_rma_10.handle_quote(&tick2).unwrap();

        assert_eq!(indicator_rma_10.count, 2);
        assert_eq!(indicator_rma_10.value, 0.0);
        assert!(!indicator_rma_10.initialized());
    }

    #[rstest]
    fn test_handle_trade_tick(indicator_rma_10: WilderMovingAverage, stub_trade: TradeTick) {
        let mut rma = indicator_rma_10;
        rma.handle_trade(&stub_trade);

        assert!(rma.has_inputs());
        assert_eq!(rma.count, 1);
        assert_eq!(rma.value, 0.0);
        assert!(!rma.initialized());
    }

    #[rstest]
    fn handle_handle_bar(
        mut indicator_rma_10: WilderMovingAverage,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_rma_10.handle_bar(&bar_ethusdt_binance_minute_bid);

        assert!(indicator_rma_10.has_inputs);
        assert_eq!(indicator_rma_10.count, 1);
        assert_eq!(indicator_rma_10.value, 0.0);
        assert!(!indicator_rma_10.initialized);
    }

    #[rstest]
    #[should_panic(expected = "WilderMovingAverage: period must be > 0")]
    fn invalid_period_panics() {
        let _ = WilderMovingAverage::new(0, None);
    }

    #[rstest]
    #[case(1.0)]
    #[case(123.456)]
    #[case(9_876.543_21)]
    fn first_tick_seeding_parity(#[case] seed_price: f64) {
        let mut rma = WilderMovingAverage::new(10, None);
        rma.update_raw(seed_price);

        assert_eq!(rma.count(), 1);
        assert_eq!(rma.value(), 0.0);
        assert!(!rma.initialized());
    }

    #[rstest]
    fn numeric_parity_with_reference_series() {
        let mut rma = WilderMovingAverage::new(10, None);
        for price in 1_u32..=10 {
            rma.update_raw(f64::from(price));
        }

        assert!(rma.initialized());
        assert_eq!(rma.count(), 10);
        assert_eq!(rma.value(), 5.5);
    }

    /// Period = 1 should act as a pure 1-tick MA (α = 1) and be initialized immediately.
    #[rstest]
    fn test_rma_period_one_behavior() {
        let mut rma = WilderMovingAverage::new(1, None);

        // First tick seeds and immediately initializes
        rma.update_raw(42.0);
        assert!(rma.initialized());
        assert_eq!(rma.count(), 1);
        assert!((rma.value() - 42.0).abs() < 1e-12);

        // With α = 1 the next tick fully replaces the previous value
        rma.update_raw(100.0);
        assert_eq!(rma.count(), 2);
        assert!((rma.value() - 100.0).abs() < 1e-12);
    }

    /// Very large period: `initialized()` must remain `false` until enough samples arrive.
    #[rstest]
    fn test_rma_large_period_not_initialized() {
        let mut rma = WilderMovingAverage::new(1_000, None);

        for p in 1_u32..=999 {
            rma.update_raw(f64::from(p));
        }

        assert_eq!(rma.count(), 999);
        assert!(!rma.initialized());
    }

    #[rstest]
    fn test_reset_reseeds_properly() {
        let mut rma = WilderMovingAverage::new(3, None);
        for value in [1.0, 2.0, 3.0, 4.0] {
            rma.update_raw(value);
        }
        rma.reset();
        for value in [10.0, 20.0, 30.0] {
            rma.update_raw(value);
        }

        assert_eq!(rma.count(), 3);
        assert!(rma.initialized());
        assert_eq!(rma.value(), 20.0);
    }

    #[rstest]
    fn test_default_price_type_is_last() {
        let rma = WilderMovingAverage::new(5, None);
        assert_eq!(rma.price_type, PriceType::Last);
    }

    #[rstest]
    fn test_update_with_nan_propagates() {
        let mut rma = WilderMovingAverage::new(3, None);
        rma.update_raw(1.0);
        let before = (rma.value(), rma.count());

        rma.update_raw(f64::NAN);
        rma.update_raw(f64::NEG_INFINITY);

        assert_eq!((rma.value(), rma.count()), before);
        assert!(rma.has_inputs());
    }
}
