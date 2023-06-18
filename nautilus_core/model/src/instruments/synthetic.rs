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

use std::collections::HashMap;

use anyhow;
use evalexpr::{ContextWithMutableVariables, HashMapContext, Node, Value};

use crate::{
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
    types::price::Price,
};

pub const SYNTHETIC_VENUE: &str = "SYNTH";

/// Represents a synthetic instrument with prices derived from component instruments using a
/// formula.
pub struct SyntheticInstrument {
    pub id: InstrumentId,
    pub precision: u8,
    pub components: Vec<InstrumentId>,
    pub formula: String,
    pub variables: Vec<String>,
    pub context: HashMapContext,
    operator_tree: Node,
}

impl SyntheticInstrument {
    pub fn new(
        symbol: Symbol,
        precision: u8,
        components: Vec<InstrumentId>,
        formula: String,
    ) -> Result<Self, anyhow::Error> {
        let context = HashMapContext::new();

        // Extract variables from the component instruments
        let variables: Vec<String> = components
            .iter()
            .map(|component| component.to_string())
            .collect();

        let operator_tree = evalexpr::build_operator_tree(&formula)?;

        Ok(SyntheticInstrument {
            id: InstrumentId::new(symbol, Venue::new(SYNTHETIC_VENUE)),
            precision,
            components,
            formula,
            variables,
            context,
            operator_tree,
        })
    }

    pub fn is_valid_formula(&self, formula: &str) -> bool {
        evalexpr::build_operator_tree(formula).is_ok()
    }

    pub fn change_formula(&mut self, formula: String) -> Result<(), anyhow::Error> {
        let operator_tree = evalexpr::build_operator_tree(&formula)?;
        self.formula = formula;
        self.operator_tree = operator_tree;
        Ok(())
    }

    /// Calculates the price of the synthetic instrument based on the given component input prices
    /// provided as a map.
    #[allow(dead_code)]
    pub fn calculate_from_map(
        &mut self,
        inputs: &HashMap<String, f64>,
    ) -> Result<Price, anyhow::Error> {
        let mut input_values = Vec::new();

        for variable in &self.variables {
            if let Some(&value) = inputs.get(variable) {
                input_values.push(value);
                self.context
                    .set_value(variable.clone(), Value::from(value))?;
            } else {
                panic!("Missing price for component: {}", variable);
            }
        }

        self.calculate(&input_values)
    }

    /// Calculates the price of the synthetic instrument based on the given component input prices
    /// provided as an array of `f64` values.
    pub fn calculate(&mut self, inputs: &[f64]) -> Result<Price, anyhow::Error> {
        if inputs.len() != self.variables.len() {
            return Err(anyhow::anyhow!("Invalid number of input values"));
        }

        for (variable, input) in self.variables.iter().zip(inputs) {
            self.context
                .set_value(variable.clone(), Value::from(*input))?;
        }

        let result: Value = self.operator_tree.eval_with_context(&self.context)?;

        match result {
            Value::Float(price) => Ok(Price::new(price, self.precision)),
            _ => Err(anyhow::anyhow!(
                "Failed to evaluate formula to a floating point number"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::identifiers::{instrument_id::InstrumentId, symbol::Symbol};

    #[test]
    fn test_calculate_from_map() {
        let btc_binance = InstrumentId::from_str("BTC.BINANCE").unwrap();
        let ltc_binance = InstrumentId::from_str("LTC.BINANCE").unwrap();
        let formula = "(BTC.BINANCE + LTC.BINANCE) / 2".to_string();
        let mut synth = SyntheticInstrument::new(
            Symbol::new("BTC-LTC"),
            2,
            vec![btc_binance.clone(), ltc_binance],
            formula.clone(),
        )
        .unwrap();

        let mut inputs = HashMap::new();
        inputs.insert("BTC.BINANCE".to_string(), 100.0);
        inputs.insert("LTC.BINANCE".to_string(), 200.0);

        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price.as_f64(), 150.0);
        assert_eq!(synth.formula, formula);
    }

    #[test]
    fn test_calculate() {
        let btc_binance = InstrumentId::from_str("BTC.BINANCE").unwrap();
        let ltc_binance = InstrumentId::from_str("LTC.BINANCE").unwrap();
        let formula = "(BTC.BINANCE + LTC.BINANCE) / 2.0".to_string();
        let mut synth = SyntheticInstrument::new(
            Symbol::new("BTC-LTC"),
            2,
            vec![btc_binance.clone(), ltc_binance],
            formula.clone(),
        )
        .unwrap();

        let inputs = vec![100.0, 200.0];
        let price = synth.calculate(&inputs).unwrap();

        assert_eq!(price.as_f64(), 150.0);
        assert_eq!(synth.formula, formula);
    }

    #[test]
    fn test_change_formula() {
        let btc_binance = InstrumentId::from_str("BTC.BINANCE").unwrap();
        let ltc_binance = InstrumentId::from_str("LTC.BINANCE").unwrap();
        let formula = "(BTC.BINANCE + LTC.BINANCE) / 2".to_string();
        let mut synth = SyntheticInstrument::new(
            Symbol::new("BTC-LTC"),
            2,
            vec![btc_binance, ltc_binance],
            formula.clone(),
        )
        .unwrap();

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
