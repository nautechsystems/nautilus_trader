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

use std::fmt::{Debug, Display};

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::data::Bar;

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

const MAX_PERIOD: usize = 1024;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct CommodityChannelIndex {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub scalar: f64,
    pub value: f64,
    pub initialized: bool,
    ma: Box<dyn MovingAverage + Send + 'static>,
    has_inputs: bool,
    mad: f64,
    prices: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
}

impl Display for CommodityChannelIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{})", self.name(), self.period, self.ma_type,)
    }
}

impl Indicator for CommodityChannelIndex {
    fn name(&self) -> String {
        stringify!(CommodityChannelIndex).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into(), (&bar.close).into());
    }

    fn reset(&mut self) {
        self.ma.reset();
        self.mad = 0.0;
        self.prices.clear();
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl CommodityChannelIndex {
    /// Creates a new [`CommodityChannelIndex`] instance.
    ///
    /// # Panics
    ///
    /// - If `period` is less than or equal to 0.
    /// - If `period` exceeds `MAX_PERIOD`.
    #[must_use]
    pub fn new(period: usize, scalar: f64, ma_type: Option<MovingAverageType>) -> Self {
        assert!(period > 0, "CommodityChannelIndex: period must be > 0");
        assert!(
            period <= MAX_PERIOD,
            "CommodityChannelIndex: period exceeds MAX_PERIOD"
        );

        Self {
            period,
            scalar,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            value: 0.0,
            prices: ArrayDeque::new(),
            ma: MovingAverageFactory::create(ma_type.unwrap_or(MovingAverageType::Simple), period),
            has_inputs: false,
            initialized: false,
            mad: 0.0,
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        let typical_price = (high + low + close) / 3.0;

        if self.prices.len() == self.period {
            let _ = self.prices.pop_front();
        }
        let _ = self.prices.push_back(typical_price);

        self.ma.update_raw(typical_price);

        self.mad = fast_mad_with_mean(self.prices.iter().copied(), self.ma.value());

        if self.ma.initialized() && self.mad != 0.0 {
            self.value = (typical_price - self.ma.value()) / (self.scalar * self.mad);
        }

        if !self.initialized {
            self.has_inputs = true;
            if self.ma.initialized() {
                self.initialized = true;
            }
        }
    }
}

pub fn fast_mad_with_mean<I>(values: I, mean: f64) -> f64
where
    I: IntoIterator<Item = f64>,
{
    let mut acc = 0.0_f64;
    let mut count = 0_usize;

    for v in values {
        acc += (v - mean).abs();
        count += 1;
    }

    if count == 0 { 0.0 } else { acc / count as f64 }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::rstest;

    use crate::{
        indicator::Indicator,
        momentum::cci::CommodityChannelIndex,
        stubs::{bar_ethusdt_binance_minute_bid, cci_10},
    };

    #[rstest]
    fn test_psl_initialized(cci_10: CommodityChannelIndex) {
        let display_str = format!("{cci_10}");
        assert_eq!(display_str, "CommodityChannelIndex(10,SIMPLE)");
        assert_eq!(cci_10.period, 10);
        assert!(!cci_10.initialized);
        assert!(!cci_10.has_inputs);
    }

    #[rstest]
    fn test_value_with_one_input(mut cci_10: CommodityChannelIndex) {
        cci_10.update_raw(1.0, 0.9, 0.95);
        assert_eq!(cci_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut cci_10: CommodityChannelIndex) {
        cci_10.update_raw(1.0, 0.9, 0.95);
        cci_10.update_raw(2.0, 1.9, 1.95);
        cci_10.update_raw(3.0, 2.9, 2.95);
        assert_eq!(cci_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut cci_10: CommodityChannelIndex) {
        cci_10.update_raw(1.00000, 0.90000, 1.00000);
        cci_10.update_raw(1.00010, 0.90010, 1.00010);
        cci_10.update_raw(1.00030, 0.90020, 1.00020);
        cci_10.update_raw(1.00040, 0.90030, 1.00030);
        cci_10.update_raw(1.00050, 0.90040, 1.00040);
        cci_10.update_raw(1.00060, 0.90050, 1.00050);
        cci_10.update_raw(1.00050, 0.90040, 1.00040);
        cci_10.update_raw(1.00040, 0.90030, 1.00030);
        cci_10.update_raw(1.00030, 0.90020, 1.00020);
        cci_10.update_raw(1.00010, 0.90010, 1.00010);
        cci_10.update_raw(1.00000, 0.90000, 1.00000);
        assert_eq!(cci_10.value, -0.976_190_476_190_006_1);
    }

    #[rstest]
    fn test_initialized_with_required_input(mut cci_10: CommodityChannelIndex) {
        for i in 1..10 {
            cci_10.update_raw(f64::from(i), f64::from(i), f64::from(i));
        }
        assert!(!cci_10.initialized);
        cci_10.update_raw(10.0, 10.0, 10.0);
        assert!(cci_10.initialized);
    }

    #[rstest]
    fn test_handle_bar(mut cci_10: CommodityChannelIndex, bar_ethusdt_binance_minute_bid: Bar) {
        cci_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(cci_10.value, 0.0);
        assert!(cci_10.has_inputs);
        assert!(!cci_10.initialized);
    }

    #[rstest]
    fn test_reset(mut cci_10: CommodityChannelIndex) {
        cci_10.update_raw(1.0, 0.9, 0.95);
        cci_10.reset();
        assert_eq!(cci_10.value, 0.0);
        assert_eq!(cci_10.prices.len(), 0);
        assert_eq!(cci_10.mad, 0.0);
        assert!(!cci_10.has_inputs);
        assert!(!cci_10.initialized);
    }
}
