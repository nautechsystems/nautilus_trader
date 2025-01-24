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
pub struct PsychologicalLine {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub value: f64,
    pub initialized: bool,
    ma: Box<dyn MovingAverage + Send + 'static>,
    has_inputs: bool,
    diff: f64,
    previous_close: f64,
}

impl Display for PsychologicalLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{})", self.name(), self.period, self.ma_type,)
    }
}

impl Indicator for PsychologicalLine {
    fn name(&self) -> String {
        stringify!(PsychologicalLine).to_string()
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
        self.ma.reset();
        self.diff = 0.0;
        self.previous_close = 0.0;
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl PsychologicalLine {
    /// Creates a new [`PsychologicalLine`] instance.
    #[must_use]
    pub fn new(period: usize, ma_type: Option<MovingAverageType>) -> Self {
        Self {
            period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            value: 0.0,
            previous_close: 0.0,
            ma: MovingAverageFactory::create(ma_type.unwrap_or(MovingAverageType::Simple), period),
            has_inputs: false,
            initialized: false,
            diff: 0.0,
        }
    }

    pub fn update_raw(&mut self, close: f64) {
        if !self.has_inputs {
            self.previous_close = close;
        }

        self.diff = close - self.previous_close;
        if self.diff <= 0.0 {
            self.ma.update_raw(0.0);
        } else {
            self.ma.update_raw(1.0);
        }
        self.value = 100.0 * self.ma.value();

        if !self.initialized {
            self.has_inputs = true;
            if self.ma.initialized() {
                self.initialized = true;
            }
        }

        self.previous_close = close;
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::rstest;

    use crate::{
        indicator::Indicator,
        momentum::psl::PsychologicalLine,
        stubs::{bar_ethusdt_binance_minute_bid, psl_10},
    };

    #[rstest]
    fn test_psl_initialized(psl_10: PsychologicalLine) {
        let display_str = format!("{psl_10}");
        assert_eq!(display_str, "PsychologicalLine(10,SIMPLE)");
        assert_eq!(psl_10.period, 10);
        assert!(!psl_10.initialized);
        assert!(!psl_10.has_inputs);
    }

    #[rstest]
    fn test_value_with_one_input(mut psl_10: PsychologicalLine) {
        psl_10.update_raw(1.0);
        assert_eq!(psl_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut psl_10: PsychologicalLine) {
        psl_10.update_raw(1.0);
        psl_10.update_raw(2.0);
        psl_10.update_raw(3.0);
        assert_eq!(psl_10.value, 66.666_666_666_666_66);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut psl_10: PsychologicalLine) {
        psl_10.update_raw(1.00000);
        psl_10.update_raw(1.00010);
        psl_10.update_raw(1.00020);
        psl_10.update_raw(1.00030);
        psl_10.update_raw(1.00040);
        psl_10.update_raw(1.00050);
        psl_10.update_raw(1.00040);
        psl_10.update_raw(1.00030);
        psl_10.update_raw(1.00020);
        psl_10.update_raw(1.00010);
        psl_10.update_raw(1.00000);
        assert_eq!(psl_10.value, 50.0);
    }

    #[rstest]
    fn test_initialized_with_required_input(mut psl_10: PsychologicalLine) {
        for i in 1..10 {
            psl_10.update_raw(f64::from(i));
        }
        assert!(!psl_10.initialized);
        psl_10.update_raw(10.0);
        assert!(psl_10.initialized);
    }

    #[rstest]
    fn test_handle_bar(mut psl_10: PsychologicalLine, bar_ethusdt_binance_minute_bid: Bar) {
        psl_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(psl_10.value, 0.0);
        assert!(psl_10.has_inputs);
        assert!(!psl_10.initialized);
    }

    #[rstest]
    fn test_reset(mut psl_10: PsychologicalLine) {
        psl_10.update_raw(1.0);
        psl_10.reset();
        assert_eq!(psl_10.value, 0.0);
        assert_eq!(psl_10.previous_close, 0.0);
        assert_eq!(psl_10.diff, 0.0);
        assert!(!psl_10.has_inputs);
        assert!(!psl_10.initialized);
    }
}
