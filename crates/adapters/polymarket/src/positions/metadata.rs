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

//! Verified on-chain metadata for negative-risk CTF events.
//!
//! Neg-risk question IDs share the first 31 bytes of their market ID and encode the
//! question index in the final byte. Market IDs always have a zero final byte, so question
//! indices are derived from this relationship rather than from Gamma display order or the
//! CLOB token array.

use alloy_primitives::{B256, U256};

use super::convert::{CONVERT_FEE_DENOMINATOR, ConvertIndexSet, MAX_CONVERT_QUESTIONS};
use crate::http::error::{Error, Result};

/// One verified condition in a negative-risk event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegRiskCondition {
    /// Question index, also the index-set bit selecting the NO outcome.
    pub index: u8,
    /// On-chain question identifier.
    pub question_id: B256,
    /// Conditional Tokens condition identifier.
    pub condition_id: B256,
    /// YES outcome token identifier.
    pub yes_token_id: U256,
    /// NO outcome token identifier.
    pub no_token_id: U256,
}

/// Verified metadata for a negative-risk event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegRiskMarket {
    /// On-chain negative-risk market identifier.
    pub market_id: B256,
    /// Number of questions in the event.
    pub question_count: u16,
    /// Conversion fee in basis points.
    pub fee_bips: u16,
    /// Conditions ordered by question index.
    pub conditions: Vec<NegRiskCondition>,
}

impl NegRiskMarket {
    /// Creates verified market metadata, checking internal consistency.
    ///
    /// # Errors
    ///
    /// Returns an error if the market ID is not a canonical neg-risk market ID, the question
    /// count is invalid, or the conditions do not match the market and question indices.
    pub fn new(
        market_id: B256,
        question_count: u16,
        fee_bips: u16,
        conditions: Vec<NegRiskCondition>,
    ) -> Result<Self> {
        validate_market_id(market_id)?;

        if question_count == 0 || question_count > MAX_CONVERT_QUESTIONS {
            return Err(Error::bad_request(format!(
                "neg-risk market question count must be between 1 and {MAX_CONVERT_QUESTIONS}, was {question_count}"
            )));
        }

        if fee_bips > CONVERT_FEE_DENOMINATOR {
            return Err(Error::bad_request(format!(
                "neg-risk market fee must not exceed {CONVERT_FEE_DENOMINATOR} basis points, was {fee_bips}"
            )));
        }

        if conditions.len() != usize::from(question_count) {
            return Err(Error::bad_request(format!(
                "neg-risk market metadata has {} conditions but {question_count} questions",
                conditions.len()
            )));
        }

        for (position, condition) in conditions.iter().enumerate() {
            let expected_index = u8::try_from(position).map_err(|_| {
                Error::bad_request("neg-risk market condition index exceeds the uint8 range")
            })?;

            if condition.index != expected_index {
                return Err(Error::bad_request(format!(
                    "neg-risk market condition {expected_index} is out of order"
                )));
            }

            if condition.question_id != question_id(market_id, expected_index)? {
                return Err(Error::bad_request(format!(
                    "neg-risk question {expected_index} does not belong to market {market_id:#x}"
                )));
            }

            if condition.condition_id.is_zero()
                || condition.yes_token_id.is_zero()
                || condition.no_token_id.is_zero()
            {
                return Err(Error::bad_request(format!(
                    "neg-risk question {expected_index} has incomplete on-chain metadata"
                )));
            }

            if condition.yes_token_id == condition.no_token_id {
                return Err(Error::bad_request(format!(
                    "neg-risk question {expected_index} has identical YES and NO token IDs"
                )));
            }
        }

        for (position, condition) in conditions.iter().enumerate() {
            for other in &conditions[position + 1..] {
                if condition.yes_token_id == other.yes_token_id
                    || condition.no_token_id == other.no_token_id
                    || condition.yes_token_id == other.no_token_id
                    || condition.no_token_id == other.yes_token_id
                {
                    return Err(Error::bad_request(format!(
                        "neg-risk questions {} and {} share a token ID",
                        condition.index, other.index
                    )));
                }
            }
        }

        Ok(Self {
            market_id,
            question_count,
            fee_bips,
            conditions,
        })
    }

    /// Returns the condition for `index`, if it exists.
    #[must_use]
    pub fn condition(&self, index: u8) -> Option<&NegRiskCondition> {
        self.conditions.get(usize::from(index))
    }

    /// Returns the condition whose NO token is `token_id`.
    #[must_use]
    pub fn condition_by_no_token(&self, token_id: U256) -> Option<&NegRiskCondition> {
        self.conditions
            .iter()
            .find(|condition| condition.no_token_id == token_id)
    }

    /// Returns the condition whose YES token is `token_id`.
    #[must_use]
    pub fn condition_by_yes_token(&self, token_id: U256) -> Option<&NegRiskCondition> {
        self.conditions
            .iter()
            .find(|condition| condition.yes_token_id == token_id)
    }

    /// Builds a validated index set from NO token identifiers.
    ///
    /// # Errors
    ///
    /// Returns an error if the token list is empty, repeats a token, or contains a token
    /// that is not a NO outcome of this market.
    pub fn index_set_for_no_tokens(&self, token_ids: &[U256]) -> Result<ConvertIndexSet> {
        if token_ids.is_empty() {
            return Err(Error::bad_request(
                "neg-risk conversion requires at least one NO instrument",
            ));
        }

        let mut indices = Vec::with_capacity(token_ids.len());
        for token_id in token_ids {
            let condition = self.condition_by_no_token(*token_id).ok_or_else(|| {
                Error::bad_request(format!(
                    "token {token_id} is not a NO outcome of neg-risk market {:#x}",
                    self.market_id
                ))
            })?;

            if indices.contains(&condition.index) {
                return Err(Error::bad_request(format!(
                    "neg-risk conversion input token {token_id} is duplicated"
                )));
            }

            indices.push(condition.index);
        }

        ConvertIndexSet::from_indices(&indices)
    }
}

/// Returns the question ID for a question index within a neg-risk market.
///
/// # Errors
///
/// Returns an error if `market_id` is not a canonical neg-risk market ID with a zero final
/// byte.
pub fn question_id(market_id: B256, index: u8) -> Result<B256> {
    validate_market_id(market_id)?;

    let mut bytes = market_id.as_slice().to_vec();
    bytes[31] = index;
    Ok(B256::from_slice(&bytes))
}

/// Validates that `market_id` is a canonical neg-risk market identifier.
///
/// # Errors
///
/// Returns an error if the final byte is not zero.
pub fn validate_market_id(market_id: B256) -> Result<()> {
    if market_id.as_slice()[31] != 0 {
        return Err(Error::bad_request(format!(
            "neg-risk market ID {market_id:#x} must have a zero final byte"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn market_id() -> B256 {
        let mut bytes = [0x23u8; 32];
        bytes[31] = 0;
        B256::from(bytes)
    }

    fn condition(index: u8) -> NegRiskCondition {
        NegRiskCondition {
            index,
            question_id: question_id(market_id(), index).unwrap(),
            condition_id: B256::repeat_byte(index + 1),
            yes_token_id: U256::from(index) * U256::from(10u8) + U256::from(1u8),
            no_token_id: U256::from(index) * U256::from(10u8) + U256::from(2u8),
        }
    }

    #[rstest]
    fn test_question_id_encodes_index_in_final_byte() {
        let market = market_id();
        let first = question_id(market, 0).unwrap();
        let second = question_id(market, 7).unwrap();

        assert_eq!(first, market);
        assert_eq!(second.as_slice()[31], 7);
        assert_eq!(&second.as_slice()[..31], &market.as_slice()[..31]);
    }

    #[rstest]
    fn test_question_id_rejects_non_canonical_market() {
        let mut bytes = [0x23u8; 32];
        bytes[31] = 1;
        assert!(question_id(B256::from(bytes), 0).is_err());
    }

    #[rstest]
    fn test_market_metadata_lookups() {
        let market =
            NegRiskMarket::new(market_id(), 3, 100, (0..3).map(condition).collect()).unwrap();

        assert_eq!(market.condition(1).unwrap().index, 1);
        assert!(market.condition(3).is_none());
        assert_eq!(
            market
                .condition_by_no_token(U256::from(12u8))
                .unwrap()
                .index,
            1
        );
        assert_eq!(
            market
                .condition_by_yes_token(U256::from(11u8))
                .unwrap()
                .index,
            1
        );
        assert!(market.condition_by_no_token(U256::from(11u8)).is_none());
    }

    #[rstest]
    fn test_market_index_set_from_no_tokens() {
        let market =
            NegRiskMarket::new(market_id(), 4, 100, (0..4).map(condition).collect()).unwrap();

        let set = market
            .index_set_for_no_tokens(&[U256::from(2u8), U256::from(32u8)])
            .unwrap();
        assert_eq!(set.indices(), vec![0, 3]);
    }

    #[rstest]
    fn test_market_rejects_duplicate_and_foreign_tokens() {
        let market =
            NegRiskMarket::new(market_id(), 2, 100, (0..2).map(condition).collect()).unwrap();

        assert!(market.index_set_for_no_tokens(&[]).is_err());
        assert!(
            market
                .index_set_for_no_tokens(&[U256::from(2u8), U256::from(2u8)])
                .is_err()
        );
        assert!(market.index_set_for_no_tokens(&[U256::from(99u8)]).is_err());
    }

    #[rstest]
    fn test_market_rejects_inconsistent_metadata() {
        let mut conditions: Vec<_> = (0..2).map(condition).collect();

        assert!(NegRiskMarket::new(market_id(), 0, 100, conditions.clone()).is_err());
        assert!(NegRiskMarket::new(market_id(), 3, 100, conditions.clone()).is_err());
        assert!(NegRiskMarket::new(market_id(), 2, 10_001, conditions.clone()).is_err());

        conditions[1].index = 0;
        assert!(NegRiskMarket::new(market_id(), 2, 100, conditions).is_err());
    }

    #[rstest]
    fn test_market_rejects_mismatched_question_and_shared_tokens() {
        let mut conditions: Vec<_> = (0..2).map(condition).collect();
        let mut other_market = [0x24u8; 32];
        other_market[31] = 0;
        conditions[1].question_id =
            question_id(B256::from(other_market), 1).expect("valid market ID");
        assert!(NegRiskMarket::new(market_id(), 2, 100, conditions).is_err());

        let mut conditions: Vec<_> = (0..2).map(condition).collect();
        conditions[1].no_token_id = conditions[0].yes_token_id;
        assert!(NegRiskMarket::new(market_id(), 2, 100, conditions).is_err());
    }
}
