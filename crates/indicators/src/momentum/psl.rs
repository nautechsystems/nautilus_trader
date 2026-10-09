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
};

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
        write!(f, "{}({},{})", self.name(), self.period, self.ma_type)
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
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero or exceeds the supported indicator period limit.
    #[must_use]
    pub fn new(period: usize, ma_type: Option<MovingAverageType>) -> Self {
        Self::new_checked(period, ma_type).expect(FAILED)
    }

    pub(crate) fn new_checked(
        period: usize,
        ma_type: Option<MovingAverageType>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=MAX_PERIOD).contains(&period),
            "period must be in [1, {MAX_PERIOD}]"
        );
        Ok(Self {
            period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            value: 0.0,
            previous_close: 0.0,
            ma: MovingAverageFactory::create(ma_type.unwrap_or(MovingAverageType::Simple), period),
            has_inputs: false,
            initialized: false,
            diff: 0.0,
        })
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

#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::rstest;

    use super::{MAX_PERIOD, MovingAverageType};
    use crate::{
        indicator::Indicator,
        momentum::psl::PsychologicalLine,
        stubs::{bar_ethusdt_binance_minute_bid, psl_10},
    };

    #[rstest]
    #[case(0)]
    #[case(MAX_PERIOD + 1)]
    #[case(usize::MAX)]
    fn test_checked_constructor_rejects_invalid_periods(
        #[case] period: usize,
        #[values(
            MovingAverageType::Simple,
            MovingAverageType::Exponential,
            MovingAverageType::DoubleExponential,
            MovingAverageType::Wilder,
            MovingAverageType::Hull
        )]
        ma_type: MovingAverageType,
    ) {
        let error = PsychologicalLine::new_checked(period, Some(ma_type)).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("period must be in [1, {MAX_PERIOD}]")
        );
    }

    #[rstest]
    fn test_checked_constructor_accepts_maximum_period() {
        let ind = PsychologicalLine::new_checked(MAX_PERIOD, Some(MovingAverageType::Exponential))
            .unwrap();
        assert_eq!(ind.period, MAX_PERIOD);
        assert_eq!(ind.ma_type, MovingAverageType::Exponential);
        assert_eq!(ind.value, 0.0);
        assert!(!ind.initialized());
        assert!(!ind.has_inputs());
    }

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
        assert_eq!(psl_10.value, 0.0);
        assert!(!psl_10.initialized());
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
