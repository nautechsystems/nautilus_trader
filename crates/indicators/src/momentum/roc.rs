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

use std::{collections::VecDeque, fmt::Display};

use nautilus_core::correctness::FAILED;
use nautilus_model::data::{Bar, QuoteTick, TradeTick};

use crate::{indicator::Indicator, support::MAX_PERIOD};

/// Output convention for [`RateOfChange`].
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.indicators",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.indicators")
)]
pub enum RateOfChangeMode {
    /// Percentage change: `100 * (current - previous) / previous`.
    #[default]
    Percentage,
    /// Fractional change: `(current - previous) / previous`.
    Fraction,
    /// Price ratio: `current / previous`.
    Ratio,
    /// Price ratio scaled by 100.
    RatioPercent,
    /// Natural logarithm of the price ratio.
    Log,
}

/// Rate of change with configurable output units.
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
pub struct RateOfChange {
    pub period: usize,
    pub use_log: bool,
    pub mode: RateOfChangeMode,
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    prices: VecDeque<f64>,
}

impl Display for RateOfChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for RateOfChange {
    fn name(&self) -> String {
        stringify!(RateOfChange).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        Ok(())
    }

    fn handle_trade(&mut self, _trade: &TradeTick) {}

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.prices.clear();
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl RateOfChange {
    /// Creates a new [`RateOfChange`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is outside `1..=MAX_PERIOD`.
    #[must_use]
    pub fn new(period: usize, use_log: Option<bool>) -> Self {
        let mode = if use_log.unwrap_or(false) {
            RateOfChangeMode::Log
        } else {
            RateOfChangeMode::Percentage
        };
        Self::new_checked(period, mode).expect(FAILED)
    }

    pub(crate) fn new_checked(period: usize, mode: RateOfChangeMode) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=MAX_PERIOD).contains(&period),
            "period must be in 1..={MAX_PERIOD}"
        );
        Ok(Self {
            period,
            use_log: mode == RateOfChangeMode::Log,
            mode,
            value: 0.0,
            prices: VecDeque::with_capacity(period + 1),
            has_inputs: false,
            initialized: false,
        })
    }

    /// Creates a new [`RateOfChange`] with an explicit output convention.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero or exceeds 16,777,216.
    #[must_use]
    pub fn new_with_mode(period: usize, mode: RateOfChangeMode) -> Self {
        Self::new_checked(period, mode).expect(FAILED)
    }

    pub fn update_raw(&mut self, price: f64) {
        if !price.is_finite() || (self.mode == RateOfChangeMode::Log && price <= 0.0) {
            return;
        }
        // The window holds `period + 1` prices so the front is the price exactly
        // `period` updates ago (the standard ROC lookback).
        if self.prices.len() == self.period + 1 {
            let _ = self.prices.pop_front();
        }
        self.prices.push_back(price);

        if !self.initialized {
            self.has_inputs = true;

            if self.prices.len() > self.period {
                self.initialized = true;
            }
        }

        if !self.initialized {
            return;
        }

        if let Some(first) = self.prices.front() {
            if *first == 0.0 {
                self.value = 0.0;
                return;
            }
            let ratio = price / first;
            self.value = match self.mode {
                RateOfChangeMode::Percentage => 100.0 * (ratio - 1.0),
                RateOfChangeMode::Fraction => ratio - 1.0,
                RateOfChangeMode::Ratio => ratio,
                RateOfChangeMode::RatioPercent => 100.0 * ratio,
                RateOfChangeMode::Log => (price / first).ln(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{stubs::roc_10, testing::assert_approx_equal};

    #[rstest]
    fn test_name_returns_expected_string(roc_10: RateOfChange) {
        assert_eq!(roc_10.name(), "RateOfChange");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(roc_10: RateOfChange) {
        assert_eq!(format!("{roc_10}"), "RateOfChange(10)");
    }

    #[rstest]
    fn test_period_returns_expected_value(roc_10: RateOfChange) {
        assert_eq!(roc_10.period, 10);
        assert!(roc_10.use_log);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(roc_10: RateOfChange) {
        assert!(!roc_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut roc_10: RateOfChange) {
        let close_values = [
            0.95, 1.95, 2.95, 3.95, 4.95, 5.95, 6.95, 7.95, 8.95, 9.95, 10.05, 10.15, 10.25, 11.05,
            11.45,
        ];

        for close in &close_values {
            roc_10.update_raw(*close);
        }

        assert!(roc_10.initialized());
        // ln(11.45 / 4.95): the price exactly 10 updates back is 4.95
        assert_approx_equal(roc_10.value, 0.838_602_153_419_649_5);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut roc_10: RateOfChange) {
        roc_10.update_raw(1.00020);
        roc_10.update_raw(1.00030);
        roc_10.update_raw(1.00070);

        roc_10.reset();

        assert!(!roc_10.initialized());
        assert!(!roc_10.has_inputs);
        assert_eq!(roc_10.value, 0.0);
    }

    #[rstest]
    fn test_value_respects_period_window() {
        let mut roc = RateOfChange::new(3, Some(false));

        roc.update_raw(100.0);
        roc.update_raw(1.0);
        roc.update_raw(2.0);
        roc.update_raw(3.0);
        roc.update_raw(4.0);

        // Lookback is exactly `period` = 3 updates: 4.0 against 1.0
        assert_eq!(roc.value, 300.0);
    }
}
