// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use anyhow::Result;
use nautilus_model::{
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    enums::PriceType,
};
use pyo3::prelude::*;

use crate::{
    indicator::{Indicator, MovingAverage},
    ratio::efficiency_ratio::EfficiencyRatio,
};

/// An indicator which calculates an adaptive moving average (AMA) across a
/// rolling window. Developed by Perry Kaufman, the AMA is a moving average
/// designed to account for market noise and volatility. The AMA will closely
/// follow prices when the price swings are relatively small and the noise is
/// low. The AMA will increase lag when the price swings increase.
#[repr(C)]
#[derive(Debug)]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")]
pub struct AdaptiveMovingAverage {
    /// The period for the internal `EfficiencyRatio` indicator.
    pub period_efficiency_ratio: usize,
    /// The period for the fast smoothing constant (> 0).
    pub period_fast: usize,
    /// The period for the slow smoothing constant (> 0 < `period_fast`).
    pub period_slow: usize,
    /// The price type used for calculations.
    pub price_type: PriceType,
    /// The last indicator value.
    pub value: f64,
    /// The input count for the indicator.
    pub count: usize,
    _efficiency_ratio: EfficiencyRatio,
    _prior_value: Option<f64>,
    _alpha_fast: f64,
    _alpha_slow: f64,
    has_inputs: bool,
    is_initialized: bool,
}

impl Display for AdaptiveMovingAverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{})",
            self.name(),
            self.period_efficiency_ratio,
            self.period_fast,
            self.period_slow
        )
    }
}

impl Indicator for AdaptiveMovingAverage {
    fn name(&self) -> String {
        stringify!(AdaptiveMovingAverage).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn is_initialized(&self) -> bool {
        self.is_initialized
    }

    fn handle_quote_tick(&mut self, tick: &QuoteTick) {
        self.update_raw(tick.extract_price(self.price_type).into());
    }

    fn handle_trade_tick(&mut self, tick: &TradeTick) {
        self.update_raw((&tick.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.is_initialized = false;
    }
}

impl AdaptiveMovingAverage {
    pub fn new(
        period_efficiency_ratio: usize,
        period_fast: usize,
        period_slow: usize,
        price_type: Option<PriceType>,
    ) -> Result<Self> {
        // Inputs don't require validation, however we return a `Result`
        // to standardize with other indicators which do need validation.
        Ok(Self {
            period_efficiency_ratio,
            period_fast,
            period_slow,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            count: 0,
            _alpha_fast: 2.0 / (period_fast + 1) as f64,
            _alpha_slow: 2.0 / (period_slow + 1) as f64,
            _prior_value: None,
            has_inputs: false,
            is_initialized: false,
            _efficiency_ratio: EfficiencyRatio::new(period_efficiency_ratio, price_type)?,
        })
    }

    #[must_use]
    pub fn alpha_diff(&self) -> f64 {
        self._alpha_fast - self._alpha_slow
    }

    pub fn reset(&mut self) {
        self.value = 0.0;
        self._prior_value = None;
        self.count = 0;
        self.has_inputs = false;
        self.is_initialized = false;
    }
}

impl MovingAverage for AdaptiveMovingAverage {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.count
    }

    fn update_raw(&mut self, value: f64) {
        if !self.has_inputs {
            self._prior_value = Some(value);
            self._efficiency_ratio.update_raw(value);
            self.value = value;
            self.has_inputs = true;
            return;
        }
        self._efficiency_ratio.update_raw(value);
        self._prior_value = Some(self.value);

        // Calculate the smoothing constant
        let smoothing_constant = self
            ._efficiency_ratio
            .value
            .mul_add(self.alpha_diff(), self._alpha_slow)
            .powi(2);

        // Calculate the AMA
        self.value = smoothing_constant.mul_add(
            value - self._prior_value.unwrap(),
            self._prior_value.unwrap(),
        );
        if self._efficiency_ratio.is_initialized() {
            self.is_initialized = true;
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::{bar::Bar, quote::QuoteTick, trade::TradeTick};
    use rstest::rstest;

    use crate::{
        average::ama::AdaptiveMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn test_ama_initialized(indicator_ama_10: AdaptiveMovingAverage) {
        let display_str = format!("{indicator_ama_10}");
        assert_eq!(display_str, "AdaptiveMovingAverage(10,2,30)");
        assert_eq!(indicator_ama_10.name(), "AdaptiveMovingAverage");
        assert!(!indicator_ama_10.has_inputs());
        assert!(!indicator_ama_10.is_initialized());
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_ama_10: AdaptiveMovingAverage) {
        indicator_ama_10.update_raw(1.0);
        assert_eq!(indicator_ama_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_two_inputs(mut indicator_ama_10: AdaptiveMovingAverage) {
        indicator_ama_10.update_raw(1.0);
        indicator_ama_10.update_raw(2.0);
        assert_eq!(indicator_ama_10.value, 1.444_444_444_444_444_2);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut indicator_ama_10: AdaptiveMovingAverage) {
        indicator_ama_10.update_raw(1.0);
        indicator_ama_10.update_raw(2.0);
        indicator_ama_10.update_raw(3.0);
        assert_eq!(indicator_ama_10.value, 2.135_802_469_135_802);
    }

    #[rstest]
    fn test_reset(mut indicator_ama_10: AdaptiveMovingAverage) {
        for _ in 0..10 {
            indicator_ama_10.update_raw(1.0);
        }
        assert!(indicator_ama_10.is_initialized);
        indicator_ama_10.reset();
        assert!(!indicator_ama_10.is_initialized);
        assert!(!indicator_ama_10.has_inputs);
        assert_eq!(indicator_ama_10.value, 0.0);
    }

    #[rstest]
    fn test_initialized_after_correct_number_of_input(indicator_ama_10: AdaptiveMovingAverage) {
        let mut ama = indicator_ama_10;
        for _ in 0..9 {
            ama.update_raw(1.0);
        }
        assert!(!ama.is_initialized);
        ama.update_raw(1.0);
        assert!(ama.is_initialized);
    }

    #[rstest]
    fn test_handle_quote_tick(mut indicator_ama_10: AdaptiveMovingAverage, quote_tick: QuoteTick) {
        indicator_ama_10.handle_quote_tick(&quote_tick);
        assert!(indicator_ama_10.has_inputs);
        assert!(!indicator_ama_10.is_initialized);
        assert_eq!(indicator_ama_10.value, 1501.0);
    }

    #[rstest]
    fn test_handle_trade_tick_update(
        mut indicator_ama_10: AdaptiveMovingAverage,
        trade_tick: TradeTick,
    ) {
        indicator_ama_10.handle_trade_tick(&trade_tick);
        assert!(indicator_ama_10.has_inputs);
        assert!(!indicator_ama_10.is_initialized);
        assert_eq!(indicator_ama_10.value, 1500.0);
    }

    #[rstest]
    fn handle_handle_bar(
        mut indicator_ama_10: AdaptiveMovingAverage,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_ama_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert!(indicator_ama_10.has_inputs);
        assert!(!indicator_ama_10.is_initialized);
        assert_eq!(indicator_ama_10.value, 1522.0);
    }
}
