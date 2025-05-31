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

use serde::{Deserialize, Deserializer};

use crate::defi::{chain::Chain, hex::deserialize_hex_number};

/// Represents a transaction on an EVM based blockchain.
#[derive(Debug, Clone, Deserialize)]
pub struct Transaction {
    /// The blockchain network identifier where this transaction occurred.
    #[serde(rename = "chainId", deserialize_with = "deserialize_chain")]
    pub chain: Chain,
    /// The unique identifier (hash) of the transaction.
    pub hash: String,
    /// The hash of the block containing this transaction.
    #[serde(rename = "blockHash")]
    pub block_hash: String,
    /// The block number in which this transaction was included.
    #[serde(rename = "blockNumber", deserialize_with = "deserialize_hex_number")]
    pub block_number: u64,
    /// The address of the sender (transaction originator).
    pub from: String,
    /// The address of the recipient.
    pub to: String,
    /// The amount of Ether transferred in the transaction, in wei.
    #[serde(deserialize_with = "deserialize_hex_number")]
    pub value: u64,
    /// The index of the transaction within its containing block.
    #[serde(
        rename = "transactionIndex",
        deserialize_with = "deserialize_hex_number"
    )]
    pub transaction_index: u64,
    /// The amount of gas allocated for transaction execution.
    #[serde(deserialize_with = "deserialize_hex_number")]
    pub gas: u64,
    /// The price of gas in wei.
    #[serde(rename = "gasPrice", deserialize_with = "deserialize_hex_number")]
    pub gas_price: u64,
}

impl Transaction {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        chain: Chain,
        hash: String,
        block_hash: String,
        block_number: u64,
        from: String,
        to: String,
        gas: u64,
        gas_price: u64,
        transaction_index: u64,
        value: u64,
    ) -> Self {
        Self {
            chain,
            hash,
            block_hash,
            block_number,
            from,
            to,
            gas,
            gas_price,
            transaction_index,
            value,
        }
    }
}

/// Custom deserializer function to convert a hex chain ID string to a Chain.
///
/// # Errors
///
/// Returns an error if parsing the hex string fails or the chain ID is unknown.
pub fn deserialize_chain<'de, D>(deserializer: D) -> Result<Chain, D::Error>
where
    D: Deserializer<'de>,
{
    let hex_string = String::deserialize(deserializer)?;
    let without_prefix = hex_string.trim_start_matches("0x");
    let chain_id = u32::from_str_radix(without_prefix, 16).map_err(serde::de::Error::custom)?;

    Chain::from_chain_id(chain_id)
        .cloned()
        .ok_or_else(|| serde::de::Error::custom(format!("Unknown chain ID: {}", chain_id)))
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;
    use crate::defi::{chain::Blockchain, rpc::RpcNodeHttpResponse};

    #[fixture]
    fn eth_rpc_response_eth_transfer_tx() -> String {
        // https://etherscan.io/tx/0x6d0b33a68953fdfa280a3a3d7a21e9513aed38d8587682f03728bc178b52b824
        r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "blockHash": "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0",
                "blockNumber": "0x154a1d6",
                "chainId": "0x1",
                "from": "0xd6a8749e224ecdfcc79d473d3355b1b0eb51d423",
                "gas": "0x5208",
                "gasPrice": "0x2d7a7174",
                "hash": "0x6d0b33a68953fdfa280a3a3d7a21e9513aed38d8587682f03728bc178b52b824",
                "input": "0x",
                "nonce": "0x0",
                "r": "0x6de16d6254956674d5075951a0a814e2333c6d430e9ab21113fd0c8a11ea8435",
                "s": "0x14c67075d1371f22936ee173d9fbd7e0284c37dd93e482df334be3a3dbd93fe9",
                "to": "0x3c9af20c7b7809a825373881f61b5a69ef8bc6bd",
                "transactionIndex": "0x99",
                "type": "0x0",
                "v": "0x25",
                "value": "0x5f5e100"
            }
        }"#
        .to_string()
    }

    #[fixture]
    fn eth_rpc_response_smart_contract_interaction_tx() -> String {
        // input field was omitted as it was too long and we don't need to parse it
        // https://etherscan.io/tx/0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57
        r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "accessList": [],
                "blockHash": "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0",
                "blockNumber": "0x154a1d6",
                "chainId": "0x1",
                "from": "0x2b711ee00b50d67667c4439c28aeaf7b75cb6e0d",
                "gas": "0xe4e1c0",
                "gasPrice": "0x536bc8dc",
                "hash": "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57",
                "maxFeePerGas": "0x559d2c91",
                "maxPriorityFeePerGas": "0x3b9aca00",
                "nonce": "0x4c5",
                "r": "0x65f9cf4bb1e53b0a9c04e75f8ffb3d62872d872944d660056a5ebb92a2620e0c",
                "s": "0x3dbab5a679327019488237def822f38566cad066ea50be5f53bc06d741a9404e",
                "to": "0x8c0bfc04ada21fd496c55b8c50331f904306f564",
                "transactionIndex": "0x4a",
                "type": "0x2",
                "v": "0x1",
                "value": "0x0",
                "yParity": "0x1"
            }
        }"#
        .to_string()
    }

    #[rstest]
    fn test_eth_transfer_tx(eth_rpc_response_eth_transfer_tx: String) {
        let tx = match serde_json::from_str::<RpcNodeHttpResponse<Transaction>>(
            &eth_rpc_response_eth_transfer_tx,
        ) {
            Ok(rpc_response) => rpc_response.result,
            Err(e) => panic!("Failed to deserialize transaction RPC response: {}", e),
        };
        assert_eq!(tx.chain.name, Blockchain::Ethereum);
        assert_eq!(
            tx.hash,
            "0x6d0b33a68953fdfa280a3a3d7a21e9513aed38d8587682f03728bc178b52b824"
        );
        assert_eq!(
            tx.block_hash,
            "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0"
        );
        assert_eq!(tx.block_number, 22323670);
        assert_eq!(tx.from, "0xd6a8749e224ecdfcc79d473d3355b1b0eb51d423");
        assert_eq!(tx.to, "0x3c9af20c7b7809a825373881f61b5a69ef8bc6bd");
        assert_eq!(tx.gas, 21000);
        assert_eq!(tx.gas_price, 762999156);
        assert_eq!(tx.transaction_index, 153);
        assert_eq!(tx.value, 100000000);
    }

    #[rstest]
    fn test_smart_contract_interaction_tx(eth_rpc_response_smart_contract_interaction_tx: String) {
        let tx = match serde_json::from_str::<RpcNodeHttpResponse<Transaction>>(
            &eth_rpc_response_smart_contract_interaction_tx,
        ) {
            Ok(rpc_response) => rpc_response.result,
            Err(e) => panic!("Failed to deserialize transaction RPC response: {}", e),
        };
        assert_eq!(tx.chain.name, Blockchain::Ethereum);
        assert_eq!(
            tx.hash,
            "0x6ba6dd4a82101d8a0387f4cb4ce57a2eb64a1e1bd0679a9d4ea8448a27004a57"
        );
        assert_eq!(
            tx.block_hash,
            "0xfdba50e306d1b0ebd1971ec0440799b324229841637d8c56afbd1d6950bb09f0"
        );
        assert_eq!(tx.from, "0x2b711ee00b50d67667c4439c28aeaf7b75cb6e0d");
        assert_eq!(tx.to, "0x8c0bfc04ada21fd496c55b8c50331f904306f564");
        assert_eq!(tx.gas, 15000000);
        assert_eq!(tx.gas_price, 1399572700);
        assert_eq!(tx.transaction_index, 74);
        assert_eq!(tx.value, 0);
    }
}
