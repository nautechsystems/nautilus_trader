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

use nautilus_model::data::Bar;

use super::kc::KeltnerChannel;
use crate::{average::MovingAverageType, indicator::Indicator};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct KeltnerPosition {
    pub period: usize,
    pub k_multiplier: f64,
    pub ma_type: MovingAverageType,
    pub ma_type_atr: MovingAverageType,
    pub use_previous: bool,
    pub atr_floor: f64,
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    kc: KeltnerChannel,
}

impl Display for KeltnerPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{},{},{})",
            self.name(),
            self.period,
            self.k_multiplier,
            self.ma_type,
            self.ma_type_atr,
            self.use_previous
        )
    }
}

impl Indicator for KeltnerPosition {
    fn name(&self) -> String {
        stringify!(KeltnerPosition).to_string()
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
        self.kc.reset();
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl KeltnerPosition {
    /// Creates a new [`KeltnerPosition`] instance.
    #[must_use]
    pub fn new(
        period: usize,
        k_multiplier: f64,
        ma_type: Option<MovingAverageType>,
        ma_type_atr: Option<MovingAverageType>,
        use_previous: Option<bool>,
        atr_floor: Option<f64>,
    ) -> Self {
        Self {
            period,
            k_multiplier,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            ma_type_atr: ma_type_atr.unwrap_or(MovingAverageType::Simple),
            use_previous: use_previous.unwrap_or(true),
            atr_floor: atr_floor.unwrap_or(0.0),
            value: 0.0,
            has_inputs: false,
            initialized: false,
            kc: KeltnerChannel::new(
                period,
                k_multiplier,
                ma_type,
                ma_type_atr,
                use_previous,
                atr_floor,
            ),
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        self.kc.update_raw(high, low, close);

        // Initialization logic
        if !self.initialized {
            self.has_inputs = true;
            if self.kc.initialized() {
                self.initialized = true;
            }
        }

        let k_width = (self.kc.upper - self.kc.lower) / 2.0;

        if k_width > 0.0 {
            self.value = (close - self.kc.middle) / k_width;
        } else {
            self.value = 0.0;
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::stubs::kp_10;

    #[rstest]
    fn test_name_returns_expected_string(kp_10: KeltnerPosition) {
        assert_eq!(kp_10.name(), "KeltnerPosition");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(kp_10: KeltnerPosition) {
        assert_eq!(
            format!("{kp_10}"),
            "KeltnerPosition(10,2,SIMPLE,SIMPLE,true)"
        );
    }

    #[rstest]
    fn test_period_returns_expected_value(kp_10: KeltnerPosition) {
        assert_eq!(kp_10.period, 10);
        assert_eq!(kp_10.k_multiplier, 2.0);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(kp_10: KeltnerPosition) {
        assert!(!kp_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut kp_10: KeltnerPosition) {
        let high_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let low_values = [
            0.9, 1.9, 2.9, 3.9, 4.9, 5.9, 6.9, 7.9, 8.9, 9.9, 10.1, 10.2, 10.3, 11.1, 11.4,
        ];

        let close_values = [
            0.95, 1.95, 2.95, 3.95, 4.95, 5.95, 6.95, 7.95, 8.95, 9.95, 10.05, 10.15, 10.25, 11.05,
            11.45,
        ];

        for i in 0..15 {
            kp_10.update_raw(high_values[i], low_values[i], close_values[i]);
        }

        assert!(kp_10.initialized());
        assert_eq!(kp_10.value, 0.471_631_205_673_758_94);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut kp_10: KeltnerPosition) {
        kp_10.update_raw(1.00020, 1.00050, 1.00030);
        kp_10.update_raw(1.00030, 1.00060, 1.00040);
        kp_10.update_raw(1.00070, 1.00080, 1.00075);

        kp_10.reset();

        assert!(!kp_10.initialized());
        assert!(!kp_10.has_inputs);
        assert_eq!(kp_10.value, 0.0);
        assert_eq!(kp_10.kc.upper, 0.0);
        assert_eq!(kp_10.kc.middle, 0.0);
        assert_eq!(kp_10.kc.lower, 0.0);
    }
}
