// Under development
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

use std::{collections::HashMap, sync::Arc};

use nautilus_indicators::indicator::Indicator;
use nautilus_model::{data::BarType, identifiers::InstrumentId};

/// Contains all indicator-related references.
#[derive(Clone, Default)]
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

        if indicators.iter().any(|i| Arc::ptr_eq(i, &indicator)) {
            // TODO: Log error - already registered
        } else {
            indicators.push(indicator);
            // TODO: Log registration
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

        if indicators.iter().any(|i| Arc::ptr_eq(i, &indicator)) {
            // TODO: Log error - already registered
        } else {
            indicators.push(indicator);
            // TODO: Log registration
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

        if indicators.iter().any(|i| Arc::ptr_eq(i, &indicator)) {
            // TODO: Log error - already registered
        } else {
            indicators.push(indicator);
            // TODO: Log registration
        }
    }
}
