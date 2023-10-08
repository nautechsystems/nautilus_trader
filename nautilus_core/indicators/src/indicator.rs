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

use std::{fmt, fmt::Debug};

use nautilus_model::data::{bar::Bar, quote::QuoteTick, trade::TradeTick};

/// Indicator trait
pub trait Indicator {
    fn name(&self) -> String;
    fn has_inputs(&self) -> bool;
    fn is_initialized(&self) -> bool;
    fn handle_quote_tick(&mut self, tick: &QuoteTick);
    fn handle_trade_tick(&mut self, tick: &TradeTick);
    fn handle_bar(&mut self, bar: &Bar);
    fn reset(&mut self);
}

/// Moving average trait
pub trait MovingAverage: Indicator {
    fn value(&self) -> f64;
    fn count(&self) -> usize;
    fn update_raw(&mut self, value: f64);
}

impl Debug for dyn Indicator + Send {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Implement custom formatting for the Indicator trait object.
        write!(f, "Indicator {{ ... }}")
    }
}
impl Debug for dyn MovingAverage + Send {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Implement custom formatting for the Indicator trait object.
        write!(f, "MovingAverage()")
    }
}
