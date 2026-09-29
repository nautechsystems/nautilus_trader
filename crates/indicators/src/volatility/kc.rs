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

use std::fmt::{Debug, Display};

use nautilus_core::correctness::FAILED;
use nautilus_model::data::Bar;

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
    support::MAX_PERIOD,
    volatility::atr::AverageTrueRange,
};

/// Keltner channel.
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
pub struct KeltnerChannel {
    pub period: usize,
    pub atr_period: usize,
    pub k_multiplier: f64,
    pub ma_type: MovingAverageType,
    pub ma_type_atr: MovingAverageType,
    pub use_previous: bool,
    pub atr_floor: f64,
    pub upper: f64,
    pub middle: f64,
    pub lower: f64,
    pub initialized: bool,
    has_inputs: bool,
    ma: Box<dyn MovingAverage + Send + 'static>,
    atr: AverageTrueRange,
}

impl Display for KeltnerChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for KeltnerChannel {
    fn name(&self) -> String {
        stringify!(KeltnerChannel).to_string()
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
        self.atr.reset();
        self.upper = 0.0;
        self.middle = 0.0;
        self.lower = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl KeltnerChannel {
    /// Creates a new [`KeltnerChannel`] instance.
    ///
    /// # Panics
    ///
    /// Panics if either period is outside `1..=MAX_PERIOD`, the multiplier is not positive and finite,
    /// or the ATR floor is negative or non-finite.
    ///
    /// The defaults are the standard Keltner convention: an EMA centerline
    /// over the typical price and a Wilder smoothed ATR. `atr_period`
    /// defaults to `period` when not given.
    #[must_use]
    pub fn new(
        period: usize,
        k_multiplier: f64,
        atr_period: Option<usize>,
        ma_type: Option<MovingAverageType>,
        ma_type_atr: Option<MovingAverageType>,
        use_previous: Option<bool>,
        atr_floor: Option<f64>,
    ) -> Self {
        Self::new_checked(
            period,
            k_multiplier,
            atr_period,
            ma_type,
            ma_type_atr,
            use_previous,
            atr_floor,
        )
        .expect(FAILED)
    }

    pub(crate) fn new_checked(
        period: usize,
        k_multiplier: f64,
        atr_period: Option<usize>,
        ma_type: Option<MovingAverageType>,
        ma_type_atr: Option<MovingAverageType>,
        use_previous: Option<bool>,
        atr_floor: Option<f64>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(period <= MAX_PERIOD, "period cannot exceed {MAX_PERIOD}");
        let atr_period = atr_period.unwrap_or(period);
        anyhow::ensure!(period > 0, "period must be positive");
        anyhow::ensure!(
            (1..=MAX_PERIOD).contains(&atr_period),
            "atr_period must be in 1..={MAX_PERIOD}"
        );
        anyhow::ensure!(
            k_multiplier.is_finite() && k_multiplier > 0.0,
            "k_multiplier must be finite and positive"
        );
        let atr = AverageTrueRange::new_checked(atr_period, ma_type_atr, use_previous, atr_floor)?;
        Ok(Self {
            period,
            atr_period,
            k_multiplier,
            ma_type: ma_type.unwrap_or(MovingAverageType::Exponential),
            ma_type_atr: ma_type_atr.unwrap_or(MovingAverageType::Wilder),
            use_previous: use_previous.unwrap_or(true),
            atr_floor: atr_floor.unwrap_or(0.0),
            upper: 0.0,
            middle: 0.0,
            lower: 0.0,
            has_inputs: false,
            initialized: false,
            ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Exponential),
                period,
            ),
            atr,
        })
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        let sum = high + low + close;
        let typical_price = if sum.is_finite() {
            sum / 3.0
        } else {
            high / 3.0 + low / 3.0 + close / 3.0
        };

        if !typical_price.is_finite() {
            return;
        }
        let count = self.atr.count;
        self.atr.update_raw(high, low, close);
        if self.atr.count == count {
            return;
        }
        self.ma.update_raw(typical_price);
        self.has_inputs = true;

        if !self.ma.initialized() || !self.atr.initialized {
            return;
        }

        self.upper = self.atr.value.mul_add(self.k_multiplier, self.ma.value());
        self.middle = self.ma.value();
        self.lower = self.atr.value.mul_add(-self.k_multiplier, self.ma.value());

        // Bands are only meaningful once both the centerline MA and the ATR are warm
        if !self.initialized {
            self.has_inputs = true;

            if self.ma.initialized() && self.atr.initialized {
                self.initialized = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{stubs::kc_10, testing::assert_approx_equal};

    #[rstest]
    fn test_name_returns_expected_string(kc_10: KeltnerChannel) {
        assert_eq!(kc_10.name(), "KeltnerChannel");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(kc_10: KeltnerChannel) {
        assert_eq!(format!("{kc_10}"), "KeltnerChannel(10)");
    }

    #[rstest]
    fn test_period_returns_expected_value(kc_10: KeltnerChannel) {
        assert_eq!(kc_10.period, 10);
        assert_eq!(kc_10.k_multiplier, 2.0);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(kc_10: KeltnerChannel) {
        assert!(!kc_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut kc_10: KeltnerChannel) {
        let high_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let low_values = [
            0.9, 1.9, 2.9, 3.9, 4.9, 5.9, 6.9, 7.9, 8.9, 9.9, 10.1, 10.2, 10.3, 11.1, 11.4,
        ];

        let close_values = [
            0.95, 1.95, 2.95, 3.95, 4.95, 5.95, 6.95, 7.95, 8.95, 9.95, 10.5, 11.0, 11.5, 12.5,
            13.0,
        ];

        for i in 0..15 {
            kc_10.update_raw(high_values[i], low_values[i], close_values[i]);
        }

        assert!(kc_10.initialized());
        let middle = (5..15)
            .map(|i| (high_values[i] + low_values[i] + close_values[i]) / 3.0)
            .sum::<f64>()
            / 10.0;
        let atr = (5..15)
            .map(|i| {
                f64::max(high_values[i], close_values[i - 1])
                    - f64::min(low_values[i], close_values[i - 1])
            })
            .sum::<f64>()
            / 10.0;
        assert_approx_equal(kc_10.upper, middle + 2.0 * atr);
        assert_approx_equal(kc_10.middle, middle);
        assert_approx_equal(kc_10.lower, middle - 2.0 * atr);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut kc_10: KeltnerChannel) {
        kc_10.update_raw(1.00020, 1.00050, 1.00030);
        kc_10.update_raw(1.00030, 1.00060, 1.00040);
        kc_10.update_raw(1.00070, 1.00080, 1.00075);

        kc_10.reset();

        assert!(!kc_10.initialized());
        assert!(!kc_10.has_inputs);
        assert_eq!(kc_10.upper, 0.0);
        assert_eq!(kc_10.middle, 0.0);
        assert_eq!(kc_10.lower, 0.0);
    }
}
