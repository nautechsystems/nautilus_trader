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

/// Exponential moving average.
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
pub struct ExponentialMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub alpha: f64,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    seed_sum: f64,
}

impl Display for ExponentialMovingAverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for ExponentialMovingAverage {
    fn name(&self) -> String {
        stringify!(ExponentialMovingAverage).to_string()
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

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.update_raw((&trade.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
        self.seed_sum = 0.0;
    }
}

impl ExponentialMovingAverage {
    /// Creates a new [`ExponentialMovingAverage`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero or exceeds 16,777,216.
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self::new_checked(period, price_type).expect(FAILED)
    }

    /// Creates an exponential moving average with a validated period.
    ///
    /// # Errors
    ///
    /// Returns an error if `period` is zero or exceeds 16,777,216.
    pub fn new_checked(period: usize, price_type: Option<PriceType>) -> anyhow::Result<Self> {
        check_predicate_true(
            period > 0,
            &format!("ExponentialMovingAverage: period must be > 0 (received {period})"),
        )?;
        check_predicate_true(
            period <= MAX_PERIOD,
            &format!("period cannot exceed {MAX_PERIOD}"),
        )?;
        Ok(Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            alpha: 2.0 / (period as f64 + 1.0),
            value: 0.0,
            count: 0,
            has_inputs: false,
            initialized: false,
            seed_sum: 0.0,
        })
    }
}

impl MovingAverage for ExponentialMovingAverage {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.count
    }

    fn update_raw(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }

        self.has_inputs = true;
        self.count += 1;
        if self.count < self.period {
            self.seed_sum += value;
            return;
        }

        if self.count == self.period {
            self.seed_sum += value;
            self.value = self.seed_sum / self.period as f64;
            self.initialized = true;
            return;
        }

        self.value = self.alpha.mul_add(value, (1.0 - self.alpha) * self.value);
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
        average::ema::ExponentialMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
        testing::assert_approx_equal,
    };

    #[rstest]
    #[case(0)]
    #[case(MAX_PERIOD + 1)]
    #[case(usize::MAX)]
    fn test_checked_constructor_rejects_invalid_period(#[case] period: usize) {
        assert!(ExponentialMovingAverage::new_checked(period, None).is_err());
    }

    #[rstest]
    fn test_ema_initialized(indicator_ema_10: ExponentialMovingAverage) {
        let ema = indicator_ema_10;
        let display_str = format!("{ema}");
        assert_eq!(display_str, "ExponentialMovingAverage(10)");
        assert_eq!(ema.period, 10);
        assert_eq!(ema.price_type, PriceType::Mid);
        assert_approx_equal(ema.alpha, 0.181818181818);
        assert!(!ema.initialized);
    }

    #[rstest]
    fn test_one_value_input(indicator_ema_10: ExponentialMovingAverage) {
        let mut ema = indicator_ema_10;
        ema.update_raw(1.0);
        assert_eq!(ema.count, 1);
        assert_eq!(ema.value, 0.0);
        assert!(!ema.initialized());
    }

    #[rstest]
    fn test_ema_update_raw(indicator_ema_10: ExponentialMovingAverage) {
        let mut ema = indicator_ema_10;
        for value in 1..=10 {
            ema.update_raw(f64::from(value));
        }

        assert!(ema.has_inputs());
        assert!(ema.initialized());
        assert_eq!(ema.count, 10);
        assert_eq!(ema.value, 5.5);
    }

    #[rstest]
    fn test_reset(indicator_ema_10: ExponentialMovingAverage) {
        let mut ema = indicator_ema_10;
        ema.update_raw(1.0);
        assert_eq!(ema.count, 1);
        ema.reset();
        assert_eq!(ema.count, 0);
        assert_eq!(ema.value, 0.0);
        assert!(!ema.initialized);
    }

    #[rstest]
    fn test_handle_quote_tick_single(
        indicator_ema_10: ExponentialMovingAverage,
        stub_quote: QuoteTick,
    ) {
        let mut ema = indicator_ema_10;
        ema.handle_quote(&stub_quote).unwrap();

        assert!(ema.has_inputs());
        assert_eq!(ema.count, 1);
        assert_eq!(ema.value, 0.0);
        assert!(!ema.initialized());
    }

    #[rstest]
    fn test_handle_quote_tick_multi(mut indicator_ema_10: ExponentialMovingAverage) {
        let tick1 = stub_quote("1500.0", "1502.0");
        let tick2 = stub_quote("1502.0", "1504.0");
        indicator_ema_10.handle_quote(&tick1).unwrap();
        indicator_ema_10.handle_quote(&tick2).unwrap();

        assert_eq!(indicator_ema_10.count, 2);
        assert_eq!(indicator_ema_10.value, 0.0);
        assert!(!indicator_ema_10.initialized());
    }

    #[rstest]
    fn test_handle_trade_tick(indicator_ema_10: ExponentialMovingAverage, stub_trade: TradeTick) {
        let mut ema = indicator_ema_10;
        ema.handle_trade(&stub_trade);

        assert!(ema.has_inputs());
        assert_eq!(ema.count, 1);
        assert_eq!(ema.value, 0.0);
        assert!(!ema.initialized());
    }

    #[rstest]
    fn handle_handle_bar(
        mut indicator_ema_10: ExponentialMovingAverage,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_ema_10.handle_bar(&bar_ethusdt_binance_minute_bid);

        assert!(indicator_ema_10.has_inputs);
        assert_eq!(indicator_ema_10.count, 1);
        assert_eq!(indicator_ema_10.value, 0.0);
        assert!(!indicator_ema_10.initialized);
    }

    #[rstest]
    fn test_period_one_behavior() {
        let mut ema = ExponentialMovingAverage::new(1, None);
        assert_eq!(ema.alpha, 1.0, "α must be 1 when period = 1");

        ema.update_raw(10.0);
        assert!(ema.initialized());
        assert_eq!(ema.value(), 10.0);

        ema.update_raw(42.0);
        assert_eq!(
            ema.value(),
            42.0,
            "With α = 1, the EMA must track the latest sample exactly"
        );
    }

    #[rstest]
    fn test_default_price_type_is_last() {
        let ema = ExponentialMovingAverage::new(3, None);
        assert_eq!(
            ema.price_type,
            PriceType::Last,
            "`price_type` default mismatch"
        );
    }

    #[rstest]
    fn test_nan_poisoning_and_reset_recovery() {
        let mut ema = ExponentialMovingAverage::new(4, None);
        for value in [1.0, 2.0, 3.0, 4.0] {
            ema.update_raw(value);
        }
        let before = (ema.value(), ema.count());

        ema.update_raw(f64::NAN);
        ema.update_raw(f64::INFINITY);

        assert_eq!((ema.value(), ema.count()), before);
        ema.update_raw(5.0);
        assert!(ema.value().is_finite());
        ema.reset();
        assert!(!ema.has_inputs());
        assert_eq!(ema.value(), 0.0);
    }

    #[rstest]
    fn test_reset_without_inputs_is_safe() {
        let mut ema = ExponentialMovingAverage::new(8, None);
        ema.reset();
        assert!(!ema.has_inputs());
        assert_eq!(ema.count(), 0);
        assert!(!ema.initialized());
    }

    #[rstest]
    fn test_has_inputs_lifecycle() {
        let mut ema = ExponentialMovingAverage::new(5, None);
        assert!(!ema.has_inputs());

        ema.update_raw(1.23);
        assert!(ema.has_inputs());

        ema.reset();
        assert!(!ema.has_inputs());
    }

    #[rstest]
    fn test_subnormal_inputs_do_not_underflow() {
        let mut ema = ExponentialMovingAverage::new(2, None);
        let tiny = f64::MIN_POSITIVE / 2.0;
        ema.update_raw(tiny);
        ema.update_raw(tiny);
        assert!(
            ema.value() > 0.0,
            "Underflow: EMA value collapsed to zero for sub-normal inputs"
        );
    }
}
