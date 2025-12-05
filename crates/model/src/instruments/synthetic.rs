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

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

use derive_builder::Builder;
use evalexpr::{ContextWithMutableVariables, HashMapContext, Node, Value};
use nautilus_core::{UnixNanos, correctness::FAILED};
use serde::{Deserialize, Serialize};

use crate::{
    identifiers::{InstrumentId, Symbol, Venue},
    types::Price,
};

/// Given a formula and component instrument IDs, produce:
///   * a "safe" formula string (with any hyphenated instrument IDs replaced by underscore variants)
///   * the corresponding list of safe variable names (one per component, in order)
///   * a mapping from safe variable names to original instrument ID strings
fn make_safe_formula_with_variables_and_mapping(
    formula: &str,
    components: &[InstrumentId],
) -> (String, Vec<String>, HashMap<String, String>) {
    let mut safe_formula = formula.to_string();
    let mut variables = Vec::with_capacity(components.len());
    let mut safe_to_original = HashMap::new();

    for component in components {
        let original = component.to_string();
        let safe = original.replace('-', "_");
        safe_to_original.insert(safe.clone(), original.clone());
        if original != safe {
            // Replace all occurrences of the instrument ID token with its safe variant.
            safe_formula = safe_formula.replace(&original, &safe);
        }

        variables.push(safe);
    }

    (safe_formula, variables, safe_to_original)
}

/// Represents a synthetic instrument with prices derived from component instruments using a
/// formula.
///
/// The `id` for the synthetic will become `{symbol}.{SYNTH}`.
#[derive(Clone, Debug, Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct SyntheticInstrument {
    /// The unique identifier for the synthetic instrument.
    pub id: InstrumentId,
    /// The price precision for the synthetic instrument.
    pub price_precision: u8,
    /// The minimum price increment.
    pub price_increment: Price,
    /// The component instruments for the synthetic instrument.
    pub components: Vec<InstrumentId>,
    /// The derivation formula for the synthetic instrument.
    ///
    /// NOTE: internally this is always stored in its *safe* form, i.e.
    /// any component `InstrumentId` which contains `-` in its string
    /// representation will appear here with `_` instead.
    pub formula: String,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
    context: HashMapContext,
    variables: Vec<String>,
    safe_to_original: HashMap<String, String>,
    operator_tree: Node,
}

impl Serialize for SyntheticInstrument {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("SyntheticInstrument", 7)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("price_precision", &self.price_precision)?;
        state.serialize_field("price_increment", &self.price_increment)?;
        state.serialize_field("components", &self.components)?;
        state.serialize_field("formula", &self.formula)?;
        state.serialize_field("ts_event", &self.ts_event)?;
        state.serialize_field("ts_init", &self.ts_init)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for SyntheticInstrument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Fields {
            id: InstrumentId,
            price_precision: u8,
            price_increment: Price,
            components: Vec<InstrumentId>,
            formula: String,
            ts_event: UnixNanos,
            ts_init: UnixNanos,
        }

        let fields = Fields::deserialize(deserializer)?;

        let (safe_formula, variables, safe_to_original) =
            make_safe_formula_with_variables_and_mapping(&fields.formula, &fields.components);

        let operator_tree =
            evalexpr::build_operator_tree(&safe_formula).map_err(serde::de::Error::custom)?;

        Ok(Self {
            id: fields.id,
            price_precision: fields.price_precision,
            price_increment: fields.price_increment,
            components: fields.components,
            formula: safe_formula,
            ts_event: fields.ts_event,
            ts_init: fields.ts_init,
            context: HashMapContext::new(),
            variables,
            safe_to_original,
            operator_tree,
        })
    }
}

impl SyntheticInstrument {
    /// Creates a new [`SyntheticInstrument`] instance with correctness checking.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    /// # Errors
    ///
    /// Returns an error if any input validation fails.
    pub fn new_checked(
        symbol: Symbol,
        price_precision: u8,
        components: Vec<InstrumentId>,
        formula: String,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        let price_increment = Price::new(10f64.powi(-i32::from(price_precision)), price_precision);

        // Build a safe version of the formula and the corresponding safe variable names.
        let (safe_formula, variables, safe_to_original) =
            make_safe_formula_with_variables_and_mapping(&formula, &components);
        let operator_tree = evalexpr::build_operator_tree(&safe_formula)?;

        Ok(Self {
            id: InstrumentId::new(symbol, Venue::synthetic()),
            price_precision,
            price_increment,
            components,
            formula: safe_formula,
            context: HashMapContext::new(),
            variables,
            safe_to_original,
            operator_tree,
            ts_event,
            ts_init,
        })
    }

    pub fn is_valid_formula_for_components(formula: &str, components: &[InstrumentId]) -> bool {
        let (safe_formula, _, _) =
            make_safe_formula_with_variables_and_mapping(formula, components);
        evalexpr::build_operator_tree(&safe_formula).is_ok()
    }

    /// Creates a new [`SyntheticInstrument`] instance, parsing the given formula.
    ///
    /// # Panics
    ///
    /// Panics if the provided formula is invalid and cannot be parsed.
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
        Self::is_valid_formula_for_components(formula, &self.components)
    }

    /// # Errors
    ///
    /// Returns an error if parsing the new formula fails.
    pub fn change_formula(&mut self, formula: String) -> anyhow::Result<()> {
        let (safe_formula, _, _) =
            make_safe_formula_with_variables_and_mapping(&formula, &self.components);
        let operator_tree = evalexpr::build_operator_tree(&safe_formula)?;
        self.formula = safe_formula;
        self.operator_tree = operator_tree;
        Ok(())
    }

    /// Calculates the price of the synthetic instrument based on component input prices provided as a map.
    ///
    /// # Errors
    ///
    /// Returns an error if formula evaluation fails, a required component price is missing
    /// from the input map, or if setting the value in the evaluation context fails.
    pub fn calculate_from_map(&mut self, inputs: &HashMap<String, f64>) -> anyhow::Result<Price> {
        let mut input_values = Vec::new();

        for variable in &self.variables {
            let original = self
                .safe_to_original
                .get(variable)
                .ok_or_else(|| anyhow::anyhow!("Variable not found in mapping: {variable}"))?;

            let value = inputs
                .get(original)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("Missing price for component: {original}"))?;

            input_values.push(value);

            self.context
                .set_value(variable.clone(), Value::Float(value))
                .map_err(|e| anyhow::anyhow!("Failed to set value for variable {variable}: {e}"))?;
        }

        self.calculate(&input_values)
    }

    /// Calculates the price of the synthetic instrument based on the given component input prices
    /// provided as an array of `f64` values.
    /// # Errors
    ///
    /// Returns an error if the input length does not match or formula evaluation fails.
    pub fn calculate(&mut self, inputs: &[f64]) -> anyhow::Result<Price> {
        if inputs.len() != self.variables.len() {
            anyhow::bail!("Invalid number of input values");
        }

        for (variable, input) in self.variables.iter().zip(inputs) {
            self.context
                .set_value(variable.clone(), Value::Float(*input))?;
        }

        let result: Value = self.operator_tree.eval_with_context(&self.context)?;

        match result {
            Value::Float(price) => Ok(Price::new(price, self.price_precision)),
            _ => anyhow::bail!("Failed to evaluate formula to a floating point number"),
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
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_calculate_from_map() {
        let mut synth = SyntheticInstrument::default();
        let mut inputs = HashMap::new();
        inputs.insert("BTC.BINANCE".to_string(), 100.0);
        inputs.insert("LTC.BINANCE".to_string(), 200.0);
        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price, Price::from("150.0"));
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
        assert_eq!(price, Price::from("150.0"));
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

        assert_eq!(price, Price::from("75.0"));
        assert_eq!(synth.formula, new_formula);
    }

    #[rstest]
    fn test_hyphenated_instrument_ids_are_sanitized_and_backward_compatible_calculate() {
        let comp1 = InstrumentId::from_str("ETHUSDC-PERP.BINANCE_FUTURES").unwrap();
        let comp2 = InstrumentId::from_str("ETH_USDC-PERP.HYPERLIQUID").unwrap();

        let components = vec![comp1, comp2];

        // External formula uses the *raw* InstrumentId strings with '-'
        let raw_formula = format!("({comp1} + {comp2}) / 2.0");

        let symbol = Symbol::from("ETH-USDC");

        let mut synth = SyntheticInstrument::new(
            symbol,
            2,
            components.clone(),
            raw_formula,
            0.into(),
            0.into(),
        );

        let mut inputs = HashMap::new();
        inputs.insert(components[0].to_string(), 100.0);
        inputs.insert(components[1].to_string(), 200.0);

        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price, Price::from("150.0"));
    }

    #[rstest]
    fn test_hyphenated_instrument_ids_are_sanitized_calculate() {
        let comp1 = InstrumentId::from_str("ETH-USDT-SWAP.OKX").unwrap();
        let comp2 = InstrumentId::from_str("ETH-USDC-PERP.HYPERLIQUID").unwrap();

        let components = vec![comp1, comp2];

        // External formula uses the *raw* InstrumentId strings with '-'
        let raw_formula = format!("({comp1} + {comp2}) / 2.0");

        let symbol = Symbol::from("ETH-USD");

        let mut synth =
            SyntheticInstrument::new(symbol, 2, components, raw_formula, 0.into(), 0.into());

        let inputs = vec![100.0, 200.0];
        let price = synth.calculate(&inputs).unwrap();
        assert_eq!(price, Price::from("150.0"));
    }

    #[rstest]
    fn test_hyphenated_instrument_ids_are_sanitized_calculate_from_map() {
        let comp1 = InstrumentId::from_str("ETH-USDT-SWAP.OKX").unwrap();
        let comp2 = InstrumentId::from_str("ETH-USDC-PERP.HYPERLIQUID").unwrap();

        let components = vec![comp1, comp2];

        // External formula uses the *raw* InstrumentId strings with '-'
        let raw_formula = format!("({comp1} + {comp2}) / 2.0");

        let symbol = Symbol::from("ETH-USD");

        let mut synth = SyntheticInstrument::new(
            symbol,
            2,
            components.clone(),
            raw_formula,
            0.into(),
            0.into(),
        );

        // Internally, the stored formula should NOT contain the hyphenated IDs anymore,
        // but instead the underscore-safe variants.
        for c in &components {
            let original = c.to_string();
            let safe = original.replace('-', "_");

            assert!(
                !synth.formula.contains(&original),
                "internal formula should not contain hyphenated identifier {original}"
            );
            assert!(
                synth.formula.contains(&safe),
                "internal formula should contain safe identifier {safe}"
            );
        }

        // When calling `calculate_from_map`, we should still be able to use
        // the external/original InstrumentId strings as keys.
        let mut inputs = HashMap::new();
        inputs.insert(components[0].to_string(), 100.0);
        inputs.insert(components[1].to_string(), 200.0);

        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price, Price::from("150.0"));
    }
}
