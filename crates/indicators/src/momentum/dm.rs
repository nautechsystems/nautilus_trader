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
    indicator::Indicator,
    support::{MAX_PERIOD, is_valid_high_low},
};

/// Wilder's directional movement, smoothed as a running sum.
///
/// The up-move is `high - previous_high` and the down-move is
/// `previous_low - low`; only the larger of the two contributes, and only when it
/// is positive. The first `period` movements, which start with the second bar,
/// are summed to seed `pos` and `neg`. Later bars update them with
/// `smoothed = smoothed - smoothed / period + movement`, so both values are
/// on the scale of a `period`-bar sum of movements. The first complete output
/// arrives after `period + 1` bars.
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
pub struct DirectionalMovement {
    pub period: usize,
    pub pos: f64,
    pub neg: f64,
    pub initialized: bool,
    has_inputs: bool,
    previous: Option<(f64, f64)>,
    seed_count: usize,
}

impl Display for DirectionalMovement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
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
        self.previous = None;
        self.seed_count = 0;
        self.pos = 0.0;
        self.neg = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl DirectionalMovement {
    /// Creates a new [`DirectionalMovement`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero or exceeds 16,777,216.
    #[must_use]
    pub fn new(period: usize) -> Self {
        Self::new_checked(period).expect(FAILED)
    }

    pub(crate) fn new_checked(period: usize) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=MAX_PERIOD).contains(&period),
            "DirectionalMovement: period must be in 1..={MAX_PERIOD} (received {period})"
        );

        Ok(Self {
            period,
            pos: 0.0,
            neg: 0.0,
            initialized: false,
            has_inputs: false,
            previous: None,
            seed_count: 0,
        })
    }

    pub fn update_raw(&mut self, high: f64, low: f64) {
        if !is_valid_high_low(high, low) {
            return;
        }
        self.has_inputs = true;

        let Some((previous_high, previous_low)) = self.previous.replace((high, low)) else {
            return;
        };

        let up = high - previous_high;
        let down = previous_low - low;
        let pos = if up > down && up > 0.0 { up } else { 0.0 };
        let neg = if down > up && down > 0.0 { down } else { 0.0 };

        if self.initialized {
            let period = self.period as f64;
            self.pos = self.pos - self.pos / period + pos;
            self.neg = self.neg - self.neg / period + neg;
            return;
        }

        self.pos += pos;
        self.neg += neg;
        self.seed_count += 1;
        self.initialized = self.seed_count == self.period;
    }
}

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
        assert_eq!(format!("{dm_10}"), "DirectionalMovement(10)");
    }

    #[rstest]
    fn test_period_returns_expected_value(dm_10: DirectionalMovement) {
        assert_eq!(dm_10.period, 10);
    }

    #[rstest]
    #[case(0)]
    #[case(MAX_PERIOD + 1)]
    fn test_new_checked_rejects_invalid_period(#[case] period: usize) {
        assert!(DirectionalMovement::new_checked(period).is_err());
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(dm_10: DirectionalMovement) {
        assert!(!dm_10.initialized());
    }

    #[rstest]
    fn test_has_inputs_returns_true_after_first_update(mut dm_10: DirectionalMovement) {
        assert!(!dm_10.has_inputs());
        dm_10.update_raw(1.0, 0.9);
        assert!(dm_10.has_inputs());
        assert!(!dm_10.initialized());
    }

    #[rstest]
    fn test_first_output_arrives_after_period_plus_one_bars() {
        let mut dm = DirectionalMovement::new(3);
        for i in 0..3 {
            dm.update_raw(10.0 + f64::from(i), 9.0 + f64::from(i));
            assert!(!dm.initialized());
        }

        dm.update_raw(13.0, 12.0);

        assert!(dm.initialized());
        assert_eq!(dm.pos, 3.0);
        assert_eq!(dm.neg, 0.0);
    }

    #[rstest]
    fn test_seed_is_sum_of_first_period_movements() {
        let mut dm = DirectionalMovement::new(3);
        let bars = [(10.0, 9.0), (12.0, 9.5), (11.0, 8.0), (11.5, 8.5)];
        for (high, low) in bars {
            dm.update_raw(high, low);
        }

        // Movements after the first bar: (+2, 0), (0, 1.5), (0.5, 0)
        assert!(dm.initialized());
        assert_eq!(dm.pos, 2.5);
        assert_eq!(dm.neg, 1.5);
    }

    #[rstest]
    fn test_smoothing_after_seed_decays_by_one_period() {
        let mut dm = DirectionalMovement::new(3);
        for (high, low) in [(10.0, 9.0), (12.0, 9.5), (11.0, 8.0), (11.5, 8.5)] {
            dm.update_raw(high, low);
        }
        dm.update_raw(11.0, 8.6);

        // Movement (0, 0): pos = 2.5 - 2.5 / 3, neg = 1.5 - 1.5 / 3
        assert_eq!(dm.pos, 2.5 - 2.5 / 3.0);
        assert_eq!(dm.neg, 1.5 - 1.5 / 3.0);
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut dm_10: DirectionalMovement) {
        for i in 0..15 {
            dm_10.update_raw(f64::from(i) + 1.0, f64::from(i) + 0.5);
        }

        assert!(dm_10.initialized());
        assert_eq!(dm_10.pos, 10.0);
        assert_eq!(dm_10.neg, 0.0);
    }

    #[rstest]
    fn test_value_with_all_lower_inputs_returns_expected_value(mut dm_10: DirectionalMovement) {
        for i in 0..15 {
            dm_10.update_raw(15.0 - f64::from(i), 14.5 - f64::from(i));
        }

        assert!(dm_10.initialized());
        assert_eq!(dm_10.pos, 0.0);
        assert_eq!(dm_10.neg, 10.0);
    }

    #[rstest]
    #[case(f64::NAN, 1.0)]
    #[case(2.0, f64::INFINITY)]
    #[case(1.0, 2.0)]
    fn test_rejected_input_leaves_state_unchanged(#[case] high: f64, #[case] low: f64) {
        let mut dm = DirectionalMovement::new(2);
        dm.update_raw(10.0, 9.0);
        dm.update_raw(12.0, 9.5);
        dm.update_raw(high, low);
        dm.update_raw(11.0, 8.0);

        // Movements after the first bar: (+2, 0), (0, 1.5)
        assert!(dm.initialized());
        assert_eq!(dm.pos, 2.0);
        assert_eq!(dm.neg, 1.5);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut dm_10: DirectionalMovement) {
        dm_10.update_raw(1.00020, 1.00050);
        dm_10.update_raw(1.00030, 1.00060);
        dm_10.update_raw(1.00070, 1.00080);

        dm_10.reset();

        assert!(!dm_10.initialized());
        assert!(!dm_10.has_inputs());
        assert_eq!(dm_10.pos, 0.0);
        assert_eq!(dm_10.neg, 0.0);
        assert_eq!(dm_10.previous, None);
        assert_eq!(dm_10.seed_count, 0);
    }
}
