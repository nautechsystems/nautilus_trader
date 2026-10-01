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

use nautilus_model::data::Bar;

use crate::{
    indicator::Indicator,
    support::{is_valid_hlc, typical_price},
};

/// Volume-weighted average price.
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
pub struct VolumeWeightedAveragePrice {
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    price_volume: f64,
    volume_total: f64,
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
        let (high, low, close) = (bar.high.as_f64(), bar.low.as_f64(), bar.close.as_f64());
        if !is_valid_hlc(high, low, close) {
            return;
        }
        self.update_raw(typical_price(high, low, close), bar.volume.as_f64());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
        self.price_volume = 0.0;
        self.volume_total = 0.0;
    }
}

impl VolumeWeightedAveragePrice {
    /// Creates a new [`VolumeWeightedAveragePrice`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value: 0.0,
            initialized: false,
            has_inputs: false,
            price_volume: 0.0,
            volume_total: 0.0,
        }
    }

    /// Adds a price and nonnegative volume to the current manually reset window.
    /// Non-finite inputs and unrepresentable price-volume products leave state unchanged.
    pub fn update_raw(&mut self, price: f64, volume: f64) {
        let product = price * volume;
        if !price.is_finite() || !volume.is_finite() || volume < 0.0 || !product.is_finite() {
            return;
        }
        self.has_inputs = true;

        if volume == 0.0 {
            return;
        }
        self.price_volume += product;
        self.volume_total += volume;
        self.value = self.price_volume / self.volume_total;
        self.initialized = true;
    }
}

impl Default for VolumeWeightedAveragePrice {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for VolumeWeightedAveragePrice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::rstest;

    use super::*;
    use crate::stubs::*;

    #[rstest]
    fn test_manual_window_and_reset() {
        let mut vwap = VolumeWeightedAveragePrice::new();
        assert_eq!(vwap.to_string(), "VolumeWeightedAveragePrice");
        assert_eq!(
            (vwap.value, vwap.initialized(), vwap.has_inputs()),
            (0.0, false, false)
        );
        vwap.update_raw(99.0, 0.0);
        assert_eq!(
            (vwap.value, vwap.initialized(), vwap.has_inputs()),
            (0.0, false, true)
        );

        for (price, volume) in [(10.0, 1.0), (20.0, 3.0), (30.0, 6.0)] {
            vwap.update_raw(price, volume);
        }
        assert_eq!(
            (vwap.value, vwap.initialized(), vwap.has_inputs()),
            (25.0, true, true)
        );
        vwap.update_raw(999.0, 0.0);
        assert_eq!(vwap.value, 25.0);
        vwap.reset();
        assert_eq!(
            (vwap.value, vwap.initialized(), vwap.has_inputs()),
            (0.0, false, false)
        );
        vwap.update_raw(42.0, 7.0);
        assert_eq!(vwap.value, 42.0);
    }

    #[rstest]
    #[case(f64::NAN, 1.0)]
    #[case(f64::INFINITY, 1.0)]
    #[case(2.0, f64::NAN)]
    #[case(2.0, f64::INFINITY)]
    #[case(2.0, -1.0)]
    #[case(f64::MAX, 2.0)]
    fn test_invalid_input_is_atomic(#[case] price: f64, #[case] volume: f64) {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(price, volume);
        assert_eq!(
            (vwap.value, vwap.initialized(), vwap.has_inputs()),
            (0.0, false, false)
        );
        vwap.update_raw(10.0, 2.0);
        vwap.update_raw(price, volume);
        vwap.update_raw(20.0, 2.0);
        assert_eq!(vwap.value, 15.0);
    }

    #[rstest]
    fn test_bar_uses_typical_price(bar_ethusdt_binance_minute_bid: Bar) {
        let bar = bar_ethusdt_binance_minute_bid;
        let mut event = VolumeWeightedAveragePrice::new();
        let mut raw = VolumeWeightedAveragePrice::new();
        event.handle_bar(&bar);
        raw.update_raw(
            typical_price(bar.high.as_f64(), bar.low.as_f64(), bar.close.as_f64()),
            bar.volume.as_f64(),
        );
        assert_eq!(
            (event.value, event.initialized(), event.has_inputs()),
            (raw.value, true, true)
        );
    }
}
