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

//! ABI encoding for Polymarket Conditional Token split, merge, and redeem calls.

use alloy::{primitives::Bytes, sol, sol_types::SolCall};
use alloy_primitives::{Address, B256, U256};
use rust_decimal::Decimal;

use super::amounts::pusd_to_base_units;
use crate::{
    http::error::Result,
    signing::eip712::{
        CTF_COLLATERAL_ADAPTER, NEG_RISK_CTF_COLLATERAL_ADAPTER, POLYMARKET_COLLATERAL_TOKEN,
        parse_bytes32,
    },
};

sol! {
    function splitPosition(
        address collateralToken,
        bytes32 parentCollectionId,
        bytes32 conditionId,
        uint256[] partition,
        uint256 amount
    );

    function mergePositions(
        address collateralToken,
        bytes32 parentCollectionId,
        bytes32 conditionId,
        uint256[] partition,
        uint256 amount
    );

    function redeemPositions(
        address collateralToken,
        bytes32 parentCollectionId,
        bytes32 conditionId,
        uint256[] indexSets
    );
}

const PARENT_COLLECTION_ID: B256 = B256::ZERO;
const BINARY_INDEX_SETS: [U256; 2] = [
    U256::from_limbs([1, 0, 0, 0]),
    U256::from_limbs([2, 0, 0, 0]),
];

/// Encoded collateral-adapter call for a position operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionCall {
    /// Collateral adapter that receives the call.
    pub target: Address,
    /// ABI-encoded function call.
    pub data: Bytes,
}

/// Returns the canonical collateral adapter for a standard or negative-risk market.
#[must_use]
pub const fn collateral_adapter(neg_risk: bool) -> Address {
    if neg_risk {
        NEG_RISK_CTF_COLLATERAL_ADAPTER
    } else {
        CTF_COLLATERAL_ADAPTER
    }
}

/// Encodes a `splitPosition` call for `amount` pUSD.
///
/// # Errors
///
/// Returns an error if `condition_id` is not a 32-byte hex value or `amount`
/// is not an exact positive six-decimal pUSD quantity.
pub fn encode_split_position(
    condition_id: &str,
    amount: Decimal,
    neg_risk: bool,
) -> Result<PositionCall> {
    let condition_id = parse_condition_id(condition_id)?;
    let amount = pusd_to_base_units(amount)?;
    Ok(PositionCall {
        target: collateral_adapter(neg_risk),
        data: Bytes::from(
            splitPositionCall {
                collateralToken: POLYMARKET_COLLATERAL_TOKEN,
                parentCollectionId: PARENT_COLLECTION_ID,
                conditionId: condition_id,
                partition: BINARY_INDEX_SETS.to_vec(),
                amount,
            }
            .abi_encode(),
        ),
    })
}

/// Encodes a `mergePositions` call for `amount` pUSD of complete sets.
///
/// # Errors
///
/// Returns an error if `condition_id` is not a 32-byte hex value or `amount`
/// is not an exact positive six-decimal pUSD quantity.
pub fn encode_merge_positions(
    condition_id: &str,
    amount: Decimal,
    neg_risk: bool,
) -> Result<PositionCall> {
    let condition_id = parse_condition_id(condition_id)?;
    let amount = pusd_to_base_units(amount)?;
    Ok(PositionCall {
        target: collateral_adapter(neg_risk),
        data: Bytes::from(
            mergePositionsCall {
                collateralToken: POLYMARKET_COLLATERAL_TOKEN,
                parentCollectionId: PARENT_COLLECTION_ID,
                conditionId: condition_id,
                partition: BINARY_INDEX_SETS.to_vec(),
                amount,
            }
            .abi_encode(),
        ),
    })
}

/// Encodes a `redeemPositions` call for both binary index sets.
///
/// # Errors
///
/// Returns an error if `condition_id` is not a 32-byte hex value.
pub fn encode_redeem_positions(condition_id: &str, neg_risk: bool) -> Result<PositionCall> {
    let condition_id = parse_condition_id(condition_id)?;
    Ok(PositionCall {
        target: collateral_adapter(neg_risk),
        data: Bytes::from(
            redeemPositionsCall {
                collateralToken: POLYMARKET_COLLATERAL_TOKEN,
                parentCollectionId: PARENT_COLLECTION_ID,
                conditionId: condition_id,
                indexSets: BINARY_INDEX_SETS.to_vec(),
            }
            .abi_encode(),
        ),
    })
}

fn parse_condition_id(condition_id: &str) -> Result<B256> {
    parse_bytes32(condition_id, "condition_id")
}

#[cfg(test)]
mod tests {
    use alloy_primitives::keccak256;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    const CONDITION_ID: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";

    fn selector(signature: &str) -> [u8; 4] {
        let hash = keccak256(signature.as_bytes());
        [hash[0], hash[1], hash[2], hash[3]]
    }

    #[rstest]
    fn test_collateral_adapter_targets() {
        assert_eq!(collateral_adapter(false), CTF_COLLATERAL_ADAPTER);
        assert_eq!(collateral_adapter(true), NEG_RISK_CTF_COLLATERAL_ADAPTER);
        assert_ne!(CTF_COLLATERAL_ADAPTER, NEG_RISK_CTF_COLLATERAL_ADAPTER);
    }

    #[rstest]
    fn test_encode_split_standard_fixture() {
        let call = encode_split_position(CONDITION_ID, dec!(1), false).unwrap();
        assert_eq!(call.target, CTF_COLLATERAL_ADAPTER);
        assert_eq!(
            &call.data[..4],
            selector("splitPosition(address,bytes32,bytes32,uint256[],uint256)")
        );
        assert_eq!(
            format!("0x{}", alloy_primitives::hex::encode(&call.data)),
            concat!(
                "0x72ce4275",
                "000000000000000000000000c011a7e12a19f7b1f670d46f03b03f3342e82dfb",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "1111111111111111111111111111111111111111111111111111111111111111",
                "00000000000000000000000000000000000000000000000000000000000000a0",
                "00000000000000000000000000000000000000000000000000000000000f4240",
                "0000000000000000000000000000000000000000000000000000000000000002",
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000000000000000000000000000002",
            )
        );
    }

    #[rstest]
    fn test_encode_merge_neg_risk_fixture() {
        let call = encode_merge_positions(CONDITION_ID, dec!(1), true).unwrap();
        assert_eq!(call.target, NEG_RISK_CTF_COLLATERAL_ADAPTER);
        assert_eq!(
            &call.data[..4],
            selector("mergePositions(address,bytes32,bytes32,uint256[],uint256)")
        );
        assert_eq!(
            format!("0x{}", alloy_primitives::hex::encode(&call.data)),
            concat!(
                "0x9e7212ad",
                "000000000000000000000000c011a7e12a19f7b1f670d46f03b03f3342e82dfb",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "1111111111111111111111111111111111111111111111111111111111111111",
                "00000000000000000000000000000000000000000000000000000000000000a0",
                "00000000000000000000000000000000000000000000000000000000000f4240",
                "0000000000000000000000000000000000000000000000000000000000000002",
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000000000000000000000000000002",
            )
        );
    }

    #[rstest]
    fn test_encode_redeem_standard_and_neg_risk_share_calldata() {
        let standard = encode_redeem_positions(CONDITION_ID, false).unwrap();
        let neg_risk = encode_redeem_positions(CONDITION_ID, true).unwrap();
        assert_eq!(standard.target, CTF_COLLATERAL_ADAPTER);
        assert_eq!(neg_risk.target, NEG_RISK_CTF_COLLATERAL_ADAPTER);
        assert_eq!(standard.data, neg_risk.data);
        assert_eq!(
            &standard.data[..4],
            selector("redeemPositions(address,bytes32,bytes32,uint256[])")
        );
        assert_eq!(
            format!("0x{}", alloy_primitives::hex::encode(&standard.data)),
            concat!(
                "0x01b7037c",
                "000000000000000000000000c011a7e12a19f7b1f670d46f03b03f3342e82dfb",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "1111111111111111111111111111111111111111111111111111111111111111",
                "0000000000000000000000000000000000000000000000000000000000000080",
                "0000000000000000000000000000000000000000000000000000000000000002",
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000000000000000000000000000002",
            )
        );
    }

    #[rstest]
    fn test_encode_rejects_invalid_condition_id() {
        let err = encode_split_position("not-a-condition", dec!(1), false).unwrap_err();
        assert!(err.to_string().contains("Invalid condition_id"));
    }
}
