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

//! ABI encoding for Polymarket CTF and Protocol V2 position operations.

use alloy::{primitives::Bytes, sol, sol_types::SolCall};
use alloy_primitives::{Address, B256, FixedBytes, U256, hex};
use rust_decimal::Decimal;

use super::amounts::pusd_to_base_units;
use crate::{
    http::error::{Error, Result},
    signing::eip712::{
        CTF_COLLATERAL_ADAPTER, NEG_RISK_CTF_COLLATERAL_ADAPTER, POLYMARKET_COLLATERAL_TOKEN,
        PolymarketApproval, ROUTER, parse_bytes32,
    },
};

sol! {
    function approve(address spender, uint256 amount);
    function setApprovalForAll(address operator, bool approved);
    function split(bytes31 conditionId, uint256 amount);
    function merge(bytes31 conditionId, uint256 amount);
    function redeem(bytes31 conditionId, uint256 outcomeIndex, uint256 amount);

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

    function convertPositions(bytes32 marketId, uint256 indexSet, uint256 amount);
}

const PARENT_COLLECTION_ID: B256 = B256::ZERO;
const BINARY_INDEX_SETS: [U256; 2] = [
    U256::from_limbs([1, 0, 0, 0]),
    U256::from_limbs([2, 0, 0, 0]),
];

/// Encoded contract call for a position operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionCall {
    /// Contract that receives the call.
    pub target: Address,
    /// ABI-encoded function call.
    pub data: Bytes,
}

/// Encodes one approval for submission by the asset holder.
#[must_use]
pub fn encode_approval(approval: PolymarketApproval) -> PositionCall {
    match approval {
        PolymarketApproval::Collateral {
            contract,
            spender,
            amount,
        } => PositionCall {
            target: contract,
            data: Bytes::from(approveCall { spender, amount }.abi_encode()),
        },
        PolymarketApproval::ConditionalTokens {
            contract,
            operator,
            approved,
        } => PositionCall {
            target: contract,
            data: Bytes::from(setApprovalForAllCall { operator, approved }.abi_encode()),
        },
    }
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

/// Encodes a V2 Router split for an exact six-decimal pUSD amount.
///
/// # Errors
///
/// Returns an error if the V2 condition ID is invalid, or the amount is nonpositive or inexact.
pub fn encode_split_position_v2(condition_id: &str, amount: Decimal) -> Result<PositionCall> {
    let condition_id = parse_condition_id_v2(condition_id)?;
    let amount = pusd_to_base_units(amount)?;
    Ok(PositionCall {
        target: ROUTER,
        data: Bytes::from(
            splitCall {
                conditionId: condition_id,
                amount,
            }
            .abi_encode(),
        ),
    })
}

/// Encodes a V2 Router merge that consumes `amount` shares of each outcome.
///
/// # Errors
///
/// Returns an error if the V2 condition ID is invalid, or the amount is nonpositive or inexact.
pub fn encode_merge_positions_v2(condition_id: &str, amount: Decimal) -> Result<PositionCall> {
    let condition_id = parse_condition_id_v2(condition_id)?;
    let amount = pusd_to_base_units(amount)?;
    Ok(PositionCall {
        target: ROUTER,
        data: Bytes::from(
            mergeCall {
                conditionId: condition_id,
                amount,
            }
            .abi_encode(),
        ),
    })
}

/// Encodes a V2 Router redemption of `amount` shares of outcome `0` or `1`.
///
/// # Errors
///
/// Returns an error if the condition ID or outcome index is invalid, or the amount is nonpositive
/// or inexact.
pub fn encode_redeem_position_v2(
    condition_id: &str,
    outcome_index: u8,
    amount: Decimal,
) -> Result<PositionCall> {
    if outcome_index > 1 {
        return Err(Error::bad_request("V2 outcome_index must be 0 or 1"));
    }

    let condition_id = parse_condition_id_v2(condition_id)?;
    let amount = pusd_to_base_units(amount)?;
    Ok(PositionCall {
        target: ROUTER,
        data: Bytes::from(
            redeemCall {
                conditionId: condition_id,
                outcomeIndex: U256::from(outcome_index),
                amount,
            }
            .abi_encode(),
        ),
    })
}

/// Encodes a neg-risk CTF conversion of `index_set` NO positions for `amount` each.
///
/// # Errors
///
/// Returns an error if `market_id` is not a 32-byte hex value with a zero final byte, the index
/// set is empty, or `amount` is not an exact positive six-decimal quantity.
pub fn encode_convert_positions(
    market_id: &str,
    index_set: U256,
    amount: Decimal,
) -> Result<PositionCall> {
    let market_id = parse_neg_risk_market_id(market_id)?;

    if index_set.is_zero() {
        return Err(Error::bad_request(
            "neg-risk conversion index set must not be empty",
        ));
    }
    let amount = pusd_to_base_units(amount)?;
    Ok(PositionCall {
        target: NEG_RISK_CTF_COLLATERAL_ADAPTER,
        data: Bytes::from(
            convertPositionsCall {
                marketId: market_id,
                indexSet: index_set,
                amount,
            }
            .abi_encode(),
        ),
    })
}

pub(super) fn parse_condition_id_v2(condition_id: &str) -> Result<FixedBytes<31>> {
    let value = condition_id
        .strip_prefix("0x")
        .ok_or_else(|| Error::bad_request("V2 condition_id must be 0x-prefixed hex"))?;
    let bytes =
        hex::decode(value).map_err(|_| Error::bad_request("V2 condition_id must be valid hex"))?;

    if bytes.len() != 31 && bytes.len() != 32 {
        return Err(Error::bad_request(
            "V2 condition_id must contain 31 or 32 bytes",
        ));
    }

    if bytes.len() == 32 && bytes[31] != 0 {
        return Err(Error::bad_request(
            "V2 padded condition_id must have a zero final byte",
        ));
    }

    if !matches!(bytes[0], 1 | 2) {
        return Err(Error::bad_request(
            "V2 position operations require BinaryModule or NegRiskModule",
        ));
    }

    Ok(FixedBytes::from_slice(&bytes[..31]))
}

fn parse_condition_id(condition_id: &str) -> Result<B256> {
    parse_bytes32(condition_id, "condition_id")
}

fn parse_neg_risk_market_id(market_id: &str) -> Result<B256> {
    let market_id = parse_bytes32(market_id, "market_id")?;
    if market_id.as_slice()[31] != 0 {
        return Err(Error::bad_request(
            "neg-risk market_id must have a zero final byte",
        ));
    }
    Ok(market_id)
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

    #[rstest]
    #[case("split", "split(bytes31,uint256)")]
    #[case("merge", "merge(bytes31,uint256)")]
    #[case("redeem_yes", "redeem(bytes31,uint256,uint256)")]
    #[case("redeem_no", "redeem(bytes31,uint256,uint256)")]
    fn test_encode_v2_router_fixture(
        #[case] operation: &str,
        #[case] signature: &str,
        #[values(false, true)] padded: bool,
        #[values(1u8, 2u8)] module: u8,
    ) {
        let native = format!("0x{module:02x}{}", "23".repeat(30));

        let condition_id = if padded {
            format!("{native}00")
        } else {
            native
        };

        let call = match operation {
            "split" => encode_split_position_v2(&condition_id, dec!(1.234567)),
            "merge" => encode_merge_positions_v2(&condition_id, dec!(1.234567)),
            "redeem_yes" => encode_redeem_position_v2(&condition_id, 0, dec!(1.234567)),
            "redeem_no" => encode_redeem_position_v2(&condition_id, 1, dec!(1.234567)),
            _ => unreachable!(),
        }
        .unwrap();

        assert_eq!(
            format!("{:#x}", call.target),
            "0x12121212006e4cd160d18e3f00711da5c3372600"
        );
        assert_eq!(&call.data[..4], selector(signature));
        let mut expected = format!("{module:02x}{}00", "23".repeat(30));
        if operation.starts_with("redeem") {
            expected.push_str(&format!("{:064x}", u8::from(operation == "redeem_no")));
        }

        expected.push_str("000000000000000000000000000000000000000000000000000000000012d687");
        assert_eq!(hex::encode(&call.data[4..]), expected);
    }

    #[rstest]
    #[case("0x01")]
    #[case("01")]
    #[case("0xzz")]
    #[case("0x0111111111111111111111111111111111111111111111111111111111111101")]
    #[case("0x0311111111111111111111111111111111111111111111111111111111111100")]
    fn test_encode_v2_rejects_invalid_condition(#[case] condition_id: &str) {
        assert!(encode_split_position_v2(condition_id, dec!(1)).is_err());
        assert!(encode_merge_positions_v2(condition_id, dec!(1)).is_err());
        assert!(encode_redeem_position_v2(condition_id, 0, dec!(1)).is_err());
    }

    #[rstest]
    #[case(dec!(0))]
    #[case(dec!(-1))]
    #[case(dec!(0.0000001))]
    #[case(Decimal::MAX)]
    fn test_encode_v2_rejects_invalid_amount(#[case] amount: Decimal) {
        let condition_id = format!("0x01{}", "23".repeat(30));
        assert!(encode_split_position_v2(&condition_id, amount).is_err());
        assert!(encode_merge_positions_v2(&condition_id, amount).is_err());
        assert!(encode_redeem_position_v2(&condition_id, 1, amount).is_err());
    }

    #[rstest]
    #[case(2)]
    #[case(255)]
    fn test_encode_v2_rejects_invalid_outcome(#[case] outcome_index: u8) {
        let condition_id = format!("0x01{}", "23".repeat(30));
        assert_eq!(
            encode_redeem_position_v2(&condition_id, outcome_index, dec!(1))
                .unwrap_err()
                .to_string(),
            "bad request: V2 outcome_index must be 0 or 1"
        );
    }

    #[rstest]
    fn test_encode_convert_positions_fixture() {
        let market_id = format!("0x{}00", "11".repeat(31));
        let call = encode_convert_positions(&market_id, U256::from(0b101u8), dec!(1.5)).unwrap();

        assert_eq!(call.target, NEG_RISK_CTF_COLLATERAL_ADAPTER);
        assert_eq!(
            &call.data[..4],
            selector("convertPositions(bytes32,uint256,uint256)")
        );
        let selector_hex = hex::encode(selector("convertPositions(bytes32,uint256,uint256)"));
        let market_hex = format!("{}{}", "11".repeat(31), "00");
        let index_set_hex = format!("{:064x}", 5u8);
        let amount_hex = format!("{:064x}", 1_500_000u64);
        let expected = format!("0x{selector_hex}{market_hex}{index_set_hex}{amount_hex}");
        assert_eq!(format!("0x{}", hex::encode(&call.data)), expected);
    }

    #[rstest]
    #[case("0x1111111111111111111111111111111111111111111111111111111111111111")]
    #[case("0x11111111111111111111111111111111111111111111111111111111111111")]
    #[case("not-a-market")]
    fn test_encode_convert_positions_rejects_invalid_market_id(#[case] market_id: &str) {
        assert!(encode_convert_positions(market_id, U256::ONE, dec!(1)).is_err());
    }

    #[rstest]
    fn test_encode_convert_positions_rejects_empty_index_set_or_invalid_amount() {
        let market_id = format!("0x{}00", "11".repeat(31));
        assert!(encode_convert_positions(&market_id, U256::ZERO, dec!(1)).is_err());
        assert!(encode_convert_positions(&market_id, U256::ONE, dec!(0)).is_err());
        assert!(encode_convert_positions(&market_id, U256::ONE, dec!(1.0000001)).is_err());
    }
}
