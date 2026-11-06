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

//! Exact neg-risk CTF conversion arithmetic and index-set validation.
//!
//! The neg-risk CTF collateral adapter converts one or more NO positions within a
//! negative-risk event. Bit `i` of the index set selects question index `i` as an input
//! NO position. All integer values are six-decimal base units, matching the on-chain
//! contract arithmetic.

use alloy_primitives::U256;
pub use nautilus_model::ctf::{CONVERT_FEE_DENOMINATOR, ConvertAmounts, MAX_CONVERT_QUESTIONS};

use crate::http::error::{Error, Result};

/// Computes the exact conversion fee and outputs for a gross input amount.
///
/// # Errors
///
/// Returns an error if the input is zero, the fee exceeds the denominator, or the selected
/// question count is outside `1..=MAX_CONVERT_QUESTIONS`.
pub fn convert_amounts(input: U256, fee_bips: u16, no_count: usize) -> Result<ConvertAmounts> {
    nautilus_model::ctf::convert_amounts(input, fee_bips, no_count)
        .map_err(|e| Error::bad_request(e.to_string()))
}

/// A non-empty bit set of neg-risk question indices selected as NO inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConvertIndexSet(U256);

impl ConvertIndexSet {
    /// Creates an index set from a raw contract bitmask.
    ///
    /// # Errors
    ///
    /// Returns an error if no question index is selected.
    pub fn new(bits: U256) -> Result<Self> {
        if bits.is_zero() {
            return Err(Error::bad_request(
                "neg-risk conversion index set must not be empty",
            ));
        }
        Ok(Self(bits))
    }

    /// Creates an index set from explicit question indices.
    ///
    /// # Errors
    ///
    /// Returns an error if `indices` is empty or contains a duplicate index.
    pub fn from_indices(indices: &[u8]) -> Result<Self> {
        if indices.is_empty() {
            return Err(Error::bad_request(
                "neg-risk conversion index set must not be empty",
            ));
        }

        let mut bits = U256::ZERO;
        for &index in indices {
            let bit = U256::from(1u8) << usize::from(index);
            if !(bits & bit).is_zero() {
                return Err(Error::bad_request(format!(
                    "neg-risk conversion index {index} is duplicated"
                )));
            }
            bits |= bit;
        }

        Self::new(bits)
    }

    /// Returns the raw contract bitmask.
    #[must_use]
    pub const fn bits(self) -> U256 {
        self.0
    }

    /// Returns the selected question indices in ascending order.
    #[must_use]
    pub fn indices(self) -> Vec<u8> {
        (0..=u8::MAX)
            .filter(|index| self.0.bit(usize::from(*index)))
            .collect()
    }

    /// Returns the number of selected question indices.
    #[must_use]
    pub fn len(self) -> usize {
        self.0.count_ones()
    }

    /// Returns whether the set selects no question index.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0.is_zero()
    }

    /// Validates every selected index against a neg-risk event question count.
    ///
    /// # Errors
    ///
    /// Returns an error if the question count is outside `1..=MAX_CONVERT_QUESTIONS` or a
    /// selected index is not a question of the event.
    pub fn validate(self, question_count: u16) -> Result<()> {
        if question_count == 0 || question_count > MAX_CONVERT_QUESTIONS {
            return Err(Error::bad_request(format!(
                "neg-risk market question count must be between 1 and {MAX_CONVERT_QUESTIONS}, was {question_count}"
            )));
        }

        if let Some(index) = self
            .indices()
            .into_iter()
            .find(|index| u16::from(*index) >= question_count)
        {
            return Err(Error::bad_request(format!(
                "neg-risk conversion index {index} is outside question count {question_count}"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(U256::from(1_000_000u64), 0, 1, 0, 1_000_000)]
    #[case(U256::from(1_000_000u64), 100, 1, 10_000, 990_000)]
    #[case(U256::from(1_000_000u64), 100, 2, 10_000, 990_000)]
    #[case(U256::from(1_000_001u64), 1, 1, 100, 1_000_001 - 100)]
    fn test_convert_amounts_integer_fee(
        #[case] input: U256,
        #[case] fee_bips: u16,
        #[case] no_count: usize,
        #[case] fee: u64,
        #[case] per_outcome: u64,
    ) {
        let amounts = convert_amounts(input, fee_bips, no_count).unwrap();
        let per_outcome = U256::from(per_outcome);
        let fee = U256::from(fee);

        assert_eq!(amounts.input, input);
        assert_eq!(amounts.fee, fee);
        assert_eq!(amounts.per_outcome, per_outcome);
        assert_eq!(
            amounts.collateral_return,
            per_outcome * U256::from(u64::try_from(no_count - 1).unwrap())
        );
    }

    #[rstest]
    fn test_convert_amounts_floor_rounding_never_adds_fee() {
        // 1 * 1 / 10_000 floors to zero, so the smallest input carries no fee.
        let amounts = convert_amounts(U256::from(1u8), 1, 1).unwrap();
        assert_eq!(amounts.fee, U256::ZERO);
        assert_eq!(amounts.per_outcome, U256::from(1u8));

        // 3 * 3_333 / 10_000 = 0.9999 floors to zero fee and one output unit.
        let amounts = convert_amounts(U256::from(3u8), 3_333, 1).unwrap();
        assert_eq!(amounts.fee, U256::ZERO);
        assert_eq!(amounts.per_outcome, U256::from(3u8));
    }

    #[rstest]
    fn test_convert_amounts_maximum_fee_leaves_zero_outputs() {
        let amounts = convert_amounts(U256::from(1_000_000u64), 10_000, 2).unwrap();
        assert_eq!(amounts.fee, U256::from(1_000_000u64));
        assert_eq!(amounts.per_outcome, U256::ZERO);
        assert_eq!(amounts.collateral_return, U256::ZERO);
    }

    #[rstest]
    #[case(U256::ZERO, 0, 1)]
    #[case(U256::from(1u8), 10_001, 1)]
    #[case(U256::from(1u8), 0, 0)]
    #[case(U256::from(1u8), 0, 257)]
    fn test_convert_amounts_rejects_invalid_inputs(
        #[case] input: U256,
        #[case] fee_bips: u16,
        #[case] no_count: usize,
    ) {
        assert!(convert_amounts(input, fee_bips, no_count).is_err());
    }

    #[rstest]
    fn test_convert_amounts_rejects_overflow() {
        assert!(convert_amounts(U256::MAX, 1, 256).is_err());
    }

    #[rstest]
    fn test_index_set_from_indices_round_trips() {
        let set = ConvertIndexSet::from_indices(&[0, 2, 5]).unwrap();
        assert_eq!(set.bits(), U256::from(0b10_0101u8));
        assert_eq!(set.indices(), vec![0, 2, 5]);
        assert_eq!(set.len(), 3);
        assert!(!set.is_empty());
    }

    #[rstest]
    fn test_index_set_rejects_empty_and_duplicates() {
        assert!(ConvertIndexSet::from_indices(&[]).is_err());
        assert!(ConvertIndexSet::new(U256::ZERO).is_err());

        let err = ConvertIndexSet::from_indices(&[1, 1]).unwrap_err();
        assert!(err.to_string().contains("index 1 is duplicated"));
    }

    #[rstest]
    fn test_index_set_validation_respects_question_count() {
        let set = ConvertIndexSet::from_indices(&[0, 1]).unwrap();
        assert!(set.validate(2).is_ok());

        let err = set.validate(1).unwrap_err();
        assert!(
            err.to_string()
                .contains("index 1 is outside question count 1")
        );

        assert!(
            ConvertIndexSet::new(U256::ONE)
                .unwrap()
                .validate(0)
                .is_err()
        );
    }

    #[rstest]
    fn test_index_set_supports_highest_question_index() {
        let set = ConvertIndexSet::from_indices(&[255]).unwrap();
        assert_eq!(set.bits(), U256::from(1u8) << 255);
        assert_eq!(set.indices(), vec![255]);
        assert!(set.validate(256).is_ok());
    }
}
