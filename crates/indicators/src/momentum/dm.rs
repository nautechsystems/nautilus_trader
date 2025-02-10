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

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct DirectionalMovement {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub pos: f64,
    pub neg: f64,
    pub initialized: bool,
    pos_ma: Box<dyn MovingAverage + Send + 'static>,
    neg_ma: Box<dyn MovingAverage + Send + 'static>,
    has_inputs: bool,
    previous_high: f64,
    previous_low: f64,
}

impl Display for DirectionalMovement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{})", self.name(), self.period, self.ma_type,)
    }
}

impl Indicator for DirectionalMovement {
    fn name(&self) -> String {
        stringify!(DirectionalMovement).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into());
    }

    fn reset(&mut self) {
        self.pos_ma.reset();
        self.neg_ma.reset();
        self.previous_high = 0.0;
        self.previous_low = 0.0;
        self.pos = 0.0;
        self.neg = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl DirectionalMovement {
    /// Creates a new [`DirectionalMovement`] instance.
    #[must_use]
    pub fn new(period: usize, ma_type: Option<MovingAverageType>) -> Self {
        Self {
            period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            pos: 0.0,
            neg: 0.0,
            previous_high: 0.0,
            previous_low: 0.0,
            pos_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Simple),
                period,
            ),
            neg_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Simple),
                period,
            ),
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64) {
        if !self.has_inputs {
            self.previous_high = high;
            self.previous_low = low;
        }

        let up = high - self.previous_high;
        let dn = self.previous_low - low;

        self.pos_ma
            .update_raw(if up > dn && up > 0.0 { up } else { 0.0 });
        self.neg_ma
            .update_raw(if dn > up && dn > 0.0 { dn } else { 0.0 });
        self.pos = self.pos_ma.value();
        self.neg = self.neg_ma.value();

        self.previous_high = high;
        self.previous_low = low;

        // Initialization logic
        if !self.initialized {
            self.has_inputs = true;
            if self.neg_ma.initialized() {
                self.initialized = true;
            }
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
    use crate::stubs::dm_10;

    #[rstest]
    fn test_name_returns_expected_string(dm_10: DirectionalMovement) {
        assert_eq!(dm_10.name(), "DirectionalMovement");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(dm_10: DirectionalMovement) {
        assert_eq!(format!("{dm_10}"), "DirectionalMovement(10,SIMPLE)");
    }

    #[rstest]
    fn test_period_returns_expected_value(dm_10: DirectionalMovement) {
        assert_eq!(dm_10.period, 10);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(dm_10: DirectionalMovement) {
        assert!(!dm_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut dm_10: DirectionalMovement) {
        let high_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let low_values = [
            0.9, 1.9, 2.9, 3.9, 4.9, 5.9, 6.9, 7.9, 8.9, 9.9, 10.1, 10.2, 10.3, 11.1, 11.4,
        ];

        for i in 0..15 {
            dm_10.update_raw(high_values[i], low_values[i]);
        }

        assert!(dm_10.initialized());
        assert_eq!(dm_10.pos, 1.0);
        assert_eq!(dm_10.neg, 0.0);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut dm_10: DirectionalMovement) {
        dm_10.update_raw(1.00020, 1.00050);
        dm_10.update_raw(1.00030, 1.00060);
        dm_10.update_raw(1.00070, 1.00080);

        dm_10.reset();

        assert!(!dm_10.initialized());
        assert_eq!(dm_10.pos, 0.0);
        assert_eq!(dm_10.neg, 0.0);
        assert_eq!(dm_10.previous_high, 0.0);
        assert_eq!(dm_10.previous_low, 0.0);
    }
}
