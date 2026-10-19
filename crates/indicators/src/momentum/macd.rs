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

use nautilus_core::correctness::FAILED;
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
    support::MAX_PERIOD,
};

/// Moving average convergence/divergence, signal, and histogram.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.indicators", unsendable)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.indicators")
)]
pub struct MovingAverageConvergenceDivergence {
    pub fast_period: usize,
    pub slow_period: usize,
    pub signal_period: usize,
    pub ma_type: MovingAverageType,
    pub count: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub signal: f64,
    pub histogram: f64,
    pub initialized: bool,
    has_inputs: bool,
    fast_ma: Box<dyn MovingAverage + Send + 'static>,
    slow_ma: Box<dyn MovingAverage + Send + 'static>,
    signal_ma: Box<dyn MovingAverage + Send + 'static>,
}

impl Display for MovingAverageConvergenceDivergence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{},{},{})",
            self.name(),
            self.fast_period,
            self.slow_period,
            self.signal_period,
            self.ma_type,
            self.price_type
        )
    }
}

impl Indicator for MovingAverageConvergenceDivergence {
    fn name(&self) -> String {
        stringify!(MovingAverageConvergenceDivergence).to_string()
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
        self.signal = 0.0;
        self.histogram = 0.0;
        self.count = 0;
        self.fast_ma.reset();
        self.slow_ma.reset();
        self.signal_ma.reset();
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl MovingAverageConvergenceDivergence {
    /// Creates a new [`MovingAverageConvergenceDivergence`] instance.
    ///
    /// The defaults are the standard MACD convention: exponential smoothing
    /// with a 9 period signal line.
    ///
    /// # Panics
    ///
    /// Panics if `fast_period` is zero or not less than `slow_period`, or if
    /// `signal_period` is zero.
    #[must_use]
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        signal_period: Option<usize>,
        ma_type: Option<MovingAverageType>,
        price_type: Option<PriceType>,
    ) -> Self {
        Self::new_checked(fast_period, slow_period, signal_period, ma_type, price_type)
            .expect(FAILED)
    }

    pub(crate) fn new_checked(
        fast_period: usize,
        slow_period: usize,
        signal_period: Option<usize>,
        ma_type: Option<MovingAverageType>,
        price_type: Option<PriceType>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            fast_period <= MAX_PERIOD,
            "fast_period cannot exceed {MAX_PERIOD}"
        );
        anyhow::ensure!(
            slow_period <= MAX_PERIOD,
            "slow_period cannot exceed {MAX_PERIOD}"
        );
        anyhow::ensure!(
            fast_period > 0 && fast_period < slow_period,
            "MovingAverageConvergenceDivergence: fast_period must be > 0 and < slow_period (received fast {fast_period}, slow {slow_period})"
        );
        let signal_period = signal_period.unwrap_or(9);
        anyhow::ensure!(
            signal_period <= MAX_PERIOD,
            "signal_period cannot exceed {MAX_PERIOD}"
        );
        anyhow::ensure!(
            signal_period > 0,
            "MovingAverageConvergenceDivergence: signal_period must be > 0 (received {signal_period})"
        );
        let ma_type = ma_type.unwrap_or(MovingAverageType::Exponential);

        Ok(Self {
            fast_period,
            slow_period,
            signal_period,
            ma_type,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            signal: 0.0,
            histogram: 0.0,
            count: 0,
            initialized: false,
            has_inputs: false,
            fast_ma: MovingAverageFactory::create(ma_type, fast_period),
            slow_ma: MovingAverageFactory::create(ma_type, slow_period),
            signal_ma: MovingAverageFactory::create(ma_type, signal_period),
        })
    }
}

impl MovingAverage for MovingAverageConvergenceDivergence {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.count
    }

    fn update_raw(&mut self, close: f64) {
        if !close.is_finite() {
            return;
        }
        self.fast_ma.update_raw(close);
        self.slow_ma.update_raw(close);
        self.count += 1;

        self.has_inputs = true;

        if self.fast_ma.initialized() && self.slow_ma.initialized() {
            let value = self.fast_ma.value() - self.slow_ma.value();
            self.signal_ma.update_raw(value);
            self.initialized = self.signal_ma.initialized();
            if self.initialized {
                self.value = value;
                self.signal = self.signal_ma.value();
                self.histogram = self.value - self.signal;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::{Bar, QuoteTick, TradeTick};
    use rstest::rstest;

    use crate::{
        average::MovingAverageType,
        indicator::{Indicator, MovingAverage},
        momentum::macd::MovingAverageConvergenceDivergence,
        stubs::*,
    };

    #[rstest]
    fn test_macd_initialized(macd_10: MovingAverageConvergenceDivergence) {
        let display_st = format!("{macd_10}");
        assert_eq!(
            display_st,
            "MovingAverageConvergenceDivergence(8,10,9,SIMPLE,BID)"
        );
        assert_eq!(macd_10.fast_period, 8);
        assert_eq!(macd_10.slow_period, 10);
        assert_eq!(macd_10.signal_period, 9);
        assert!(!macd_10.initialized());
        assert!(!macd_10.has_inputs());
    }

    #[rstest]
    fn test_initialized_with_required_input(mut macd_10: MovingAverageConvergenceDivergence) {
        // Composite warmup is slow_period + signal_period - 1 = 18 inputs
        for i in 1..18 {
            macd_10.update_raw(f64::from(i));
            assert!(!macd_10.initialized);
        }
        macd_10.update_raw(18.0);
        assert!(macd_10.initialized);
    }

    #[rstest]
    #[case(MovingAverageType::Simple, 3)]
    #[case(MovingAverageType::Exponential, 3)]
    #[case(MovingAverageType::DoubleExponential, 5)]
    #[case(MovingAverageType::Wilder, 3)]
    #[case(MovingAverageType::Hull, 3)]
    fn test_composite_warmup_uses_selected_ma(
        #[case] ma_type: MovingAverageType,
        #[case] warmup: usize,
    ) {
        let mut macd = MovingAverageConvergenceDivergence::new(1, 2, Some(2), Some(ma_type), None);

        for i in 1..warmup {
            macd.update_raw(i as f64);
            assert!(!macd.initialized(), "initialized at input {i}");
        }

        macd.update_raw(warmup as f64);
        assert!(macd.initialized());
    }

    #[rstest]
    fn test_hull_signal_waits_for_full_warmup() {
        let mut macd = MovingAverageConvergenceDivergence::new(
            1,
            2,
            Some(20),
            Some(MovingAverageType::Hull),
            None,
        );

        for i in 1..24 {
            macd.update_raw(f64::from(i));
            assert!(!macd.initialized(), "initialized at input {i}");
        }

        macd.update_raw(24.0);
        assert!(macd.initialized());
    }

    #[rstest]
    fn test_value_with_one_input(mut macd_10: MovingAverageConvergenceDivergence) {
        macd_10.update_raw(1.0);
        assert_eq!(macd_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut macd_10: MovingAverageConvergenceDivergence) {
        macd_10.update_raw(1.0);
        macd_10.update_raw(2.0);
        macd_10.update_raw(3.0);
        assert_eq!(macd_10.value, 0.0);
    }

    #[rstest]
    fn test_value_before_signal_ready(mut macd_10: MovingAverageConvergenceDivergence) {
        macd_10.update_raw(1.00000);
        macd_10.update_raw(1.00010);
        macd_10.update_raw(1.00020);
        macd_10.update_raw(1.00030);
        macd_10.update_raw(1.00040);
        macd_10.update_raw(1.00050);
        macd_10.update_raw(1.00040);
        macd_10.update_raw(1.00030);
        macd_10.update_raw(1.00020);
        macd_10.update_raw(1.00010);
        macd_10.update_raw(1.00000);
        assert_eq!(
            (macd_10.value, macd_10.signal, macd_10.histogram),
            (0.0, 0.0, 0.0)
        );
        assert!(!macd_10.initialized);
    }

    #[rstest]
    fn test_handle_quote_tick(
        mut macd_10: MovingAverageConvergenceDivergence,
        stub_quote: QuoteTick,
    ) {
        macd_10.handle_quote(&stub_quote).unwrap();
        assert_eq!(macd_10.value, 0.0);
    }

    #[rstest]
    fn test_handle_trade_tick(
        mut macd_10: MovingAverageConvergenceDivergence,
        stub_trade: TradeTick,
    ) {
        macd_10.handle_trade(&stub_trade);
        assert_eq!(macd_10.value, 0.0);
    }

    #[rstest]
    fn test_handle_bar(
        mut macd_10: MovingAverageConvergenceDivergence,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        macd_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(macd_10.value, 0.0);
        assert!(!macd_10.initialized);
    }

    #[rstest]
    fn test_reset(mut macd_10: MovingAverageConvergenceDivergence) {
        macd_10.update_raw(1.0);
        macd_10.reset();
        assert_eq!(macd_10.value, 0.0);
        assert_eq!(macd_10.signal, 0.0);
        assert_eq!(macd_10.histogram, 0.0);
        assert_eq!(macd_10.count, 0);
        assert_eq!(macd_10.fast_ma.value(), 0.0);
        assert_eq!(macd_10.slow_ma.value(), 0.0);
        assert!(!macd_10.has_inputs);
        assert!(!macd_10.initialized);
    }

    #[rstest]
    fn count_matches_inputs(mut macd_10: MovingAverageConvergenceDivergence) {
        assert_eq!(macd_10.count(), 0);

        for i in 1..=12 {
            macd_10.update_raw(f64::from(i));
            assert_eq!(macd_10.count(), i as usize);
        }
    }
}
