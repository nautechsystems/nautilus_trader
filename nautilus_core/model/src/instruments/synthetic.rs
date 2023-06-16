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

use rhai::{Engine, Scope, AST};

use crate::{
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
    types::price::Price,
};

pub const SYNTHETIC_VENUE: &str = "SYNTH";

/// Represents a synthetic instrument with prices derived from component instruments using a
/// formula.
#[derive(Debug)]
pub struct SyntheticInstrument {
    pub id: InstrumentId,
    pub precision: u8,
    pub components: Vec<InstrumentId>,
    pub formula: String,
    pub variables: Vec<String>,
    pub compiled: AST,
    pub engine: Engine,
}

impl SyntheticInstrument {
    pub fn new(
        symbol: Symbol,
        precision: u8,
        components: Vec<InstrumentId>,
        formula: String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let engine = Engine::new();
        let compiled = engine.compile(&formula)?;

        // Extract variables from the component instruments
        let variables: Vec<String> = components
            .iter()
            .map(|component| component.to_string())
            .collect();

        Ok(SyntheticInstrument {
            id: InstrumentId::new(symbol, Venue::new(SYNTHETIC_VENUE)),
            precision,
            components,
            formula,
            variables,
            compiled,
            engine,
        })
    }

    /// Changes the internal derivation formula by recompiling it with a new evaluation engine.
    pub fn change_formula(&mut self, formula: String) -> Result<(), Box<dyn std::error::Error>> {
        self.engine = Engine::new();
        self.compiled = self.engine.compile(&formula)?;
        self.formula = formula;
        Ok(())
    }

    /// Calculates the price of the synthetic instrument based on the given component input values
    /// provided as a map.
    pub fn calculate_from_map(
        &self,
        inputs: &HashMap<String, f64>,
    ) -> Result<Price, Box<dyn std::error::Error>> {
        let input_values: Vec<f64> = self
            .variables
            .iter()
            .map(|variable| {
                if let Some(&value) = inputs.get(variable) {
                    value
                } else {
                    panic!("Missing price for component: {}", variable);
                }
            })
            .collect();

        self.calculate(&input_values)
    }

    /// Calculates the price of the synthetic instrument based on the given component input values
    /// provided as an array of `f64` values.
    pub fn calculate(&self, inputs: &[f64]) -> Result<Price, Box<dyn std::error::Error>> {
        let mut scope = self.create_scope(inputs)?;
        let result: f64 = self
            .engine
            .eval_ast_with_scope(&mut scope, &self.compiled)?;

        Ok(Price::new(result, self.precision))
    }

    // Creates the evaluation scope with input values.
    fn create_scope(&self, inputs: &[f64]) -> Result<Scope, Box<dyn std::error::Error>> {
        if inputs.len() != self.variables.len() {
            return Err("Invalid number of input values".into());
        }

        let mut scope = Scope::new();

        for (variable, input) in self.variables.iter().zip(inputs) {
            scope.push(variable.replace('.', "_"), *input);
        }

        Ok(scope)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::identifiers::symbol::Symbol;

    #[test]
    fn test_calculate_from_map() {
        let btc_binance = InstrumentId::from_str("BTC.BINANCE").unwrap();
        let ltc_binance = InstrumentId::from_str("LTC.BINANCE").unwrap();
        let formula = "(BTC_BINANCE + LTC_BINANCE) / 2".to_string();
        let synth = SyntheticInstrument::new(
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
        let formula = "(BTC_BINANCE + LTC_BINANCE) / 2.0".to_string();
        let synth = SyntheticInstrument::new(
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
        let formula = "(BTC_BINANCE + LTC_BINANCE) / 2".to_string();
        let mut synth = SyntheticInstrument::new(
            Symbol::new("BTC-LTC"),
            2,
            vec![btc_binance.clone(), ltc_binance],
            formula.clone(),
        )
        .unwrap();

        let new_formula = "(BTC_BINANCE + LTC_BINANCE) / 4".to_string();
        synth.change_formula(new_formula.clone()).unwrap();

        let mut inputs = HashMap::new();
        inputs.insert("BTC.BINANCE".to_string(), 100.0);
        inputs.insert("LTC.BINANCE".to_string(), 200.0);

        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price.as_f64(), 75.0);
        assert_eq!(synth.formula, new_formula);
    }
}
