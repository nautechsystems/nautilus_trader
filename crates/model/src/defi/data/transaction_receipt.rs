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

use alloy_primitives::{Address, U256};
use serde::Deserialize;

use crate::defi::hex::{deserialize_hex_number, deserialize_opt_hex_u64, deserialize_opt_hex_u256};

/// Represents a log entry included in a transaction receipt.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct ReceiptLog {
    /// Address of the contract that emitted this log.
    pub address: Address,
    /// Indexed event parameters.
    pub topics: Vec<String>,
    /// Non-indexed event data.
    pub data: String,
    /// The log index in the block.
    #[serde(default, deserialize_with = "deserialize_opt_hex_u64")]
    pub log_index: Option<u64>,
    /// The transaction index in the block.
    #[serde(default, deserialize_with = "deserialize_opt_hex_u64")]
    pub transaction_index: Option<u64>,
    /// The parent transaction hash for this log.
    #[serde(default)]
    pub transaction_hash: Option<String>,
    /// The containing block hash.
    #[serde(default)]
    pub block_hash: Option<String>,
    /// The containing block number.
    #[serde(default, deserialize_with = "deserialize_opt_hex_u64")]
    pub block_number: Option<u64>,
    /// Whether this log was removed due to a chain reorg.
    #[serde(default)]
    pub removed: Option<bool>,
}

/// Represents a mined transaction receipt from an EVM node.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct TransactionReceipt {
    /// Hash of the transaction.
    pub transaction_hash: String,
    /// Hash of the block containing the transaction.
    pub block_hash: String,
    /// Number of the block containing the transaction.
    #[serde(deserialize_with = "deserialize_hex_number")]
    pub block_number: u64,
    /// Sender address.
    pub from: Address,
    /// Recipient address (None for contract-creation transactions).
    #[serde(default)]
    pub to: Option<Address>,
    /// Contract address created by a contract-creation transaction.
    #[serde(default)]
    pub contract_address: Option<Address>,
    /// Cumulative gas used in the block after this transaction.
    #[serde(deserialize_with = "deserialize_hex_number")]
    pub cumulative_gas_used: u64,
    /// Gas used by this transaction.
    #[serde(deserialize_with = "deserialize_hex_number")]
    pub gas_used: u64,
    /// Effective gas price (EIP-1559).
    #[serde(default, deserialize_with = "deserialize_opt_hex_u256")]
    pub effective_gas_price: Option<U256>,
    /// Transaction execution status (`1` success, `0` failure).
    #[serde(deserialize_with = "deserialize_hex_number")]
    pub status: u64,
    /// Decoded logs emitted by this transaction.
    #[serde(default)]
    pub logs: Vec<ReceiptLog>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::defi::rpc::RpcNodeHttpResponse;

    #[rstest]
    fn test_transaction_receipt_deserialize_with_logs() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "transactionHash": "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
                "blockHash": "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0",
                "blockNumber": "0x154a1d6",
                "from": "0x2b711ee00b50d67667c4439c28aeaf7b75cb6e0d",
                "to": "0x8c0bfc04ada21fd496c55b8c50331f904306f564",
                "cumulativeGasUsed": "0x992832",
                "gasUsed": "0x2dc6c",
                "effectiveGasPrice": "0x559d2c91",
                "status": "0x1",
                "logs": [
                    {
                        "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                        "topics": [
                            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
                        ],
                        "data": "0x00000000000000000000000000000000000000000000000000000000000003e8",
                        "logIndex": "0x2",
                        "transactionIndex": "0x4a",
                        "transactionHash": "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
                        "blockHash": "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0",
                        "blockNumber": "0x154a1d6",
                        "removed": false
                    }
                ]
            }
        }"#;

        let receipt = serde_json::from_str::<RpcNodeHttpResponse<TransactionReceipt>>(json)
            .expect("Failed to parse receipt")
            .result
            .expect("Missing receipt result");

        assert_eq!(
            receipt.transaction_hash,
            "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57"
        );
        assert_eq!(receipt.block_number, 22_323_670);
        assert_eq!(receipt.gas_used, 187_500);
        assert_eq!(receipt.status, 1);
        assert_eq!(
            receipt.effective_gas_price,
            Some(U256::from(1_436_363_921u64))
        );
        assert_eq!(receipt.logs.len(), 1);
        assert_eq!(receipt.logs[0].log_index, Some(2));
        assert_eq!(receipt.logs[0].transaction_index, Some(74));
    }

    #[rstest]
    fn test_transaction_receipt_deserialize_failure_status_and_null_to() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "transactionHash": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                "blockHash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                "blockNumber": "0x1",
                "from": "0x0000000000000000000000000000000000000001",
                "to": null,
                "contractAddress": "0x00000000000000000000000000000000000000aa",
                "cumulativeGasUsed": "0x5208",
                "gasUsed": "0x5208",
                "status": "0x0",
                "logs": []
            }
        }"#;

        let receipt = serde_json::from_str::<RpcNodeHttpResponse<TransactionReceipt>>(json)
            .expect("Failed to parse receipt")
            .result
            .expect("Missing receipt result");

        assert_eq!(receipt.status, 0);
        assert!(receipt.to.is_none());
        assert_eq!(
            receipt.contract_address,
            Some(Address::from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xaa]))
        );
        assert!(receipt.effective_gas_price.is_none());
        assert!(receipt.logs.is_empty());
    }

    #[rstest]
    fn test_transaction_receipt_deserialize_with_missing_contract_address() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "transactionHash": "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
                "blockHash": "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0",
                "blockNumber": "0x154a1d6",
                "from": "0x2b711ee00b50d67667c4439c28aeaf7b75cb6e0d",
                "to": "0x8c0bfc04ada21fd496c55b8c50331f904306f564",
                "cumulativeGasUsed": "0x992832",
                "gasUsed": "0x2dc6c",
                "status": "0x1",
                "logs": []
            }
        }"#;

        let receipt = serde_json::from_str::<RpcNodeHttpResponse<TransactionReceipt>>(json)
            .expect("Failed to parse receipt")
            .result
            .expect("Missing receipt result");

        assert!(receipt.contract_address.is_none());
    }

    #[rstest]
    fn test_transaction_receipt_log_optional_indexes_allow_null_and_missing() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "transactionHash": "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
                "blockHash": "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0",
                "blockNumber": "0x154a1d6",
                "from": "0x2b711ee00b50d67667c4439c28aeaf7b75cb6e0d",
                "to": "0x8c0bfc04ada21fd496c55b8c50331f904306f564",
                "cumulativeGasUsed": "0x992832",
                "gasUsed": "0x2dc6c",
                "status": "0x1",
                "logs": [
                    {
                        "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                        "topics": [
                            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
                        ],
                        "data": "0x00",
                        "logIndex": null,
                        "transactionIndex": null
                    },
                    {
                        "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                        "topics": [
                            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
                        ],
                        "data": "0x00"
                    }
                ]
            }
        }"#;

        let receipt = serde_json::from_str::<RpcNodeHttpResponse<TransactionReceipt>>(json)
            .expect("Failed to parse receipt")
            .result
            .expect("Missing receipt result");

        assert_eq!(receipt.logs.len(), 2);
        assert_eq!(receipt.logs[0].log_index, None);
        assert_eq!(receipt.logs[0].transaction_index, None);
        assert_eq!(receipt.logs[1].log_index, None);
        assert_eq!(receipt.logs[1].transaction_index, None);
    }
}
