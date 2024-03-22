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

use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
};

use nautilus_model::{
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    enums::PriceType,
};

use crate::indicator::Indicator;

/// The Aroon Oscillator calculates the Aroon Up and Aroon Down indicators to
/// determine if an instrument is trending, and the strength of the trend.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct AroonOscillator {
    pub period: usize,
    pub high_inputs: VecDeque<f64>,
    pub low_inputs: VecDeque<f64>,
    pub aroon_up: f64,
    pub aroon_down: f64,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
}

impl Display for AroonOscillator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for AroonOscillator {
    fn name(&self) -> String {
        stringify!(AroonOscillator).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote_tick(&mut self, tick: &QuoteTick) {
        let price = tick.extract_price(PriceType::Mid).into();
        self.update_raw(price, price);
    }

    fn handle_trade_tick(&mut self, tick: &TradeTick) {
        let price = tick.price.into();
        self.update_raw(price, price);
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into(), (&bar.close).into());
    }

    fn reset(&mut self) {
        self.high_inputs.clear();
        self.low_inputs.clear();
        self.aroon_up = 0.0;
        self.aroon_down = 0.0;
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl AroonOscillator {
    pub fn new(period: usize) -> anyhow::Result<Self> {
        Ok(Self {
            period,
            high_inputs: VecDeque::with_capacity(period),
            low_inputs: VecDeque::with_capacity(period),
            aroon_up: 0.0,
            aroon_down: 0.0,
            value: 0.0,
            count: 0,
            has_inputs: false,
            initialized: false,
        })
    }

    pub fn update_raw(&mut self, high: f64, low: f64) {
        if self.high_inputs.len() == self.period {
            self.high_inputs.pop_back();
        }
        if self.low_inputs.len() == self.period {
            self.low_inputs.pop_back();
        }

        self.high_inputs.push_front(high);
        self.low_inputs.push_front(low);

        self.increment_count();
        if self.initialized {
            // Makes sure we calculate with stable period
            self.calculate_aroon();
        }
    }

    fn calculate_aroon(&mut self) {
        let periods_since_high = self
            .high_inputs
            .iter()
            .enumerate()
            .fold((0, std::f64::MIN), |(max_idx, max_val), (idx, &val)| {
                if val > max_val {
                    (idx, val)
                } else {
                    (max_idx, max_val)
                }
            })
            .0;

        let periods_since_low = self
            .low_inputs
            .iter()
            .enumerate()
            .fold((0, std::f64::MAX), |(min_idx, min_val), (idx, &val)| {
                if val < min_val {
                    (idx, val)
                } else {
                    (min_idx, min_val)
                }
            })
            .0;

        self.aroon_up = 100.0 * ((self.period - periods_since_high) as f64 / self.period as f64);
        self.aroon_down = 100.0 * ((self.period - periods_since_low) as f64 / self.period as f64);
        self.value = self.aroon_up - self.aroon_down;
    }

    fn increment_count(&mut self) {
        self.count += 1;

        if !self.initialized {
            self.has_inputs = true;
            if self.count >= self.period {
                self.initialized = true;
            }
        }
    }
}
