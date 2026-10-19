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

use crate::indicator::Indicator;

/// On-Balance Volume: Granville's cumulative signed-volume series.
///
/// Each bar adds `+volume`, `-volume`, or `0` depending on whether its close
/// is above, below, or equal to the previous close. The first bar establishes
/// the baseline at `0`.
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
pub struct OnBalanceVolume {
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    previous_close: Option<f64>,
}

impl Display for OnBalanceVolume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}()", self.name())
    }
}

impl Indicator for OnBalanceVolume {
    fn name(&self) -> String {
        stringify!(OnBalanceVolume).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into(), (&bar.volume).into());
    }

    fn reset(&mut self) {
        self.previous_close = None;
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl Default for OnBalanceVolume {
    fn default() -> Self {
        Self::new()
    }
}

impl OnBalanceVolume {
    /// Creates a new [`OnBalanceVolume`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value: 0.0,
            previous_close: None,
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, close: f64, volume: f64) {
        if !close.is_finite() || !volume.is_finite() || volume < 0.0 {
            return;
        }

        if let Some(previous_close) = self.previous_close {
            if close > previous_close {
                self.value += volume;
            } else if close < previous_close {
                self.value -= volume;
            }
        }
        self.previous_close = Some(close);
        self.has_inputs = true;
        self.initialized = true;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_name_returns_expected_string() {
        assert_eq!(OnBalanceVolume::new().name(), "OnBalanceVolume");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string() {
        assert_eq!(format!("{}", OnBalanceVolume::new()), "OnBalanceVolume()");
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false() {
        assert!(!OnBalanceVolume::new().initialized());
    }

    #[rstest]
    fn test_first_bar_establishes_zero_baseline() {
        let mut obv = OnBalanceVolume::new();
        obv.update_raw(100.0, 5_000.0);
        assert!(obv.initialized());
        assert_eq!(obv.value, 0.0);
    }

    #[rstest]
    fn test_cumulative_close_versus_previous_close() {
        let mut obv = OnBalanceVolume::new();
        obv.update_raw(100.0, 1_000.0); // baseline
        obv.update_raw(101.0, 2_000.0); // up: +2000
        obv.update_raw(100.5, 500.0); // down: -500
        obv.update_raw(100.5, 900.0); // flat: 0
        obv.update_raw(102.0, 100.0); // up: +100

        assert_eq!(obv.value, 1_600.0);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state() {
        let mut obv = OnBalanceVolume::new();
        obv.update_raw(100.0, 1_000.0);
        obv.update_raw(101.0, 2_000.0);

        obv.reset();

        assert!(!obv.initialized());
        assert_eq!(obv.value, 0.0);
        assert!(!obv.has_inputs);

        // Post-reset the first bar is a baseline again
        obv.update_raw(50.0, 4_000.0);
        assert_eq!(obv.value, 0.0);
    }
}
