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

use nautilus_model::data::Bar;

use crate::indicator::Indicator;

#[repr(C)]
#[derive(Debug, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct VolumeWeightedAveragePrice {
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    price_volume: f64,
    volume_total: f64,
    day: f64,
}

impl Display for VolumeWeightedAveragePrice {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl Indicator for VolumeWeightedAveragePrice {
    fn name(&self) -> String {
        stringify!(VolumeWeightedAveragePrice).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        let typical_price = (bar.close.as_f64() + bar.high.as_f64() + bar.low.as_f64()) / 3.0;

        self.update_raw(typical_price, (&bar.volume).into(), bar.ts_init.as_f64());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
        self.day = 0.0;
        self.price_volume = 0.0;
        self.volume_total = 0.0;
        self.value = 0.0;
    }
}

impl VolumeWeightedAveragePrice {
    /// Creates a new [`VolumeWeightedAveragePrice`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value: 0.0,
            has_inputs: false,
            initialized: false,
            price_volume: 0.0,
            volume_total: 0.0,
            day: 0.0,
        }
    }

    pub fn update_raw(&mut self, price: f64, volume: f64, timestamp: f64) {
        if timestamp != self.day {
            self.reset();
            self.day = timestamp;
            self.value = price;
        }

        if !self.initialized {
            self.has_inputs = true;
            self.initialized = true;
        }

        if volume == 0.0 {
            return;
        }

        self.price_volume += price * volume;
        self.volume_total += volume;
        self.value = self.price_volume / self.volume_total;
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::rstest;

    use crate::{average::vwap::VolumeWeightedAveragePrice, indicator::Indicator, stubs::*};

    #[rstest]
    fn test_vwap_initialized(indicator_vwap: VolumeWeightedAveragePrice) {
        let display_st = format!("{indicator_vwap}");
        assert_eq!(display_st, "VolumeWeightedAveragePrice");
        assert!(!indicator_vwap.initialized());
        assert!(!indicator_vwap.has_inputs());
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_vwap: VolumeWeightedAveragePrice) {
        indicator_vwap.update_raw(10.0, 10.0, 10.0);
        assert_eq!(indicator_vwap.value, 10.0);
    }

    #[rstest]
    fn test_value_with_three_inputs_on_the_same_day(
        mut indicator_vwap: VolumeWeightedAveragePrice,
    ) {
        indicator_vwap.update_raw(10.0, 10.0, 10.0);
        indicator_vwap.update_raw(20.0, 20.0, 10.0);
        indicator_vwap.update_raw(30.0, 30.0, 10.0);
        assert_eq!(indicator_vwap.value, 23.333_333_333_333_332);
    }

    #[rstest]
    fn test_value_with_three_inputs_on_different_days(
        mut indicator_vwap: VolumeWeightedAveragePrice,
    ) {
        indicator_vwap.update_raw(10.0, 10.0, 10.0);
        indicator_vwap.update_raw(20.0, 20.0, 20.0);
        indicator_vwap.update_raw(30.0, 30.0, 10.0);
        assert_eq!(indicator_vwap.value, 30.0);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut indicator_vwap: VolumeWeightedAveragePrice) {
        indicator_vwap.update_raw(1.00000, 1.00000, 10.0);
        indicator_vwap.update_raw(1.00010, 2.00000, 10.0);
        indicator_vwap.update_raw(1.00020, 3.00000, 10.0);
        indicator_vwap.update_raw(1.00030, 1.00000, 10.0);
        indicator_vwap.update_raw(1.00040, 2.00000, 10.0);
        indicator_vwap.update_raw(1.00050, 3.00000, 10.0);
        indicator_vwap.update_raw(1.00040, 1.00000, 10.0);
        indicator_vwap.update_raw(1.00030, 2.00000, 10.0);
        indicator_vwap.update_raw(1.00020, 3.00000, 10.0);
        indicator_vwap.update_raw(1.00010, 1.00000, 10.0);
        indicator_vwap.update_raw(1.00000, 2.00000, 10.0);
        assert_eq!(indicator_vwap.value, 1.000_242_857_142_857);
    }

    #[rstest]
    fn test_handle_bar(
        mut indicator_vwap: VolumeWeightedAveragePrice,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_vwap.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(indicator_vwap.value, 1522.333333333333);
        assert!(indicator_vwap.initialized);
    }

    #[rstest]
    fn test_reset(mut indicator_vwap: VolumeWeightedAveragePrice) {
        indicator_vwap.update_raw(10.0, 10.0, 10.0);
        assert_eq!(indicator_vwap.value, 10.0);
        indicator_vwap.reset();
        assert_eq!(indicator_vwap.value, 0.0);
        assert!(!indicator_vwap.has_inputs);
        assert!(!indicator_vwap.initialized);
    }
}
