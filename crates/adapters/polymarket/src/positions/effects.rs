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

//! On-chain effects of a position operation for a tracked Deposit Wallet.
//!
//! Effects come from canonical token transfers only: ERC-1155 transfers on the Conditional
//! Tokens contract and ERC-20 transfers on the pUSD collateral token. Adapter and Router
//! business events are correlation aids and are never counted as a second transfer.

use std::collections::BTreeMap;

use alloy::{primitives::LogData, sol, sol_types::SolEvent};
use alloy_primitives::{Address, B256, Bytes, I256, U256};

use crate::{
    http::error::{Error, Result},
    signing::eip712::{CONDITIONAL_TOKENS, POLYMARKET_COLLATERAL_TOKEN, POSITION_MANAGER},
};

sol! {
    event TransferSingle(
        address indexed operator,
        address indexed from,
        address indexed to,
        uint256 id,
        uint256 value
    );

    event TransferBatch(
        address indexed operator,
        address indexed from,
        address indexed to,
        uint256[] ids,
        uint256[] values
    );

    event Transfer(address indexed from, address indexed to, uint256 value);
}

/// A confirmed on-chain transaction receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionReceipt {
    /// Transaction hash.
    pub transaction_hash: B256,
    /// Block number containing the transaction.
    pub block_number: u64,
    /// Whether the transaction executed successfully.
    pub status: bool,
    /// Logs emitted by the transaction in receipt order.
    pub logs: Vec<ReceiptLog>,
}

impl TransactionReceipt {
    /// Parses a JSON-RPC `eth_getTransactionReceipt` result.
    ///
    /// # Errors
    ///
    /// Returns an error if the receipt is not yet available, reverted without a status, or
    /// contains malformed fields.
    pub fn from_rpc(value: &serde_json::Value) -> Result<Self> {
        let Some(object) = value.as_object() else {
            return Err(Error::exchange(
                "transaction receipt is not available; the transaction may still be pending",
            ));
        };

        let transaction_hash = parse_b256(object.get("transactionHash"), "transactionHash")?;
        let block_number = parse_u64(object.get("blockNumber"), "blockNumber")?;
        let status = match parse_u64(object.get("status"), "status")? {
            0 => false,
            1 => true,
            other => {
                return Err(Error::decode(format!(
                    "transaction receipt has invalid status {other}"
                )));
            }
        };

        let logs = object
            .get("logs")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Error::decode("transaction receipt omitted logs"))?
            .iter()
            .map(ReceiptLog::from_rpc)
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            transaction_hash,
            block_number,
            status,
            logs,
        })
    }
}

/// One log included in a transaction receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptLog {
    /// Emitting contract address.
    pub address: Address,
    /// Event signature and indexed topics.
    pub topics: Vec<B256>,
    /// ABI-encoded non-indexed event data.
    pub data: Bytes,
    /// Position of the log within the block.
    pub log_index: u64,
}

impl ReceiptLog {
    fn from_rpc(value: &serde_json::Value) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| Error::decode("transaction receipt log is not an object"))?;

        let address = parse_address(object.get("address"), "address")?;
        let topics = object
            .get("topics")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Error::decode("transaction receipt log omitted topics"))?
            .iter()
            .map(|topic| parse_b256(Some(topic), "topic"))
            .collect::<Result<Vec<_>>>()?;
        let data = Bytes::from(parse_hex_bytes(object.get("data"), "data")?);
        let log_index = parse_u64(object.get("logIndex"), "logIndex")?;

        Ok(Self {
            address,
            topics,
            data,
            log_index,
        })
    }
}

/// A token quantity received or spent by the tracked wallet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TokenAmount {
    /// Conditional Tokens position identifier.
    pub token_id: U256,
    /// Token quantity in six-decimal base units.
    pub quantity: U256,
}

/// Net effect of a confirmed transaction on one tracked wallet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionOperationEffects {
    /// Transaction hash.
    pub transaction_hash: B256,
    /// Block number containing the transaction.
    pub block_number: u64,
    /// Outcome tokens received by the wallet, ordered by token ID.
    pub received: Vec<TokenAmount>,
    /// Outcome tokens spent by the wallet, ordered by token ID.
    pub spent: Vec<TokenAmount>,
    /// Net pUSD collateral change: positive for inflows, negative for outflows.
    pub collateral_delta: I256,
}

impl PositionOperationEffects {
    /// Parses the effects for `wallet` from a confirmed transaction receipt.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction reverted or a token amount cannot be represented.
    pub fn parse(receipt: &TransactionReceipt, wallet: Address) -> Result<Self> {
        if !receipt.status {
            return Err(Error::exchange(format!(
                "transaction {:#x} reverted",
                receipt.transaction_hash
            )));
        }

        let mut token_deltas: BTreeMap<U256, I256> = BTreeMap::new();
        let mut collateral_delta = I256::ZERO;

        for log in &receipt.logs {
            let log_data = LogData::new_unchecked(log.topics.clone(), log.data.clone());

            if log.address == CONDITIONAL_TOKENS || log.address == POSITION_MANAGER {
                match log.topics.first() {
                    Some(signature) if *signature == TransferSingle::SIGNATURE_HASH => {
                        let event = TransferSingle::decode_log_data(&log_data).map_err(|e| {
                            Error::decode(format!("invalid position token TransferSingle: {e}"))
                        })?;
                        add_token_delta(
                            &mut token_deltas,
                            wallet,
                            event.from,
                            event.to,
                            event.id,
                            event.value,
                        )?;
                    }
                    Some(signature) if *signature == TransferBatch::SIGNATURE_HASH => {
                        let event = TransferBatch::decode_log_data(&log_data).map_err(|e| {
                            Error::decode(format!("invalid position token TransferBatch: {e}"))
                        })?;
                        if event.ids.len() != event.values.len() {
                            return Err(Error::decode(
                                "position token TransferBatch has mismatched id and value counts",
                            ));
                        }
                        for (id, value) in event.ids.into_iter().zip(event.values) {
                            add_token_delta(
                                &mut token_deltas,
                                wallet,
                                event.from,
                                event.to,
                                id,
                                value,
                            )?;
                        }
                    }
                    _ => {}
                }
            } else if log.address == POLYMARKET_COLLATERAL_TOKEN
                && log.topics.first() == Some(&Transfer::SIGNATURE_HASH)
            {
                let event = Transfer::decode_log_data(&log_data)
                    .map_err(|e| Error::decode(format!("invalid pUSD collateral Transfer: {e}")))?;
                collateral_delta = add_signed(
                    collateral_delta,
                    delta_for_wallet(wallet, event.from, event.to, event.value)?,
                )?;
            }
        }

        let mut received = Vec::new();
        let mut spent = Vec::new();
        for (token_id, delta) in token_deltas {
            if delta.is_positive() {
                received.push(TokenAmount {
                    token_id,
                    quantity: delta.unsigned_abs(),
                });
            } else if delta.is_negative() {
                spent.push(TokenAmount {
                    token_id,
                    quantity: delta.unsigned_abs(),
                });
            }
        }

        Ok(Self {
            transaction_hash: receipt.transaction_hash,
            block_number: receipt.block_number,
            received,
            spent,
            collateral_delta,
        })
    }
}

fn add_token_delta(
    deltas: &mut BTreeMap<U256, I256>,
    wallet: Address,
    from: Address,
    to: Address,
    token_id: U256,
    value: U256,
) -> Result<()> {
    let delta = delta_for_wallet(wallet, from, to, value)?;
    if delta.is_zero() {
        return Ok(());
    }

    let entry = deltas.entry(token_id).or_insert(I256::ZERO);
    *entry = add_signed(*entry, delta)?;
    Ok(())
}

fn delta_for_wallet(wallet: Address, from: Address, to: Address, value: U256) -> Result<I256> {
    let value = I256::try_from(value)
        .map_err(|_| Error::decode("token amount exceeds the signed 256-bit range"))?;

    let mut delta = I256::ZERO;
    if from == wallet {
        delta = delta
            .checked_sub(value)
            .ok_or_else(|| Error::decode("token amount underflowed"))?;
    }
    if to == wallet {
        delta = delta
            .checked_add(value)
            .ok_or_else(|| Error::decode("token amount overflowed"))?;
    }
    Ok(delta)
}

fn add_signed(left: I256, right: I256) -> Result<I256> {
    left.checked_add(right)
        .ok_or_else(|| Error::decode("token amount overflowed"))
}

fn parse_b256(value: Option<&serde_json::Value>, field: &str) -> Result<B256> {
    let bytes = parse_hex_bytes(value, field)?;
    if bytes.len() != 32 {
        return Err(Error::decode(format!(
            "transaction receipt field {field} must be 32 bytes"
        )));
    }
    Ok(B256::from_slice(&bytes))
}

fn parse_address(value: Option<&serde_json::Value>, field: &str) -> Result<Address> {
    let bytes = parse_hex_bytes(value, field)?;
    if bytes.len() != 20 {
        return Err(Error::decode(format!(
            "transaction receipt field {field} must be 20 bytes"
        )));
    }
    Ok(Address::from_slice(&bytes))
}

fn parse_hex_bytes(value: Option<&serde_json::Value>, field: &str) -> Result<Vec<u8>> {
    let text = value
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::decode(format!("transaction receipt field {field} is missing")))?;
    let text = text
        .strip_prefix("0x")
        .ok_or_else(|| Error::decode(format!("transaction receipt field {field} is not hex")))?;
    alloy_primitives::hex::decode(text)
        .map_err(|_| Error::decode(format!("transaction receipt field {field} is not hex")))
}

fn parse_u64(value: Option<&serde_json::Value>, field: &str) -> Result<u64> {
    let text = value
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::decode(format!("transaction receipt field {field} is missing")))?;
    let text = text
        .strip_prefix("0x")
        .ok_or_else(|| Error::decode(format!("transaction receipt field {field} is not hex")))?;
    u64::from_str_radix(text, 16)
        .map_err(|_| Error::decode(format!("transaction receipt field {field} is not a number")))
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, hex};
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    const WALLET: Address = address!("0x1111111111111111111111111111111111111111");
    const OTHER: Address = address!("0x2222222222222222222222222222222222222222");

    fn topic(address: Address) -> String {
        format!("0x{}{}", "0".repeat(24), hex::encode(address))
    }

    fn receipt(logs: &[serde_json::Value]) -> serde_json::Value {
        json!({
            "transactionHash": format!("0x{}", "ab".repeat(32)),
            "blockNumber": "0x10",
            "status": "0x1",
            "logs": logs,
        })
    }

    fn transfer_single(
        contract: Address,
        from: Address,
        to: Address,
        token_id: u64,
        value: u64,
    ) -> serde_json::Value {
        let data = format!("0x{token_id:064x}{value:064x}");
        json!({
            "address": format!("{contract:#x}"),
            "topics": [
                format!("{:#x}", TransferSingle::SIGNATURE_HASH),
                topic(OTHER),
                topic(from),
                topic(to),
            ],
            "data": data,
            "logIndex": "0x0",
        })
    }

    fn transfer_batch(
        from: Address,
        to: Address,
        ids: &[u64],
        values: &[u64],
    ) -> serde_json::Value {
        let encoded = encode_batch(ids, values);
        json!({
            "address": format!("{CONDITIONAL_TOKENS:#x}"),
            "topics": [
                format!("{:#x}", TransferBatch::SIGNATURE_HASH),
                topic(OTHER),
                topic(from),
                topic(to),
            ],
            "data": encoded,
            "logIndex": "0x1",
        })
    }

    fn encode_batch(ids: &[u64], values: &[u64]) -> String {
        let mut words: Vec<String> = Vec::new();
        let ids_offset = 0x40u64;
        let values_offset = ids_offset + 32 + 32 * u64::try_from(ids.len()).expect("id count fits");
        // Head: offsets for ids and values (two dynamic arrays).
        words.push(format!("{ids_offset:064x}"));
        words.push(format!("{values_offset:064x}"));
        for values_ref in [ids, values] {
            words.push(format!("{:064x}", values_ref.len()));
            for value in values_ref {
                words.push(format!("{value:064x}"));
            }
        }
        format!("0x{}", words.concat())
    }

    fn collateral_transfer(from: Address, to: Address, value: u64) -> serde_json::Value {
        json!({
            "address": format!("{POLYMARKET_COLLATERAL_TOKEN:#x}"),
            "topics": [
                format!("{:#x}", Transfer::SIGNATURE_HASH),
                topic(from),
                topic(to),
            ],
            "data": format!("0x{value:064x}"),
            "logIndex": "0x2",
        })
    }

    #[rstest]
    fn test_parse_convert_effects() {
        let value = receipt(&[
            transfer_single(CONDITIONAL_TOKENS, WALLET, OTHER, 10, 1_000),
            transfer_single(CONDITIONAL_TOKENS, OTHER, WALLET, 11, 900),
            collateral_transfer(OTHER, WALLET, 900),
        ]);
        let receipt = TransactionReceipt::from_rpc(&value).unwrap();
        let effects = PositionOperationEffects::parse(&receipt, WALLET).unwrap();

        assert_eq!(
            effects.spent,
            vec![TokenAmount {
                token_id: U256::from(10u8),
                quantity: U256::from(1_000u64),
            }]
        );
        assert_eq!(
            effects.received,
            vec![TokenAmount {
                token_id: U256::from(11u8),
                quantity: U256::from(900u64),
            }]
        );
        assert_eq!(effects.collateral_delta, I256::try_from(900u64).unwrap());
    }

    #[rstest]
    fn test_parse_v2_position_manager_transfers() {
        let value = receipt(&[
            transfer_single(POSITION_MANAGER, WALLET, OTHER, 1, 500),
            transfer_single(POSITION_MANAGER, OTHER, WALLET, 2, 400),
            collateral_transfer(OTHER, WALLET, 400),
        ]);
        let receipt = TransactionReceipt::from_rpc(&value).unwrap();
        let effects = PositionOperationEffects::parse(&receipt, WALLET).unwrap();

        assert_eq!(
            effects.spent,
            vec![TokenAmount {
                token_id: U256::from(1u8),
                quantity: U256::from(500u64),
            }]
        );
        assert_eq!(
            effects.received,
            vec![TokenAmount {
                token_id: U256::from(2u8),
                quantity: U256::from(400u64),
            }]
        );
        assert_eq!(effects.collateral_delta, I256::try_from(400u64).unwrap());
    }

    #[rstest]
    fn test_parse_batch_effects_and_net_duplicate_tokens() {
        let value = receipt(&[
            transfer_batch(WALLET, OTHER, &[1, 2], &[10, 20]),
            transfer_single(CONDITIONAL_TOKENS, OTHER, WALLET, 1, 3),
        ]);
        let receipt = TransactionReceipt::from_rpc(&value).unwrap();
        let effects = PositionOperationEffects::parse(&receipt, WALLET).unwrap();

        assert_eq!(
            effects.spent,
            vec![
                TokenAmount {
                    token_id: U256::from(1u8),
                    quantity: U256::from(7u8),
                },
                TokenAmount {
                    token_id: U256::from(2u8),
                    quantity: U256::from(20u8),
                },
            ]
        );
        assert!(effects.received.is_empty());
        assert_eq!(effects.collateral_delta, I256::ZERO);
    }

    #[rstest]
    fn test_parse_ignores_unrelated_contracts_and_wallets() {
        let value = receipt(&[
            transfer_single(OTHER, WALLET, OTHER, 1, 100),
            transfer_single(OTHER, OTHER, WALLET, 2, 100),
            collateral_transfer(OTHER, OTHER, 100),
        ]);
        let receipt = TransactionReceipt::from_rpc(&value).unwrap();
        let effects = PositionOperationEffects::parse(&receipt, WALLET).unwrap();

        assert!(effects.received.is_empty());
        assert!(effects.spent.is_empty());
        assert_eq!(effects.collateral_delta, I256::ZERO);
    }

    #[rstest]
    fn test_parse_split_outflow_collateral_is_negative() {
        let value = receipt(&[
            transfer_single(CONDITIONAL_TOKENS, Address::ZERO, WALLET, 1, 1_000),
            transfer_single(CONDITIONAL_TOKENS, Address::ZERO, WALLET, 2, 1_000),
            collateral_transfer(WALLET, OTHER, 1_000),
        ]);
        let receipt = TransactionReceipt::from_rpc(&value).unwrap();
        let effects = PositionOperationEffects::parse(&receipt, WALLET).unwrap();

        assert_eq!(effects.received.len(), 2);
        assert_eq!(effects.collateral_delta, I256::try_from(-1_000i64).unwrap());
    }

    #[rstest]
    fn test_receipt_rejects_reverted_transaction() {
        let mut value = receipt(&[]);
        value["status"] = json!("0x0");
        let receipt = TransactionReceipt::from_rpc(&value).unwrap();

        let err = PositionOperationEffects::parse(&receipt, WALLET).unwrap_err();
        assert!(err.to_string().contains("reverted"));
    }

    #[rstest]
    fn test_receipt_rejects_missing_and_malformed_fields() {
        assert!(TransactionReceipt::from_rpc(&json!(null)).is_err());
        assert!(TransactionReceipt::from_rpc(&json!({})).is_err());

        let mut value = receipt(&[]);
        value["status"] = json!("0x2");
        assert!(TransactionReceipt::from_rpc(&value).is_err());

        let mut value = receipt(&[transfer_single(CONDITIONAL_TOKENS, WALLET, OTHER, 1, 1)]);
        value["logs"][0]["topics"][0] = json!("0xzz");
        assert!(TransactionReceipt::from_rpc(&value).is_err());

        let mut value = receipt(&[transfer_single(CONDITIONAL_TOKENS, WALLET, OTHER, 1, 1)]);
        value["logs"][0]["data"] = json!("0x00");
        let receipt = TransactionReceipt::from_rpc(&value).unwrap();
        assert!(PositionOperationEffects::parse(&receipt, WALLET).is_err());
    }
}
