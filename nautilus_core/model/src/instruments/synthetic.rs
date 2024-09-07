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
    collections::HashMap,
    hash::{Hash, Hasher},
};

use derive_builder::Builder;
use evalexpr::{ContextWithMutableVariables, HashMapContext, Node, Value};
use nautilus_core::{correctness::FAILED, nanos::UnixNanos};

use crate::{
    identifiers::{InstrumentId, Symbol, Venue},
    types::price::Price,
};

/// Represents a synthetic instrument with prices derived from component instruments using a
/// formula.
#[derive(Clone, Debug, Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct SyntheticInstrument {
    pub id: InstrumentId,
    pub price_precision: u8,
    pub price_increment: Price,
    pub components: Vec<InstrumentId>,
    pub formula: String,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    context: HashMapContext,
    variables: Vec<String>,
    operator_tree: Node,
}

impl SyntheticInstrument {
    /// Creates a new [`SyntheticInstrument`] instance with correctness checking.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(
        symbol: Symbol,
        price_precision: u8,
        components: Vec<InstrumentId>,
        formula: String,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        let price_increment = Price::new(10f64.powi(-i32::from(price_precision)), price_precision);

        // Extract variables from the component instruments
        let variables: Vec<String> = components
            .iter()
            .map(std::string::ToString::to_string)
            .collect();

        let operator_tree = evalexpr::build_operator_tree(&formula)?;

        Ok(Self {
            id: InstrumentId::new(symbol, Venue::synthetic()),
            price_precision,
            price_increment,
            components,
            formula,
            context: HashMapContext::new(),
            variables,
            operator_tree,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`SyntheticInstrument`] instance
    pub fn new(
        symbol: Symbol,
        price_precision: u8,
        components: Vec<InstrumentId>,
        formula: String,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            symbol,
            price_precision,
            components,
            formula,
            ts_event,
            ts_init,
        )
        .expect(FAILED)
    }

    #[must_use]
    pub fn is_valid_formula(&self, formula: &str) -> bool {
        evalexpr::build_operator_tree(formula).is_ok()
    }

    pub fn change_formula(&mut self, formula: String) -> anyhow::Result<()> {
        let operator_tree = evalexpr::build_operator_tree(&formula)?;
        self.formula = formula;
        self.operator_tree = operator_tree;
        Ok(())
    }

    /// Calculates the price of the synthetic instrument based on the given component input prices
    /// provided as a map.
    #[allow(dead_code)]
    pub fn calculate_from_map(&mut self, inputs: &HashMap<String, f64>) -> anyhow::Result<Price> {
        let mut input_values = Vec::new();

        for variable in &self.variables {
            if let Some(&value) = inputs.get(variable) {
                input_values.push(value);
                self.context
                    .set_value(variable.clone(), Value::from(value))
                    .expect("TODO: Unable to set value");
            } else {
                panic!("Missing price for component: {variable}");
            }
        }

        self.calculate(&input_values)
    }

    /// Calculates the price of the synthetic instrument based on the given component input prices
    /// provided as an array of `f64` values.
    pub fn calculate(&mut self, inputs: &[f64]) -> anyhow::Result<Price> {
        if inputs.len() != self.variables.len() {
            return Err(anyhow::anyhow!("Invalid number of input values"));
        }

        for (variable, input) in self.variables.iter().zip(inputs) {
            self.context
                .set_value(variable.clone(), Value::from(*input))?;
        }

        let result: Value = self.operator_tree.eval_with_context(&self.context)?;

        match result {
            Value::Float(price) => Ok(Price::new(price, self.price_precision)),
            _ => Err(anyhow::anyhow!(
                "Failed to evaluate formula to a floating point number"
            )),
        }
    }
}

impl PartialEq<Self> for SyntheticInstrument {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for SyntheticInstrument {}

impl Hash for SyntheticInstrument {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
///////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_calculate_from_map() {
        let mut synth = SyntheticInstrument::default();
        let mut inputs = HashMap::new();
        inputs.insert("BTC.BINANCE".to_string(), 100.0);
        inputs.insert("LTC.BINANCE".to_string(), 200.0);
        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price.as_f64(), 150.0);
        assert_eq!(
            synth.formula,
            "(BTC.BINANCE + LTC.BINANCE) / 2.0".to_string()
        );
    }

    #[rstest]
    fn test_calculate() {
        let mut synth = SyntheticInstrument::default();
        let inputs = vec![100.0, 200.0];
        let price = synth.calculate(&inputs).unwrap();
        assert_eq!(price.as_f64(), 150.0);
    }

    #[rstest]
    fn test_change_formula() {
        let mut synth = SyntheticInstrument::default();
        let new_formula = "(BTC.BINANCE + LTC.BINANCE) / 4".to_string();
        synth.change_formula(new_formula.clone()).unwrap();

        let mut inputs = HashMap::new();
        inputs.insert("BTC.BINANCE".to_string(), 100.0);
        inputs.insert("LTC.BINANCE".to_string(), 200.0);
        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price.as_f64(), 75.0);
        assert_eq!(synth.formula, new_formula);
    }
}
