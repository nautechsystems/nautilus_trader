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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

use std::{collections::HashMap, sync::Arc};

use nautilus_indicators::indicator::Indicator;
use nautilus_model::{data::BarType, identifiers::InstrumentId};

/// Contains all indicator-related references.
#[derive(Default)]
pub(crate) struct Indicators {
    pub indicators: Vec<Arc<dyn Indicator>>,
    pub indicators_for_quotes: HashMap<InstrumentId, Vec<Arc<dyn Indicator>>>,
    pub indicators_for_trades: HashMap<InstrumentId, Vec<Arc<dyn Indicator>>>,
    pub indicators_for_bars: HashMap<BarType, Vec<Arc<dyn Indicator>>>,
}

impl Indicators {
    /// Checks if all registered indicators are initialized.
    pub fn is_initialized(&self) -> bool {
        if self.indicators.is_empty() {
            return false;
        }

        self.indicators
            .iter()
            .all(|indicator| indicator.initialized())
    }

    /// Register an indicator to receive quote ticks for the given instrument ID.
    pub fn register_indicator_for_quotes(
        &mut self,
        instrument_id: InstrumentId,
        indicator: Arc<dyn Indicator>,
    ) {
        // Add to overall indicators if not already present
        if !self.indicators.iter().any(|i| Arc::ptr_eq(i, &indicator)) {
            self.indicators.push(indicator.clone());
        }

        // Add to instrument-specific quotes indicators
        let indicators = self.indicators_for_quotes.entry(instrument_id).or_default();

        if !indicators.iter().any(|i| Arc::ptr_eq(i, &indicator)) {
            indicators.push(indicator);
            // TODO: Log registration
        } else {
            // TODO: Log error - already registered
        }
    }

    /// Register an indicator to receive trade ticks for the given instrument ID.
    pub fn register_indicator_for_trades(
        &mut self,
        instrument_id: InstrumentId,
        indicator: Arc<dyn Indicator>,
    ) {
        // Add to overall indicators if not already present
        if !self.indicators.iter().any(|i| Arc::ptr_eq(i, &indicator)) {
            self.indicators.push(indicator.clone());
        }

        // Add to instrument-specific trades indicators
        let indicators = self.indicators_for_trades.entry(instrument_id).or_default();

        if !indicators.iter().any(|i| Arc::ptr_eq(i, &indicator)) {
            indicators.push(indicator);
            // TODO: Log registration
        } else {
            // TODO: Log error - already registered
        }
    }

    /// Register an indicator to receive bar data for the given bar type.
    pub fn register_indicator_for_bars(
        &mut self,
        bar_type: BarType,
        indicator: Arc<dyn Indicator>,
    ) {
        // Add to overall indicators if not already present
        if !self.indicators.iter().any(|i| Arc::ptr_eq(i, &indicator)) {
            self.indicators.push(indicator.clone());
        }

        // Get standard bar type
        let standard_bar_type = bar_type.standard();

        // Add to bar type-specific indicators
        let indicators = self
            .indicators_for_bars
            .entry(standard_bar_type)
            .or_default();

        if !indicators.iter().any(|i| Arc::ptr_eq(i, &indicator)) {
            indicators.push(indicator);
            // TODO: Log registration
        } else {
            // TODO: Log error - already registered
        }
    }
}
