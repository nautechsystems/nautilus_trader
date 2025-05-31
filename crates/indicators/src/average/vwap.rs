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
    day: i64,
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
        self.day = -1;
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
            day: -1,
        }
    }

    pub fn update_raw(&mut self, price: f64, volume: f64, timestamp: f64) {
        const SECONDS_PER_DAY: f64 = 86_400.0;
        let epoch_day = (timestamp / SECONDS_PER_DAY).floor() as i64;

        if epoch_day != self.day {
            self.reset();
            self.day = epoch_day;
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

impl Display for VolumeWeightedAveragePrice {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
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

    const SECONDS_PER_DAY: f64 = 86_400.0;
    const DAY0: f64 = 10.0;
    const DAY1: f64 = SECONDS_PER_DAY;

    #[rstest]
    fn test_vwap_initialized(indicator_vwap: VolumeWeightedAveragePrice) {
        let display_st = format!("{indicator_vwap}");
        assert_eq!(display_st, "VolumeWeightedAveragePrice");
        assert!(!indicator_vwap.initialized());
        assert!(!indicator_vwap.has_inputs());
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_vwap: VolumeWeightedAveragePrice) {
        indicator_vwap.update_raw(10.0, 10.0, DAY0);
        assert_eq!(indicator_vwap.value, 10.0);
    }

    #[rstest]
    fn test_value_with_three_inputs_on_the_same_day(
        mut indicator_vwap: VolumeWeightedAveragePrice,
    ) {
        indicator_vwap.update_raw(10.0, 10.0, DAY0);
        indicator_vwap.update_raw(20.0, 20.0, DAY0 + 1.0);
        indicator_vwap.update_raw(30.0, 30.0, DAY0 + 2.0);
        assert!((indicator_vwap.value - 23.333_333_333_333_332).abs() < 1e-12);
    }

    #[rstest]
    fn test_value_with_three_inputs_on_different_days(
        mut indicator_vwap: VolumeWeightedAveragePrice,
    ) {
        indicator_vwap.update_raw(10.0, 10.0, DAY0);
        indicator_vwap.update_raw(20.0, 20.0, DAY1);
        indicator_vwap.update_raw(30.0, 30.0, DAY0);
        assert_eq!(indicator_vwap.value, 30.0);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut indicator_vwap: VolumeWeightedAveragePrice) {
        for i in 0..10 {
            let price = 0.00010f64.mul_add(f64::from(i), 1.00000);
            let volume = 1.0 + f64::from(i % 3);
            indicator_vwap.update_raw(price, volume, DAY0);
        }
        indicator_vwap.update_raw(1.00000, 2.00000, DAY0);
        assert!((indicator_vwap.value - 1.000_414_285_714_286).abs() < 1e-12);
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
        indicator_vwap.update_raw(10.0, 10.0, DAY0);
        indicator_vwap.reset();
        assert_eq!(indicator_vwap.value, 0.0);
        assert!(!indicator_vwap.has_inputs);
        assert!(!indicator_vwap.initialized);
    }

    #[rstest]
    fn test_reset_on_exact_day_boundary() {
        let mut vwap = VolumeWeightedAveragePrice::new();

        vwap.update_raw(100.0, 5.0, DAY0);
        let old = vwap.value;

        vwap.update_raw(200.0, 5.0, DAY1);
        assert_eq!(vwap.value, 200.0);
        assert_ne!(vwap.value, old);
    }

    #[rstest]
    fn test_no_reset_within_same_day() {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(100.0, 5.0, DAY0);

        vwap.update_raw(200.0, 5.0, DAY0 + 1.0);
        assert!(vwap.value > 100.0 && vwap.value < 200.0);
    }

    #[rstest]
    fn test_zero_volume_does_not_change_value() {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(100.0, 10.0, DAY0);
        let before = vwap.value;

        vwap.update_raw(9999.0, 0.0, DAY0);
        assert_eq!(vwap.value, before);
    }

    #[rstest]
    fn test_epoch_day_floor_rounding() {
        let mut vwap = VolumeWeightedAveragePrice::new();

        vwap.update_raw(50.0, 5.0, DAY1 - 0.000_001);
        let before = vwap.value;

        vwap.update_raw(150.0, 5.0, DAY1);
        assert_eq!(vwap.value, 150.0);
        assert_ne!(vwap.value, before);
    }

    #[rstest]
    fn test_reset_when_timestamp_goes_backwards() {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(10.0, 10.0, DAY0);
        vwap.update_raw(20.0, 10.0, DAY1);
        vwap.update_raw(30.0, 10.0, DAY0);
        assert_eq!(vwap.value, 30.0);
    }

    #[rstest]
    #[case(10.0, 11.0)]
    #[case(43_200.123, 86_399.999)]
    fn test_no_reset_for_same_epoch_day(#[case] t1: f64, #[case] t2: f64) {
        let mut vwap = VolumeWeightedAveragePrice::new();

        vwap.update_raw(100.0, 10.0, t1);
        let before = vwap.value;

        vwap.update_raw(200.0, 10.0, t2);

        assert!(vwap.value > before && vwap.value < 200.0);
    }

    #[rstest]
    #[case(86_399.999, 86_400.0)]
    #[case(86_400.0, 172_800.0)]
    fn test_reset_when_epoch_day_changes(#[case] t1: f64, #[case] t2: f64) {
        let mut vwap = VolumeWeightedAveragePrice::new();

        vwap.update_raw(100.0, 10.0, t1);

        vwap.update_raw(200.0, 10.0, t2);

        assert_eq!(vwap.value, 200.0);
    }

    #[rstest]
    fn test_first_input_zero_volume_does_not_divide_by_zero() {
        let mut vwap = VolumeWeightedAveragePrice::new();

        vwap.update_raw(100.0, 0.0, DAY0);
        assert_eq!(vwap.value, 100.0);
        assert!(vwap.initialized());

        vwap.update_raw(200.0, 10.0, DAY0 + 1.0);
        assert_eq!(vwap.value, 200.0);
    }

    #[rstest]
    fn test_zero_volume_day_rollover_resets_and_seeds() {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(100.0, 10.0, DAY0);

        vwap.update_raw(9999.0, 0.0, DAY1);
        assert_eq!(vwap.value, 9999.0);
    }

    #[rstest]
    fn test_handle_bar_matches_update_raw(
        mut indicator_vwap: VolumeWeightedAveragePrice,
        bar_ethusdt_binance_minute_bid: nautilus_model::data::Bar,
    ) {
        indicator_vwap.handle_bar(&bar_ethusdt_binance_minute_bid);

        let tp = (bar_ethusdt_binance_minute_bid.close.as_f64()
            + bar_ethusdt_binance_minute_bid.high.as_f64()
            + bar_ethusdt_binance_minute_bid.low.as_f64())
            / 3.0;

        let mut vwap_raw = VolumeWeightedAveragePrice::new();
        vwap_raw.update_raw(
            tp,
            (&bar_ethusdt_binance_minute_bid.volume).into(),
            bar_ethusdt_binance_minute_bid.ts_init.as_f64(),
        );

        assert!((indicator_vwap.value - vwap_raw.value).abs() < 1e-12);
    }

    #[rstest]
    #[case(1.0e-9, 1.0e-9)]
    #[case(1.0e9, 1.0e6)]
    #[case(42.4242, 3.1415)]
    fn test_extreme_prices_and_volumes_do_not_overflow(#[case] price: f64, #[case] volume: f64) {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(price, volume, DAY0);
        assert_eq!(vwap.value, price);
    }

    #[rstest]
    fn negative_timestamp() {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(42.0, 1.0, -1.0);
        assert_eq!(vwap.value, 42.0);
        vwap.update_raw(43.0, 1.0, -1.0);
        assert!(vwap.value > 42.0 && vwap.value < 43.0);
    }

    #[rstest]
    fn huge_future_timestamp_saturates() {
        let ts = 1.0e20;
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(1.0, 1.0, ts);
        vwap.update_raw(2.0, 1.0, ts + 1.0);
        assert!(vwap.value > 1.0 && vwap.value < 2.0);
    }

    #[rstest]
    fn negative_volume_changes_sign() {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(100.0, 10.0, 0.0);
        vwap.update_raw(200.0, -10.0, 0.0);
        assert_eq!(vwap.volume_total, 0.0);
    }

    #[rstest]
    fn nan_volume_propagates() {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(100.0, 1.0, 0.0);
        vwap.update_raw(200.0, f64::NAN, 0.0);
        assert!(vwap.value.is_nan());
    }

    #[rstest]
    fn zero_and_negative_price() {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(0.0, 5.0, 0.0);
        assert_eq!(vwap.value, 0.0);
        vwap.update_raw(-10.0, 5.0, 0.0);
        assert!(vwap.value < 0.0);
    }

    #[rstest]
    fn nan_price_propagates() {
        let mut vwap = VolumeWeightedAveragePrice::new();
        vwap.update_raw(f64::NAN, 1.0, 0.0);
        assert!(vwap.value.is_nan());
    }
}
