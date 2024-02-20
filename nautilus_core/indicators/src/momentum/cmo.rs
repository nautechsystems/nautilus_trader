// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use anyhow::Result;
use nautilus_model::data::{bar::Bar, quote::QuoteTick, trade::TradeTick};
use pyo3::prelude::*;

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

#[repr(C)]
#[derive(Debug)]
#[pyclass(module = "nautilus_trader.core.nautilus.pyo3.indicators")]
pub struct ChandeMomentumOscillator {
    pub period: usize,
    pub average_gain: Box<dyn MovingAverage + Send + 'static>,
    pub average_loss: Box<dyn MovingAverage + Send + 'static>,
    pub previous_close: f64,
    pub value: f64,
    pub count: usize,
    pub is_initialized: bool,
    has_inputs: bool,
}

impl Display for ChandeMomentumOscillator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for ChandeMomentumOscillator {
    fn name(&self) -> String {
        stringify!(ChandeMomentumOscillator).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn is_initialized(&self) -> bool {
        self.is_initialized
    }

    fn handle_quote_tick(&mut self, _tick: &QuoteTick) {
        // Function body intentionally left blank.
    }

    fn handle_trade_tick(&mut self, _tick: &TradeTick) {
        // Function body intentionally left blank.
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.is_initialized = false;
        self.previous_close = 0.0;
    }
}

impl ChandeMomentumOscillator {
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self {
            period,
            average_gain: MovingAverageFactory::create(MovingAverageType::Wilder, period),
            average_loss: MovingAverageFactory::create(MovingAverageType::Wilder, period),
            previous_close: 0.0,
            value: 0.0,
            count: 0,
            is_initialized: false,
            has_inputs: false,
        })
    }

    pub fn update_raw(&mut self, close: f64) {
        if !self.has_inputs {
            self.previous_close = close;
            self.has_inputs = true;
        }

        let gain: f64 = close - self.previous_close;
        if gain > 0.0 {
            self.average_gain.update_raw(gain);
            self.average_loss.update_raw(0.0);
        } else if gain < 0.0 {
            self.average_gain.update_raw(0.0);
            self.average_loss.update_raw(-gain);
        } else {
            self.average_gain.update_raw(0.0);
            self.average_loss.update_raw(0.0);
        }

        if !self.is_initialized
            && self.average_gain.is_initialized()
            && self.average_loss.is_initialized()
        {
            self.is_initialized = true;
        }
        if self.is_initialized {
            self.value = 100.0 * (self.average_gain.value() - self.average_loss.value())
                / (self.average_gain.value() + self.average_loss.value());
        }
        self.previous_close = close;
    }
}
