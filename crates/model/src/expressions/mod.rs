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

//! Numeric expression engine for synthetic instrument formulas.
//!
//! Compiles formula strings into bytecode and evaluates them against f64 input slots.
//! The engine supports arithmetic, comparisons, boolean logic, local variables,
//! and built-in functions (`abs`, `ceil`, `floor`, `round`, `min`, `max`, `if`).

mod error;
mod eval;
mod lexer;
mod parser;

use std::fmt::Display;

use ahash::AHashMap;

pub(crate) use self::{
    error::{ExpressionError, Result},
    eval::CompiledExpression,
};

/// The type produced by a compiled expression: numeric, boolean, or empty.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValueType {
    Number,
    Bool,
    Empty,
}

impl ValueType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Bool => "bool",
            Self::Empty => "empty",
        }
    }
}

impl Display for ValueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Maps component names to input slot indices for formula compilation.
///
/// Plain identifiers (letters, digits, underscores) go into a hash map for O(1) lookup.
/// Names containing special characters (`/`, `-`, `.`) use a longest-match strategy
/// keyed by first character.
#[derive(Clone, Debug, Default)]
pub(crate) struct Bindings {
    plain: AHashMap<Box<str>, usize>,
    special: AHashMap<char, Vec<SpecialBinding>>,
    max_slot_plus_one: usize,
}

#[derive(Clone, Debug)]
pub(super) struct SpecialBinding {
    name: Box<str>,
    slot: usize,
}

impl Bindings {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// # Errors
    ///
    /// Returns an error if the same binding name is assigned to multiple input slots.
    pub(crate) fn add(&mut self, slot: usize, name: &str) -> Result<()> {
        self.add_name(slot, name)
    }

    /// # Errors
    ///
    /// Returns an error if the same alias name is assigned to multiple input slots.
    pub(crate) fn add_alias(&mut self, slot: usize, alias: &str) -> Result<()> {
        self.add_name(slot, alias)
    }

    #[must_use]
    pub(crate) fn input_len(&self) -> usize {
        self.max_slot_plus_one
    }

    pub(super) fn resolve_plain(&self, ident: &str) -> Option<usize> {
        self.plain.get(ident).copied()
    }

    pub(super) fn special_candidates(&self, first: char) -> Option<&[SpecialBinding]> {
        self.special.get(&first).map(Vec::as_slice)
    }

    fn add_name(&mut self, slot: usize, name: &str) -> Result<()> {
        self.max_slot_plus_one = self.max_slot_plus_one.max(slot + 1);

        if is_plain_identifier(name) {
            if let Some(existing) = self.plain.get(name) {
                if *existing != slot {
                    return Err(ExpressionError::DuplicateBinding {
                        name: name.to_string(),
                        existing_slot: *existing,
                        slot,
                    });
                }
                return Ok(());
            }

            self.plain.insert(name.into(), slot);
            return Ok(());
        }

        let Some(first) = name.chars().next() else {
            return Err(ExpressionError::EmptyBindingName);
        };

        let entries = self.special.entry(first).or_default();
        if let Some(existing) = entries.iter().find(|entry| entry.name.as_ref() == name) {
            if existing.slot != slot {
                return Err(ExpressionError::DuplicateBinding {
                    name: name.to_string(),
                    existing_slot: existing.slot,
                    slot,
                });
            }
            return Ok(());
        }

        entries.push(SpecialBinding {
            name: name.into(),
            slot,
        });
        entries.sort_unstable_by(|left, right| {
            right
                .name
                .len()
                .cmp(&left.name.len())
                .then_with(|| left.name.cmp(&right.name))
        });

        Ok(())
    }
}

/// # Errors
///
/// Returns an error if tokenization, parsing, or semantic validation fails.
pub(crate) fn compile(source: &str, bindings: &Bindings) -> Result<CompiledExpression> {
    let tokens = lexer::tokenize(source, bindings)?;
    let program = parser::parse(&tokens)?;
    eval::compile(&program, bindings)
}

/// # Errors
///
/// Returns an error if the compiled expression does not produce a numeric result.
pub(crate) fn compile_numeric(source: &str, bindings: &Bindings) -> Result<CompiledExpression> {
    let compiled = compile(source, bindings)?;
    if compiled.result_type() != ValueType::Number {
        return Err(ExpressionError::NonNumericResult {
            actual: compiled.result_type(),
        });
    }

    Ok(compiled)
}

pub(super) fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

pub(super) fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_plain_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    is_ident_start(first) && chars.all(is_ident_continue)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_bindings_resolve_plain_identifiers() {
        let mut bindings = Bindings::new();
        bindings.add(0, "spread").unwrap();
        bindings.add(1, "ratio").unwrap();

        assert_eq!(bindings.resolve_plain("spread"), Some(0));
        assert_eq!(bindings.resolve_plain("ratio"), Some(1));
        assert_eq!(bindings.resolve_plain("missing"), None);
    }

    #[rstest]
    fn test_bindings_keep_special_candidates_sorted_by_length() {
        let mut bindings = Bindings::new();
        bindings.add(0, "ETH-USDT-SWAP.OKX").unwrap();
        bindings.add(1, "ETH-USDT").unwrap();

        let candidates = bindings.special_candidates('E').unwrap();
        let names: Vec<&str> = candidates
            .iter()
            .map(|candidate| candidate.name.as_ref())
            .collect();

        assert_eq!(names, vec!["ETH-USDT-SWAP.OKX", "ETH-USDT"]);
    }

    #[rstest]
    fn test_bindings_reject_duplicate_names_for_different_slots() {
        let mut bindings = Bindings::new();
        bindings.add(0, "spread").unwrap();
        let error = bindings.add(1, "spread").unwrap_err();

        assert_eq!(
            error,
            ExpressionError::DuplicateBinding {
                name: "spread".to_string(),
                existing_slot: 0,
                slot: 1,
            }
        );
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    fn bindings_with_xy() -> Bindings {
        let mut b = Bindings::new();
        b.add(0, "x").unwrap();
        b.add(1, "y").unwrap();
        b
    }

    fn finite_f64() -> impl Strategy<Value = f64> {
        prop_oneof![
            -1e12f64..1e12,
            prop::num::f64::NORMAL,
            Just(0.0),
            Just(-0.0),
            Just(1.0),
            Just(-1.0),
        ]
    }

    #[rstest]
    fn prop_arithmetic_never_panics() {
        let bindings = bindings_with_xy();
        let formulas = [
            "x + y",
            "x - y",
            "x * y",
            "x / y",
            "x % y",
            "(x + y) * (x - y)",
            "x + y + x + y",
        ];

        proptest!(|(a in finite_f64(), b in finite_f64())| {
            for formula in &formulas {
                if let Ok(compiled) = compile_numeric(formula, &bindings) {
                    let _ = compiled.eval_number(&[a, b]);
                }
            }
        });
    }

    #[rstest]
    fn prop_comparison_returns_zero_or_one() {
        let bindings = bindings_with_xy();
        let formulas = ["x < y", "x <= y", "x > y", "x >= y", "x == y", "x != y"];

        proptest!(|(a in finite_f64(), b in finite_f64())| {
            for formula in &formulas {
                let compiled = compile(formula, &bindings).unwrap();
                let value = compiled.eval_number(&[a, b]).unwrap();
                prop_assert!(value == 0.0 || value == 1.0, "returned {value} for {formula}");
            }
        });
    }

    #[rstest]
    fn prop_compile_eval_roundtrip_is_deterministic() {
        let bindings = bindings_with_xy();
        let formula = "(x + y) * (x - y) / (abs(x) + 1)";
        let compiled = compile_numeric(formula, &bindings).unwrap();

        proptest!(|(a in finite_f64(), b in finite_f64())| {
            let r1 = compiled.eval_number(&[a, b]).unwrap();
            let r2 = compiled.eval_number(&[a, b]).unwrap();
            prop_assert_eq!(r1.to_bits(), r2.to_bits());
        });
    }

    #[rstest]
    fn prop_arbitrary_ascii_never_panics_lexer() {
        let bindings = Bindings::new();

        proptest!(|(source in "[a-z0-9+\\-*/ ().;=<>!&|^%,]{0,64}")| {
            let _ = compile(&source, &bindings);
        });
    }
}
