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

//! Exact neg-risk CTF conversion arithmetic shared by live and simulated venues.
//!
//! The neg-risk CTF collateral adapter converts one or more NO positions within a
//! negative-risk event. Bit `i` of the index set selects question index `i` as an input
//! NO position. All integer values are six-decimal base units, matching the on-chain
//! contract arithmetic.

use ahash::AHashSet;
use nautilus_core::Params;
pub use ruint::aliases::U256;
use rust_decimal::{Decimal, prelude::ToPrimitive};

use crate::{identifiers::InstrumentId, types::Quantity};

/// Instrument metadata key carrying the Polymarket market version.
pub const MARKET_VERSION_KEY: &str = "market_version";

/// Polymarket V2 market version, which does not use the CTF conversion contract.
pub const MARKET_VERSION_V2: &str = "v2";

/// Denominator for the neg-risk conversion fee expressed in basis points.
pub const CONVERT_FEE_DENOMINATOR: u16 = 10_000;

/// Maximum number of questions a neg-risk event can hold.
///
/// Question IDs encode the question index in their final byte, so the count cannot exceed
/// the value range of a `uint8` index.
pub const MAX_CONVERT_QUESTIONS: u16 = 256;

/// Integer amounts produced by a neg-risk conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConvertAmounts {
    /// Gross amount of each input NO position.
    pub input: U256,
    /// Fee retained by the conversion, charged per input amount.
    pub fee: U256,
    /// Amount of each unselected YES position received.
    pub per_outcome: U256,
    /// Net pUSD collateral returned for `k` selected NO positions, `(k - 1) * per_outcome`.
    pub collateral_return: U256,
}

/// Computes the exact conversion fee and outputs for a gross input amount.
///
/// # Errors
///
/// Returns an error if the input is zero, the fee exceeds the denominator, or the selected
/// question count is outside `1..=MAX_CONVERT_QUESTIONS`.
pub fn convert_amounts(
    input: U256,
    fee_bips: u16,
    no_count: usize,
) -> anyhow::Result<ConvertAmounts> {
    if input.is_zero() {
        anyhow::bail!("neg-risk conversion input must be positive");
    }

    if fee_bips > CONVERT_FEE_DENOMINATOR {
        anyhow::bail!(
            "neg-risk conversion fee must not exceed {CONVERT_FEE_DENOMINATOR} basis points, was {fee_bips}"
        );
    }

    if no_count == 0 || no_count > usize::from(MAX_CONVERT_QUESTIONS) {
        anyhow::bail!(
            "neg-risk conversion requires between 1 and {MAX_CONVERT_QUESTIONS} NO positions, was {no_count}"
        );
    }

    let fee = input
        .checked_mul(U256::from(fee_bips))
        .ok_or_else(|| anyhow::anyhow!("neg-risk conversion fee overflowed"))?
        .checked_div(U256::from(CONVERT_FEE_DENOMINATOR))
        .ok_or_else(|| anyhow::anyhow!("neg-risk conversion fee division by zero"))?;

    let per_outcome = input
        .checked_sub(fee)
        .ok_or_else(|| anyhow::anyhow!("neg-risk conversion output underflowed"))?;

    let selected = u8::try_from(no_count - 1)
        .map_err(|_| anyhow::anyhow!("neg-risk conversion selection count is invalid"))?;
    let collateral_return = per_outcome
        .checked_mul(U256::from(selected))
        .ok_or_else(|| anyhow::anyhow!("neg-risk conversion collateral overflowed"))?;

    Ok(ConvertAmounts {
        input,
        fee,
        per_outcome,
        collateral_return,
    })
}

/// One question of a negative-risk event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegRiskCondition {
    /// The on-chain question index.
    pub question_index: u8,
    /// The CLOB condition identifier.
    pub condition_id: String,
    /// The YES outcome token identifier.
    pub yes_token_id: String,
    /// The NO outcome token identifier.
    pub no_token_id: String,
}

/// Negative-risk event metadata supplied through the instrument `neg_risk_market` params.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegRiskMarketMetadata {
    /// The on-chain negative-risk market identifier.
    pub market_id: String,
    /// The conversion fee in basis points.
    pub fee_bips: u16,
    /// Event questions ordered by question index.
    pub conditions: Vec<NegRiskCondition>,
}

impl NegRiskMarketMetadata {
    /// Parses and validates the `neg_risk_market` instrument metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not negative-risk, the metadata is missing, the fee
    /// is invalid, or the question indices are not unique and contiguous from zero.
    pub fn parse(info: &Params) -> anyhow::Result<Self> {
        if info.get_bool("neg_risk") != Some(true) {
            anyhow::bail!("instrument is not part of a negative-risk market");
        }

        let value = info
            .get("neg_risk_market")
            .ok_or_else(|| anyhow::anyhow!("instrument is missing neg_risk_market metadata"))?;
        let market_id = value
            .get("market_id")
            .and_then(serde_json::Value::as_str)
            .filter(|market_id| !market_id.is_empty())
            .ok_or_else(|| anyhow::anyhow!("neg_risk_market metadata has no market_id"))?
            .to_string();
        let fee_bips = value
            .get("fee_bips")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("neg_risk_market metadata has no fee_bips"))?;
        let fee_bips = u16::try_from(fee_bips)
            .map_err(|_| anyhow::anyhow!("neg_risk_market fee_bips is invalid: {fee_bips}"))?;

        let raw_conditions = value
            .get("conditions")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("neg_risk_market metadata has no conditions"))?;

        if raw_conditions.is_empty() {
            anyhow::bail!("neg_risk_market metadata has no conditions");
        }
        if raw_conditions.len() > usize::from(MAX_CONVERT_QUESTIONS) {
            anyhow::bail!("neg_risk_market has more than {MAX_CONVERT_QUESTIONS} conditions");
        }

        let mut conditions = Vec::with_capacity(raw_conditions.len());
        for raw in raw_conditions {
            let question_index = raw
                .get("question_index")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    anyhow::anyhow!("neg_risk_market condition has no question_index")
                })?;
            let question_index = u8::try_from(question_index).map_err(|_| {
                anyhow::anyhow!("neg_risk_market question_index is invalid: {question_index}")
            })?;
            let condition_id = raw
                .get("condition_id")
                .and_then(serde_json::Value::as_str)
                .filter(|condition_id| !condition_id.is_empty())
                .ok_or_else(|| anyhow::anyhow!("neg_risk_market condition has no condition_id"))?
                .to_string();
            let yes_token_id = raw
                .get("yes_token_id")
                .and_then(serde_json::Value::as_str)
                .filter(|token_id| !token_id.is_empty())
                .ok_or_else(|| anyhow::anyhow!("neg_risk_market condition has no yes_token_id"))?
                .to_string();
            let no_token_id = raw
                .get("no_token_id")
                .and_then(serde_json::Value::as_str)
                .filter(|token_id| !token_id.is_empty())
                .ok_or_else(|| anyhow::anyhow!("neg_risk_market condition has no no_token_id"))?
                .to_string();

            conditions.push(NegRiskCondition {
                question_index,
                condition_id,
                yes_token_id,
                no_token_id,
            });
        }

        conditions.sort_by_key(|condition| condition.question_index);
        for (expected, condition) in conditions.iter().enumerate() {
            if usize::from(condition.question_index) != expected {
                anyhow::bail!("neg_risk_market question indices must be contiguous from zero");
            }
        }

        let mut condition_ids = AHashSet::new();
        let mut token_ids = AHashSet::new();
        for condition in &conditions {
            if !condition_ids.insert(condition.condition_id.as_str()) {
                anyhow::bail!(
                    "neg_risk_market contains duplicate condition_id {}",
                    condition.condition_id
                );
            }
            for token_id in [&condition.yes_token_id, &condition.no_token_id] {
                if !token_ids.insert(token_id.as_str()) {
                    anyhow::bail!(
                        "neg_risk_market token {token_id} is assigned to multiple outcomes"
                    );
                }
            }
        }

        Ok(Self {
            market_id,
            fee_bips,
            conditions,
        })
    }

    /// Returns the question whose NO token matches `token_id`.
    #[must_use]
    pub fn condition_for_no_token(&self, token_id: &str) -> Option<&NegRiskCondition> {
        self.conditions
            .iter()
            .find(|condition| condition.no_token_id == token_id)
    }
}

/// Converts a positive pUSD quantity to six-decimal base units.
///
/// # Errors
///
/// Returns an error if the quantity is not positive or has more than six decimal places.
pub fn quantity_to_base_units(quantity: Quantity) -> anyhow::Result<U256> {
    let amount = quantity.as_decimal();
    if amount <= Decimal::ZERO {
        anyhow::bail!("convert quantity must be positive");
    }

    let scaled = amount * Decimal::from(1_000_000u64);
    if !scaled.fract().is_zero() {
        anyhow::bail!("convert quantity precision exceeds six decimals");
    }

    let value = scaled
        .to_u128()
        .ok_or_else(|| anyhow::anyhow!("convert quantity is too large"))?;
    Ok(U256::from(value))
}

/// Computes one CTF payout amount using contract integer base-unit arithmetic.
///
/// The token quantity is scaled to six-decimal base units and multiplied by the payout
/// numerator before dividing by the denominator, with any remainder discarded.
///
/// # Errors
///
/// Returns an error if the quantity is invalid, the payout fraction is invalid, or the
/// arithmetic overflows.
pub fn ctf_payout_amount(
    quantity: Quantity,
    numerator: U256,
    denominator: U256,
) -> anyhow::Result<Decimal> {
    if denominator.is_zero() || numerator > denominator {
        anyhow::bail!("invalid CTF payout fraction {numerator}/{denominator}");
    }
    let base = quantity_to_base_units(quantity)?;
    let payout = base
        .checked_mul(numerator)
        .ok_or_else(|| anyhow::anyhow!("CTF payout numerator overflowed"))?
        .checked_div(denominator)
        .ok_or_else(|| anyhow::anyhow!("CTF payout denominator is zero"))?;
    base_units_to_decimal(payout)
}

/// Converts six-decimal base units to an exact pUSD decimal.
///
/// # Errors
///
/// Returns an error if the integer amount cannot be represented as a decimal.
pub fn base_units_to_decimal(amount: U256) -> anyhow::Result<Decimal> {
    let value: Decimal = amount
        .to_string()
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid base-unit amount: {e}"))?;
    Ok(value / Decimal::from(1_000_000u64))
}

/// Validates that an instrument's validity window contains `now`.
///
/// # Errors
///
/// Returns an error if the instrument is not yet active or has expired.
pub fn ensure_instrument_active(
    instrument_id: &InstrumentId,
    activation_ns: nautilus_core::UnixNanos,
    expiration_ns: nautilus_core::UnixNanos,
    now: nautilus_core::UnixNanos,
) -> anyhow::Result<()> {
    if !activation_ns.is_zero() && now < activation_ns {
        anyhow::bail!("instrument {instrument_id} is not active yet");
    }
    if !expiration_ns.is_zero() && now >= expiration_ns {
        anyhow::bail!("instrument {instrument_id} has expired");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(U256::from(1_000_000u64), 0, 1, 0, 1_000_000)]
    #[case(U256::from(1_000_000u64), 100, 1, 10_000, 990_000)]
    #[case(U256::from(1_000_000u64), 100, 2, 10_000, 990_000)]
    #[case(U256::from(1_000_000u64), 100, 3, 10_000, 990_000)]
    fn test_convert_amounts(
        #[case] input: U256,
        #[case] fee_bips: u16,
        #[case] no_count: usize,
        #[case] expected_fee: u64,
        #[case] expected_per_outcome: u64,
    ) {
        let amounts = convert_amounts(input, fee_bips, no_count).unwrap();
        let expected_per_outcome = U256::from(expected_per_outcome);
        assert_eq!(amounts.input, input);
        assert_eq!(amounts.fee, U256::from(expected_fee));
        assert_eq!(amounts.per_outcome, expected_per_outcome);
        assert_eq!(
            amounts.collateral_return,
            expected_per_outcome * U256::from(u64::try_from(no_count - 1).unwrap())
        );
    }

    #[rstest]
    fn test_convert_amounts_rejects_invalid_inputs() {
        assert!(convert_amounts(U256::ZERO, 0, 1).is_err());
        assert!(convert_amounts(U256::from(1u64), 10_001, 1).is_err());
        assert!(convert_amounts(U256::from(1u64), 0, 0).is_err());
        assert!(convert_amounts(U256::from(1u64), 0, 257).is_err());
    }

    fn market_info(conditions: &serde_json::Value) -> Params {
        Params::new(
            [
                ("neg_risk".to_string(), serde_json::Value::Bool(true)),
                (
                    "neg_risk_market".to_string(),
                    serde_json::json!({
                        "market_id": "0xmarket",
                        "fee_bips": 100,
                        "conditions": conditions,
                    }),
                ),
            ]
            .into(),
        )
    }

    #[rstest]
    fn test_neg_risk_market_metadata_orders_conditions() {
        let info = market_info(&serde_json::json!([
            {
                "question_index": 1,
                "condition_id": "0xc1",
                "yes_token_id": "1100",
                "no_token_id": "1101",
            },
            {
                "question_index": 0,
                "condition_id": "0xc0",
                "yes_token_id": "1000",
                "no_token_id": "1001",
            },
        ]));

        let market = NegRiskMarketMetadata::parse(&info).unwrap();
        assert_eq!(market.market_id, "0xmarket");
        assert_eq!(market.fee_bips, 100);
        assert_eq!(market.conditions[0].question_index, 0);
        assert_eq!(
            market.condition_for_no_token("1101").unwrap().condition_id,
            "0xc1"
        );
    }

    #[rstest]
    fn test_neg_risk_market_metadata_rejects_non_contiguous_indices() {
        let info = market_info(&serde_json::json!([{
            "question_index": 2,
            "condition_id": "0xc2",
            "yes_token_id": "1200",
            "no_token_id": "1201",
        }]));

        assert!(NegRiskMarketMetadata::parse(&info).is_err());
    }

    #[rstest]
    fn test_neg_risk_market_metadata_rejects_shared_tokens_and_conditions() {
        let shared_yes = market_info(&serde_json::json!([
            {
                "question_index": 0,
                "condition_id": "0xc0",
                "yes_token_id": "1200",
                "no_token_id": "1001",
            },
            {
                "question_index": 1,
                "condition_id": "0xc1",
                "yes_token_id": "1200",
                "no_token_id": "1101",
            },
        ]));
        assert!(NegRiskMarketMetadata::parse(&shared_yes).is_err());

        let shared_no = market_info(&serde_json::json!([
            {
                "question_index": 0,
                "condition_id": "0xc0",
                "yes_token_id": "1000",
                "no_token_id": "1101",
            },
            {
                "question_index": 1,
                "condition_id": "0xc1",
                "yes_token_id": "1100",
                "no_token_id": "1101",
            },
        ]));
        assert!(NegRiskMarketMetadata::parse(&shared_no).is_err());

        let shared_condition = market_info(&serde_json::json!([
            {
                "question_index": 0,
                "condition_id": "0xc0",
                "yes_token_id": "1000",
                "no_token_id": "1001",
            },
            {
                "question_index": 1,
                "condition_id": "0xc0",
                "yes_token_id": "1100",
                "no_token_id": "1101",
            },
        ]));
        assert!(NegRiskMarketMetadata::parse(&shared_condition).is_err());
    }

    #[rstest]
    fn test_neg_risk_market_metadata_requires_neg_risk_flag() {
        let info = Params::new(
            [
                ("neg_risk".to_string(), serde_json::Value::Bool(false)),
                (
                    "neg_risk_market".to_string(),
                    serde_json::json!({
                        "market_id": "0xmarket",
                        "fee_bips": 100,
                        "conditions": [],
                    }),
                ),
            ]
            .into(),
        );

        assert!(NegRiskMarketMetadata::parse(&info).is_err());
    }

    #[rstest]
    fn test_quantity_and_base_units_are_exact() {
        assert_eq!(
            quantity_to_base_units(Quantity::from("100")).unwrap(),
            U256::from(100_000_000u64)
        );
        assert_eq!(
            base_units_to_decimal(U256::from(99_000_000u64)).unwrap(),
            Decimal::from_str("99").unwrap()
        );
        assert!(quantity_to_base_units(Quantity::from("0")).is_err());
        assert!(quantity_to_base_units(Quantity::from("1.0000001")).is_err());
    }
}
