// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_model::data::bar::Bar;

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]

pub struct Bias {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    _ma: Box<dyn MovingAverage + Send + 'static>,
    _has_inputs: bool,
    _previous_close: f64,
}

impl Display for Bias {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{})", self.name(), self.period, self.ma_type,)
    }
}

impl Indicator for Bias {
    fn name(&self) -> String {
        stringify!(Bias).to_string()
    }

    fn has_inputs(&self) -> bool {
        self._has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self._previous_close = 0.0;
        self.value = 0.0;
        self.count = 0;
        self._has_inputs = false;
        self.initialized = false;
    }
}

impl Bias {
    pub fn new(period: usize, ma_type: Option<MovingAverageType>) -> anyhow::Result<Self> {
        Ok(Self {
            period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            value: 0.0,
            count: 0,
            _previous_close: 0.0,
            _ma: MovingAverageFactory::create(MovingAverageType::Simple, period),
            _has_inputs: false,
            initialized: false,
        })
    }

    pub fn update_raw(&mut self, close: f64) {
        self._ma.update_raw(close);
        self.value = (close / self._ma.value()) - 1.0;
        self._check_initialized();
    }

    pub fn _check_initialized(&mut self) {
        if !self.initialized {
            self._has_inputs = true;
            if self._ma.initialized() {
                self.initialized = true;
            }
        }
    }
}
