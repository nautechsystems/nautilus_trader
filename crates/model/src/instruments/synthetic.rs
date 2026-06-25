// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
    error::Error,
    hash::{Hash, Hasher},
};

use derive_builder::Builder;
use nautilus_core::{
    UnixNanos,
    correctness::{CorrectnessError, FAILED},
};
use serde::{Deserialize, Serialize};

use crate::{
    expressions::{Bindings, CompiledExpression, ExpressionError, compile_numeric},
    identifiers::{InstrumentId, Symbol, Venue},
    types::Price,
};

const MAX_INLINE_COMPONENTS: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum SyntheticInstrumentError {
    #[error("{0}")]
    Validation(#[from] CorrectnessError),
    #[error("{source}")]
    Expression {
        #[source]
        source: Box<dyn Error + Send + Sync + 'static>,
    },
    #[error("Missing price for component: {component_name}")]
    MissingInput { component_name: String },
    #[error("Expected {expected} input values, received {actual}")]
    InputCountMismatch { expected: usize, actual: usize },
    #[error("Non-finite input price for component {component_name}: {value}")]
    NonFiniteInput { component_name: String, value: f64 },
    #[error("Formula result produced invalid price: {source}")]
    InvalidPriceResult {
        #[source]
        source: CorrectnessError,
    },
}

impl SyntheticInstrumentError {
    fn expression(source: ExpressionError) -> Self {
        Self::Expression {
            source: Box::new(source),
        }
    }
}

/// Represents a synthetic instrument with prices derived from component instruments using a
/// formula.
///
/// The `id` for the synthetic will become `{symbol}.{SYNTH}`.
#[derive(Clone, Debug, Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
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
    pub formula: String,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
    #[builder(setter(skip), default)]
    component_names: Vec<String>,
    #[builder(setter(skip), default)]
    compiled_formula: CompiledExpression,
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
        let component_names = component_names_from_components(&fields.components);
        let compiled_formula =
            compile_formula(&fields.formula, &component_names).map_err(serde::de::Error::custom)?;

        Ok(Self {
            id: fields.id,
            price_precision: fields.price_precision,
            price_increment: fields.price_increment,
            components: fields.components,
            formula: fields.formula,
            ts_event: fields.ts_event,
            ts_init: fields.ts_init,
            component_names,
            compiled_formula,
        })
    }
}

impl SyntheticInstrument {
    /// Creates a new [`SyntheticInstrument`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if input validation or formula compilation fails.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(
        symbol: Symbol,
        price_precision: u8,
        components: Vec<InstrumentId>,
        formula: &str,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Result<Self, SyntheticInstrumentError> {
        let price_increment =
            Price::new_checked(10f64.powi(-i32::from(price_precision)), price_precision)?;
        let component_names = component_names_from_components(&components);
        let compiled_formula = compile_formula(formula, &component_names)?;

        Ok(Self {
            id: InstrumentId::new(symbol, Venue::synthetic()),
            price_precision,
            price_increment,
            components,
            formula: formula.to_string(),
            component_names,
            compiled_formula,
            ts_event,
            ts_init,
        })
    }

    /// Returns whether the given formula compiles against the provided components.
    #[must_use]
    pub fn is_valid_formula_for_components(formula: &str, components: &[InstrumentId]) -> bool {
        let component_names = component_names_from_components(components);
        compile_formula(formula, &component_names).is_ok()
    }

    /// Creates a new [`SyntheticInstrument`] instance, parsing the given formula.
    ///
    /// # Panics
    ///
    /// Panics if the provided formula is invalid and cannot be parsed.
    #[must_use]
    pub fn new(
        symbol: Symbol,
        price_precision: u8,
        components: Vec<InstrumentId>,
        formula: &str,
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
        .unwrap_or_else(|e| panic!("{FAILED}: {e}"))
    }

    /// Returns whether the given formula compiles against this instrument's components.
    #[must_use]
    pub fn is_valid_formula(&self, formula: &str) -> bool {
        Self::is_valid_formula_for_components(formula, &self.components)
    }

    /// Replaces the derivation formula, recompiling it against the existing components.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing the new formula fails.
    pub fn change_formula(&mut self, formula: &str) -> Result<(), SyntheticInstrumentError> {
        let compiled_formula = compile_formula(formula, &self.component_names)?;
        self.formula = formula.to_string();
        self.compiled_formula = compiled_formula;
        Ok(())
    }

    /// Calculates the price of the synthetic instrument based on component input prices provided as a map.
    ///
    /// # Errors
    ///
    /// Returns an error if formula evaluation fails or a required component price is missing from
    /// the input map.
    pub fn calculate_from_map(
        &self,
        inputs: &HashMap<String, f64>,
    ) -> Result<Price, SyntheticInstrumentError> {
        let n = self.component_names.len();
        let mut buf = [0.0_f64; MAX_INLINE_COMPONENTS];
        let input_values: &[f64] = if n <= MAX_INLINE_COMPONENTS {
            for (i, component_name) in self.component_names.iter().enumerate() {
                buf[i] = *inputs.get(component_name).ok_or_else(|| {
                    SyntheticInstrumentError::MissingInput {
                        component_name: component_name.clone(),
                    }
                })?;
            }
            &buf[..n]
        } else {
            // Fallback for large component sets
            let v: Result<Vec<f64>, _> = self
                .component_names
                .iter()
                .map(|name| {
                    inputs.get(name).copied().ok_or_else(|| {
                        SyntheticInstrumentError::MissingInput {
                            component_name: name.clone(),
                        }
                    })
                })
                .collect();
            return self.calculate(&v?);
        };

        self.calculate(input_values)
    }

    /// Calculates the price of the synthetic instrument based on the given component input prices
    /// provided as an array of `f64` values.
    ///
    /// # Errors
    ///
    /// Returns an error if the input length does not match, any input is non-finite, or formula
    /// evaluation fails.
    pub fn calculate(&self, inputs: &[f64]) -> Result<Price, SyntheticInstrumentError> {
        if inputs.len() != self.component_names.len() {
            return Err(SyntheticInstrumentError::InputCountMismatch {
                expected: self.component_names.len(),
                actual: inputs.len(),
            });
        }

        for (i, value) in inputs.iter().enumerate() {
            if !value.is_finite() {
                return Err(SyntheticInstrumentError::NonFiniteInput {
                    component_name: self.component_names[i].clone(),
                    value: *value,
                });
            }
        }

        let price = self
            .compiled_formula
            .eval_number(inputs)
            .map_err(SyntheticInstrumentError::expression)?;
        Price::new_checked(price, self.price_precision)
            .map_err(|source| SyntheticInstrumentError::InvalidPriceResult { source })
    }
}

fn component_names_from_components(components: &[InstrumentId]) -> Vec<String> {
    components.iter().map(ToString::to_string).collect()
}

/// # Errors
///
/// Returns an error if primary component names collide.
fn build_bindings(component_names: &[String]) -> Result<Bindings, SyntheticInstrumentError> {
    let mut bindings = Bindings::new();

    for (slot, component_name) in component_names.iter().enumerate() {
        bindings
            .add(slot, component_name)
            .map_err(SyntheticInstrumentError::expression)?;
    }

    for (slot, component_name) in component_names.iter().enumerate() {
        let legacy_name = component_name.replace('-', "_");

        if legacy_name != *component_name {
            // Best-effort: skip if alias collides with a primary binding
            let _ = bindings.add_alias(slot, &legacy_name);
        }
    }

    Ok(bindings)
}

/// # Errors
///
/// Returns an error if parsing or semantic validation fails.
fn compile_formula(
    formula: &str,
    component_names: &[String],
) -> Result<CompiledExpression, SyntheticInstrumentError> {
    let bindings = build_bindings(component_names)?;
    compile_numeric(formula, &bindings).map_err(SyntheticInstrumentError::expression)
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;
    use crate::types::fixed::FIXED_PRECISION;

    #[rstest]
    fn test_calculate_from_map() {
        let synth = SyntheticInstrument::default();
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
        let synth = SyntheticInstrument::default();
        let inputs = vec![100.0, 200.0];
        let price = synth.calculate(&inputs).unwrap();
        assert_eq!(price, Price::from("150.0"));
    }

    #[rstest]
    fn test_change_formula() {
        let mut synth = SyntheticInstrument::default();
        let new_formula = "(BTC.BINANCE + LTC.BINANCE) / 4";
        synth.change_formula(new_formula).unwrap();

        let mut inputs = HashMap::new();
        inputs.insert("BTC.BINANCE".to_string(), 100.0);
        inputs.insert("LTC.BINANCE".to_string(), 200.0);
        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price, Price::from("75.0"));
        assert_eq!(synth.formula, new_formula);
    }

    #[rstest]
    fn test_hyphenated_instrument_ids_preserve_raw_formula() {
        let comp1 = InstrumentId::from_str("ETHUSDC-PERP.BINANCE_FUTURES").unwrap();
        let comp2 = InstrumentId::from_str("ETH_USDC-PERP.HYPERLIQUID").unwrap();
        let components = vec![comp1, comp2];
        let raw_formula = format!("({comp1} + {comp2}) / 2.0");
        let symbol = Symbol::from("ETH-USDC");
        let synth =
            SyntheticInstrument::new(symbol, 2, components, &raw_formula, 0.into(), 0.into());
        let price = synth.calculate(&[100.0, 200.0]).unwrap();

        assert_eq!(price, Price::from("150.0"));
        assert_eq!(synth.formula, raw_formula);
    }

    #[rstest]
    fn test_hyphenated_instrument_ids_support_legacy_sanitized_formula() {
        let comp1 = InstrumentId::from_str("ETH-USDT-SWAP.OKX").unwrap();
        let comp2 = InstrumentId::from_str("ETH-USDC-PERP.HYPERLIQUID").unwrap();
        let components = vec![comp1, comp2];
        let legacy_formula = format!(
            "({} + {}) / 2.0",
            components[0].to_string().replace('-', "_"),
            components[1].to_string().replace('-', "_"),
        );
        let symbol = Symbol::from("ETH-USD");
        let synth = SyntheticInstrument::new(
            symbol,
            2,
            components.clone(),
            &legacy_formula,
            0.into(),
            0.into(),
        );
        let mut inputs = HashMap::new();
        inputs.insert(components[0].to_string(), 100.0);
        inputs.insert(components[1].to_string(), 200.0);
        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price, Price::from("150.0"));
        assert_eq!(synth.formula, legacy_formula);
    }

    #[rstest]
    fn test_slashed_instrument_ids_calculate_from_map() {
        let comp1 = InstrumentId::from_str("AUD/USD.SIM").unwrap();
        let comp2 = InstrumentId::from_str("NZD/USD.SIM").unwrap();
        let components = vec![comp1, comp2];
        let raw_formula = format!("({} + {}) / 2.0", components[0], components[1]);

        let synth = SyntheticInstrument::new(
            Symbol::from("FX-BASKET"),
            5,
            components.clone(),
            &raw_formula,
            0.into(),
            0.into(),
        );
        let mut inputs = HashMap::new();
        inputs.insert(components[0].to_string(), 0.65001);
        inputs.insert(components[1].to_string(), 0.59001);

        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price, Price::from("0.62001"));
        assert_eq!(synth.formula, raw_formula);
    }

    #[rstest]
    fn test_new_checked_rejects_unknown_formula_symbol_with_expression_error() {
        let components = vec![
            InstrumentId::from_str("BTC.BINANCE").unwrap(),
            InstrumentId::from_str("LTC.BINANCE").unwrap(),
        ];

        let error = SyntheticInstrument::new_checked(
            Symbol::from("BTC-LTC"),
            2,
            components,
            "BTC.BINANCE + missing",
            0.into(),
            0.into(),
        )
        .unwrap_err();

        assert!(matches!(
            &error,
            SyntheticInstrumentError::Expression { .. }
        ));
        assert_eq!(error.to_string(), "Unknown symbol `missing`");
    }

    #[rstest]
    fn test_new_checked_rejects_invalid_precision_with_validation_error() {
        let components = vec![
            InstrumentId::from_str("BTC.BINANCE").unwrap(),
            InstrumentId::from_str("LTC.BINANCE").unwrap(),
        ];

        let error = SyntheticInstrument::new_checked(
            Symbol::from("BTC-LTC"),
            FIXED_PRECISION + 1,
            components,
            "BTC.BINANCE + LTC.BINANCE",
            0.into(),
            0.into(),
        )
        .unwrap_err();

        match &error {
            SyntheticInstrumentError::Validation(CorrectnessError::PredicateViolation {
                message,
            }) => {
                assert!(message.contains("precision"), "{message}");
            }
            _ => panic!("Expected validation error, received {error:?}"),
        }
    }

    #[rstest]
    fn test_deserialize_rejects_unknown_formula_symbol() {
        let synth = SyntheticInstrument::default();
        let payload = serde_json::to_string(&synth).unwrap().replace(
            "\"(BTC.BINANCE + LTC.BINANCE) / 2.0\"",
            "\"BTC.BINANCE + missing\"",
        );

        let error = serde_json::from_str::<SyntheticInstrument>(&payload).unwrap_err();

        assert!(
            error.to_string().contains("Unknown symbol `missing`"),
            "{error}",
        );
    }

    #[rstest]
    fn test_calculate_rejects_wrong_input_count() {
        let synth = SyntheticInstrument::default();
        let error = synth.calculate(&[100.0]).unwrap_err();

        match &error {
            SyntheticInstrumentError::InputCountMismatch { expected, actual } => {
                assert_eq!((*expected, *actual), (2, 1));
            }
            _ => panic!("Expected input count mismatch, received {error:?}"),
        }
        assert_eq!(error.to_string(), "Expected 2 input values, received 1");
    }

    #[rstest]
    fn test_change_formula_rejects_invalid_formula_without_mutation() {
        let mut synth = SyntheticInstrument::default();
        let original_formula = synth.formula.clone();
        let original_price = synth.calculate(&[100.0, 200.0]).unwrap();

        let error = synth.change_formula("BTC.BINANCE + missing").unwrap_err();
        let current_price = synth.calculate(&[100.0, 200.0]).unwrap();

        assert!(matches!(
            &error,
            SyntheticInstrumentError::Expression { .. }
        ));
        assert_eq!(error.to_string(), "Unknown symbol `missing`");
        assert_eq!(synth.formula, original_formula);
        assert_eq!(current_price, original_price);
    }

    #[rstest]
    fn test_calculate_from_map_rejects_missing_component() {
        let synth = SyntheticInstrument::default();
        let mut inputs = HashMap::new();
        inputs.insert("BTC.BINANCE".to_string(), 100.0);

        let error = synth.calculate_from_map(&inputs).unwrap_err();

        match &error {
            SyntheticInstrumentError::MissingInput { component_name } => {
                assert_eq!(component_name, "LTC.BINANCE");
            }
            _ => panic!("Expected missing input, received {error:?}"),
        }
        assert_eq!(
            error.to_string(),
            "Missing price for component: LTC.BINANCE",
        );
    }

    #[rstest]
    fn test_calculate_from_map_fallback_rejects_missing_component() {
        let count = MAX_INLINE_COMPONENTS + 2;
        let components: Vec<InstrumentId> = (0..count)
            .map(|i| InstrumentId::from(format!("C{i}.VENUE").as_str()))
            .collect();
        let terms: Vec<String> = components.iter().map(ToString::to_string).collect();
        let formula = terms.join(" + ");
        let missing_component = components.last().unwrap().to_string();

        let synth = SyntheticInstrument::new(
            Symbol::from("BIG"),
            2,
            components.clone(),
            &formula,
            0.into(),
            0.into(),
        );

        let mut inputs = HashMap::new();
        for component in components.iter().take(count - 1) {
            inputs.insert(component.to_string(), 10.0);
        }

        let error = synth.calculate_from_map(&inputs).unwrap_err();

        match &error {
            SyntheticInstrumentError::MissingInput { component_name } => {
                assert_eq!(component_name, &missing_component);
            }
            _ => panic!("Expected missing input, received {error:?}"),
        }
        assert_eq!(
            error.to_string(),
            format!("Missing price for component: {missing_component}"),
        );
    }

    #[rstest]
    fn test_calculate_rejects_invalid_price_result() {
        let mut synth = SyntheticInstrument::default();
        synth
            .change_formula("BTC.BINANCE / (LTC.BINANCE - LTC.BINANCE)")
            .unwrap();

        let error = synth.calculate(&[100.0, 100.0]).unwrap_err();

        match &error {
            SyntheticInstrumentError::InvalidPriceResult {
                source: CorrectnessError::InvalidValue { param, .. },
            } => {
                assert_eq!(param, "value");
            }
            _ => panic!("Expected invalid price result, received {error:?}"),
        }
        assert_eq!(
            error.to_string(),
            "Formula result produced invalid price: invalid f64 for 'value', was inf",
        );
    }

    #[rstest]
    fn test_is_valid_formula() {
        let synth = SyntheticInstrument::default();

        assert!(synth.is_valid_formula("(BTC.BINANCE + LTC.BINANCE) / 3"));
        assert!(!synth.is_valid_formula("UNKNOWN.VENUE + 1"));
        assert!(!synth.is_valid_formula(""));
    }

    #[rstest]
    #[case(f64::NAN, 100.0, "Non-finite input price")]
    #[case(100.0, f64::INFINITY, "Non-finite input price")]
    #[case(f64::NEG_INFINITY, 100.0, "Non-finite input price")]
    fn test_calculate_rejects_non_finite_inputs(
        #[case] a: f64,
        #[case] b: f64,
        #[case] expected_msg: &str,
    ) {
        let synth = SyntheticInstrument::default();
        let error = synth.calculate(&[a, b]).unwrap_err();

        match &error {
            SyntheticInstrumentError::NonFiniteInput { component_name, .. } => {
                assert!(["BTC.BINANCE", "LTC.BINANCE"].contains(&component_name.as_str()));
            }
            _ => panic!("Expected non-finite input, received {error:?}"),
        }
        assert!(error.to_string().contains(expected_msg), "{error}");
    }

    #[rstest]
    fn test_components_with_colliding_legacy_aliases_coexist() {
        let comp1 = InstrumentId::from_str("FOO-BAR.VENUE").unwrap();
        let comp2 = InstrumentId::from_str("FOO_BAR.VENUE").unwrap();
        let formula = format!("{comp1} + {comp2}");
        let synth = SyntheticInstrument::new(
            Symbol::from("TEST"),
            2,
            vec![comp1, comp2],
            &formula,
            0.into(),
            0.into(),
        );
        let price = synth.calculate(&[100.0, 200.0]).unwrap();

        assert_eq!(price, Price::from("300.0"));
    }

    #[rstest]
    fn test_calculate_from_map_fallback_for_many_components() {
        let count = MAX_INLINE_COMPONENTS + 2;
        let components: Vec<InstrumentId> = (0..count)
            .map(|i| InstrumentId::from(format!("C{i}.VENUE").as_str()))
            .collect();
        let terms: Vec<String> = components.iter().map(|c| c.to_string()).collect();
        let formula = terms.join(" + ");

        let synth = SyntheticInstrument::new(
            Symbol::from("BIG"),
            2,
            components.clone(),
            &formula,
            0.into(),
            0.into(),
        );

        let mut inputs = HashMap::new();
        for component in &components {
            inputs.insert(component.to_string(), 10.0);
        }

        let price = synth.calculate_from_map(&inputs).unwrap();

        assert_eq!(price, Price::from("100.0"));
    }
}
