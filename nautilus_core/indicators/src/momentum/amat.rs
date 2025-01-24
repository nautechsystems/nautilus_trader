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

use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
};

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
pub struct ArcherMovingAveragesTrends {
    pub fast_period: usize,
    pub slow_period: usize,
    pub signal_period: usize,
    pub ma_type: MovingAverageType,
    pub long_run: bool,
    pub short_run: bool,
    pub initialized: bool,
    fast_ma: Box<dyn MovingAverage + Send + 'static>,
    slow_ma: Box<dyn MovingAverage + Send + 'static>,
    fast_ma_price: VecDeque<f64>,
    slow_ma_price: VecDeque<f64>,
    has_inputs: bool,
}

impl Display for ArcherMovingAveragesTrends {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{},{})",
            self.name(),
            self.fast_period,
            self.slow_period,
            self.signal_period,
            self.ma_type,
        )
    }
}

impl Indicator for ArcherMovingAveragesTrends {
    fn name(&self) -> String {
        stringify!(ArcherMovingAveragesTrends).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.fast_ma.reset();
        self.slow_ma.reset();
        self.long_run = false;
        self.short_run = false;
        self.fast_ma_price.clear();
        self.slow_ma_price.clear();
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl ArcherMovingAveragesTrends {
    /// Creates a new [`ArcherMovingAveragesTrends`] instance.
    #[must_use]
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        signal_period: usize,
        ma_type: Option<MovingAverageType>,
    ) -> Self {
        Self {
            fast_period,
            slow_period,
            signal_period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            long_run: false,
            short_run: false,
            fast_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Simple),
                fast_period,
            ),
            slow_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Simple),
                slow_period,
            ),
            fast_ma_price: VecDeque::with_capacity(signal_period + 1),
            slow_ma_price: VecDeque::with_capacity(signal_period + 1),
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, close: f64) {
        self.fast_ma.update_raw(close);
        self.slow_ma.update_raw(close);

        if self.slow_ma.initialized() {
            self.fast_ma_price.push_back(self.fast_ma.value());
            self.slow_ma_price.push_back(self.slow_ma.value());

            let fast_back = self.fast_ma.value();
            let slow_back = self.slow_ma.value();
            // TODO: Reduce unwraps
            let fast_front = self.fast_ma_price.front().unwrap();
            let slow_front = self.slow_ma_price.front().unwrap();

            self.long_run = fast_back - fast_front > 0.0 && slow_back - slow_front < 0.0;

            self.long_run =
                fast_back - fast_front > 0.0 && slow_back - slow_front > 0.0 || self.long_run;

            self.short_run = fast_back - fast_front < 0.0 && slow_back - slow_front > 0.0;

            self.short_run =
                fast_back - fast_front < 0.0 && slow_back - slow_front < 0.0 || self.short_run;
        }

        // Initialization logic
        if !self.initialized {
            self.has_inputs = true;
            if self.slow_ma_price.len() > self.signal_period && self.slow_ma.initialized() {
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
    use crate::stubs::amat_345;

    #[rstest]
    fn test_name_returns_expected_string(amat_345: ArcherMovingAveragesTrends) {
        assert_eq!(amat_345.name(), "ArcherMovingAveragesTrends");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(amat_345: ArcherMovingAveragesTrends) {
        assert_eq!(
            format!("{amat_345}"),
            "ArcherMovingAveragesTrends(3,4,5,SIMPLE)"
        );
    }

    #[rstest]
    fn test_period_returns_expected_value(amat_345: ArcherMovingAveragesTrends) {
        assert_eq!(amat_345.fast_period, 3);
        assert_eq!(amat_345.slow_period, 4);
        assert_eq!(amat_345.signal_period, 5);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(amat_345: ArcherMovingAveragesTrends) {
        assert!(!amat_345.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(
        mut amat_345: ArcherMovingAveragesTrends,
    ) {
        let closes = [
            0.9, 1.9, 2.9, 3.9, 4.9, 3.2, 6.9, 7.9, 8.9, 9.9, 1.1, 3.2, 10.3, 11.1, 11.4,
        ];

        for close in &closes {
            amat_345.update_raw(*close);
        }

        assert!(amat_345.initialized());
        assert!(amat_345.long_run);
        assert!(!amat_345.short_run);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(
        mut amat_345: ArcherMovingAveragesTrends,
    ) {
        amat_345.update_raw(1.00020);
        amat_345.update_raw(1.00030);
        amat_345.update_raw(1.00070);

        amat_345.reset();

        assert!(!amat_345.initialized());
        assert!(!amat_345.long_run);
        assert!(!amat_345.short_run);
    }
}
