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

use std::fmt::Display;

use nautilus_model::{data::QuoteTick, identifiers::InstrumentId};

use crate::indicator::Indicator;

/// An indicator which calculates the efficiency ratio across a rolling window.
///
/// The Kaufman Efficiency measures the ratio of the relative market speed in
/// relation to the volatility, this could be thought of as a proxy for noise.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct SpreadAnalyzer {
    pub capacity: usize,
    pub instrument_id: InstrumentId,
    pub current: f64,
    pub average: f64,
    pub initialized: bool,
    has_inputs: bool,
    spreads: Vec<f64>,
}

impl Display for SpreadAnalyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{})",
            self.name(),
            self.capacity,
            self.instrument_id
        )
    }
}

impl Indicator for SpreadAnalyzer {
    fn name(&self) -> String {
        stringify!(SpreadAnalyzer).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }
    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) {
        if quote.instrument_id != self.instrument_id {
            return;
        }

        // Check initialization
        if !self.initialized {
            self.has_inputs = true;
            if self.spreads.len() == self.capacity {
                self.initialized = true;
            }
        }

        let bid: f64 = quote.bid_price.into();
        let ask: f64 = quote.ask_price.into();
        let spread = ask - bid;

        self.current = spread;
        self.spreads.push(spread);

        // Update average spread
        self.average =
            fast_mean_iterated(&self.spreads, spread, self.average, self.capacity, false).unwrap();
    }

    fn reset(&mut self) {
        self.current = 0.0;
        self.average = 0.0;
        self.spreads.clear();
        self.initialized = false;
        self.has_inputs = false;
    }
}

impl SpreadAnalyzer {
    /// Creates a new [`SpreadAnalyzer`] instance.
    #[must_use]
    pub fn new(capacity: usize, instrument_id: InstrumentId) -> Self {
        Self {
            capacity,
            instrument_id,
            current: 0.0,
            average: 0.0,
            initialized: false,
            has_inputs: false,
            spreads: Vec::with_capacity(capacity),
        }
    }
}

fn fast_mean_iterated(
    values: &[f64],
    next_value: f64,
    current_value: f64,
    expected_length: usize,
    drop_left: bool,
) -> Result<f64, &'static str> {
    let length = values.len();

    if length < expected_length {
        return Ok(fast_mean(values));
    }

    if length != expected_length {
        return Err("length of values must equal expected_length");
    }

    let value_to_drop = if drop_left {
        values[0]
    } else {
        values[length - 1]
    };

    Ok(current_value + (next_value - value_to_drop) / length as f64)
}

fn fast_mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {

    use rstest::rstest;

    use crate::{
        indicator::Indicator,
        ratio::spread_analyzer::SpreadAnalyzer,
        stubs::{spread_analyzer_10, *},
    };
    #[rstest]
    fn test_efficiency_ratio_initialized(spread_analyzer_10: SpreadAnalyzer) {
        let display_str = format!("{spread_analyzer_10}");
        assert_eq!(display_str, "SpreadAnalyzer(10,ETHUSDT-PERP.BINANCE)");
        assert_eq!(spread_analyzer_10.capacity, 10);
        assert!(!spread_analyzer_10.initialized);
    }

    #[rstest]
    fn test_with_correct_number_of_required_inputs(mut spread_analyzer_10: SpreadAnalyzer) {
        let bid_price: [&str; 10] = [
            "100.50", "100.45", "100.55", "100.60", "100.52", "100.48", "100.53", "100.57",
            "100.49", "100.51",
        ];

        let ask_price: [&str; 10] = [
            "100.55", "100.50", "100.60", "100.65", "100.57", "100.53", "100.58", "100.62",
            "100.54", "100.56",
        ];
        for i in 1..10 {
            spread_analyzer_10.handle_quote(&stub_quote(bid_price[i], ask_price[i]));
        }
        assert!(!spread_analyzer_10.initialized);
    }

    #[rstest]
    fn test_value_with_one_input(mut spread_analyzer_10: SpreadAnalyzer) {
        spread_analyzer_10.handle_quote(&stub_quote("100.50", "100.55"));
        assert_eq!(spread_analyzer_10.average, 0.049_999_999_999_997_16);
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(
        mut spread_analyzer_10: SpreadAnalyzer,
    ) {
        let bid_price: [&str; 15] = [
            "100.50", "100.45", "100.55", "100.60", "100.52", "100.48", "100.53", "100.57",
            "100.49", "100.51", "100.54", "100.56", "100.58", "100.50", "100.52",
        ];

        let ask_price: [&str; 15] = [
            "100.55", "100.50", "100.60", "100.65", "100.57", "100.53", "100.58", "100.62",
            "100.54", "100.56", "100.59", "100.61", "100.63", "100.55", "100.57",
        ];
        for i in 0..10 {
            spread_analyzer_10.handle_quote(&stub_quote(bid_price[i], ask_price[i]));
        }

        assert_eq!(spread_analyzer_10.average, 0.050_000_000_000_001_9);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(
        mut spread_analyzer_10: SpreadAnalyzer,
    ) {
        spread_analyzer_10.handle_quote(&stub_quote("100.50", "100.55"));
        spread_analyzer_10.reset();
        assert!(!spread_analyzer_10.initialized());
        assert_eq!(spread_analyzer_10.current, 0.0);
        assert_eq!(spread_analyzer_10.average, 0.0);
        assert!(!spread_analyzer_10.has_inputs);
        assert!(!spread_analyzer_10.initialized);
    }
}
