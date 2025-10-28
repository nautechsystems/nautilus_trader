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

use std::fmt::Display;

use alloy_primitives::U256;
use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::defi::{
    Blockchain,
    hex::{
        deserialize_hex_number, deserialize_hex_timestamp, deserialize_opt_hex_u64,
        deserialize_opt_hex_u256,
    },
};

/// Represents the precise position of an event within a blockchain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockPosition {
    /// The block number (height) in the blockchain where the event occurred.
    pub number: u64,
    /// The unique hash identifier of the transaction containing the event.
    pub transaction_hash: String,
    /// The index position of the transaction within the block (0-based).
    pub transaction_index: u32,
    /// The index position of the log/event within the transaction (0-based).
    pub log_index: u32,
}

impl BlockPosition {
    /// Creates a new [`BlockPosition`] with the specified positioning data.
    pub fn new(number: u64, transaction_hash: String, index: u32, log_index: u32) -> Self {
        Self {
            number,
            transaction_hash,
            transaction_index: index,
            log_index,
        }
    }
}

/// Represents an Ethereum-compatible blockchain block with essential metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Block {
    /// The blockchain network this block is part of.
    #[serde(skip)]
    pub chain: Option<Blockchain>, // TODO: We should make this required eventually
    /// The unique identifier hash of the block.
    pub hash: String,
    /// The block height/number in the blockchain.
    #[serde(deserialize_with = "deserialize_hex_number")]
    pub number: u64,
    /// Hash of the parent block.
    pub parent_hash: String,
    /// Address of the miner or validator who produced this block.
    pub miner: Ustr,
    /// Maximum amount of gas allowed in this block.
    #[serde(deserialize_with = "deserialize_hex_number")]
    pub gas_limit: u64,
    /// Total gas actually used by all transactions in this block.
    #[serde(deserialize_with = "deserialize_hex_number")]
    pub gas_used: u64,
    /// EIP-1559 base fee per gas (wei); absent on pre-1559 or non-EIP chains.
    #[serde(default, deserialize_with = "deserialize_opt_hex_u256")]
    pub base_fee_per_gas: Option<U256>,
    /// Blob gas used in this block (EIP-4844); absent on chains without blobs.
    #[serde(default, deserialize_with = "deserialize_opt_hex_u256")]
    pub blob_gas_used: Option<U256>,
    /// Excess blob gas remaining after block execution (EIP-4844); None if not applicable.
    #[serde(default, deserialize_with = "deserialize_opt_hex_u256")]
    pub excess_blob_gas: Option<U256>,
    /// L1 gas price used for posting this block's calldata (wei); Arbitrum only.
    #[serde(default, deserialize_with = "deserialize_opt_hex_u256")]
    pub l1_gas_price: Option<U256>,
    /// L1 calldata gas units consumed when posting this block; Arbitrum only.
    #[serde(default, deserialize_with = "deserialize_opt_hex_u64")]
    pub l1_gas_used: Option<u64>,
    /// Fixed-point (1e-6) scalar applied to the raw L1 fee; Arbitrum only.
    #[serde(default, deserialize_with = "deserialize_opt_hex_u64")]
    pub l1_fee_scalar: Option<u64>,
    /// Unix timestamp when the block was created.
    #[serde(deserialize_with = "deserialize_hex_timestamp")]
    pub timestamp: UnixNanos,
}

impl Block {
    /// Creates a new [`Block`] instance with the specified properties.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        hash: String,
        parent_hash: String,
        number: u64,
        miner: Ustr,
        gas_limit: u64,
        gas_used: u64,
        timestamp: UnixNanos,
        chain: Option<Blockchain>,
    ) -> Self {
        Self {
            chain,
            hash,
            parent_hash,
            number,
            miner,
            gas_used,
            gas_limit,
            timestamp,
            base_fee_per_gas: None,
            blob_gas_used: None,
            excess_blob_gas: None,
            l1_gas_price: None,
            l1_gas_used: None,
            l1_fee_scalar: None,
        }
    }

    /// Returns the blockchain for this block.
    ///
    /// # Panics
    ///
    /// Panics if the `chain` has not been set.
    pub fn chain(&self) -> Blockchain {
        if let Some(chain) = self.chain {
            chain
        } else {
            panic!("Must have the `chain` field set")
        }
    }

    pub fn set_chain(&mut self, chain: Blockchain) {
        self.chain = Some(chain);
    }

    /// Sets the EIP-1559 base fee and returns `self` for chaining.
    #[must_use]
    pub fn with_base_fee(mut self, fee: U256) -> Self {
        self.base_fee_per_gas = Some(fee);
        self
    }

    /// Sets blob-gas metrics (EIP-4844) and returns `self` for chaining.
    #[must_use]
    pub fn with_blob_gas(mut self, used: U256, excess: U256) -> Self {
        self.blob_gas_used = Some(used);
        self.excess_blob_gas = Some(excess);
        self
    }

    /// Sets L1 fee components relevant for Arbitrum cost calculation and returns `self` for chaining.
    #[must_use]
    pub fn with_l1_fee_components(mut self, price: U256, gas_used: u64, scalar: u64) -> Self {
        self.l1_gas_price = Some(price);
        self.l1_gas_used = Some(gas_used);
        self.l1_fee_scalar = Some(scalar);
        self
    }
}

impl PartialEq for Block {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for Block {}

impl Display for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Block(chain={}, number={}, timestamp={}, hash={})",
            self.chain(),
            self.number,
            self.timestamp.to_rfc3339(),
            self.hash
        )
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::U256;
    use chrono::{TimeZone, Utc};
    use nautilus_core::UnixNanos;
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::Block;
    use crate::defi::{Blockchain, chain::chains, rpc::RpcNodeWssResponse};

    #[fixture]
    fn eth_rpc_block_response() -> String {
        // https://etherscan.io/block/22294175
        r#"{
        "jsonrpc":"2.0",
        "method":"eth_subscription",
        "params":{
            "subscription":"0xe06a2375238a4daa8ec823f585a0ef1e",
            "result":{
                "baseFeePerGas":"0x1862a795",
                "blobGasUsed":"0xc0000",
                "difficulty":"0x0",
                "excessBlobGas":"0x4840000",
                "extraData":"0x546974616e2028746974616e6275696c6465722e78797a29",
                "gasLimit":"0x223b4a1",
                "gasUsed":"0xde3909",
                "hash":"0x71ece187051700b814592f62774e6ebd8ebdf5efbb54c90859a7d1522ce38e0a",
                "miner":"0x4838b106fce9647bdf1e7877bf73ce8b0bad5f97",
                "mixHash":"0x43adbd4692459c8820b0913b0bc70e8a87bed2d40c395cc41059aa108a7cbe84",
                "nonce":"0x0000000000000000",
                "number":"0x1542e9f",
                "parentBeaconBlockRoot":"0x58673bf001b31af805fb7634fbf3257dde41fbb6ae05c71799b09632d126b5c7",
                "parentHash":"0x2abcce1ac985ebea2a2d6878a78387158f46de8d6db2cefca00ea36df4030a40",
                "receiptsRoot":"0x35fead0b79338d4acbbc361014521d227874a1e02d24342ed3e84460df91f271",
                "sha3Uncles":"0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                "stateRoot":"0x99f29ee8ed6622c6a1520dca86e361029605f76d2e09aa7d3b1f9fc8b0268b13",
                "timestamp":"0x6801f4bb",
                "transactionsRoot":"0x9484b18d38886f25a44b465ad0136c792ef67dd5863b102cab2ab7a76bfb707d",
                "withdrawalsRoot":"0x152f0040f4328639397494ef0d9c02d36c38b73f09588f304084e9f29662e9cb"
            }
         }
      }"#.to_string()
    }

    #[fixture]
    fn polygon_rpc_block_response() -> String {
        // https://polygonscan.com/block/70453741
        r#"{
        "jsonrpc": "2.0",
        "method": "eth_subscription",
        "params": {
            "subscription": "0x20f7c54c468149ed99648fd09268c903",
            "result": {
                "baseFeePerGas": "0x19e",
                "difficulty": "0x18",
                "gasLimit": "0x1c9c380",
                "gasUsed": "0x1270f14",
                "hash": "0x38ca655a2009e1748097f5559a0c20de7966243b804efeb53183614e4bebe199",
                "miner": "0x0000000000000000000000000000000000000000",
                "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "nonce": "0x0000000000000000",
                "number": "0x43309ed",
                "parentHash": "0xf25e108267e3d6e1e4aaf4e329872273f2b1ad6186a4a22e370623aa8d021c50",
                "receiptsRoot": "0xfffb93a991d15b9689536e59f20564cc49c254ec41a222d988abe58d2869968c",
                "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                "stateRoot": "0xe66a9bc516bde8fc7b8c1ba0b95bfea0f4574fc6cfe95c68b7f8ab3d3158278d",
                "timestamp": "0x680250d5",
                "totalDifficulty": "0x505bd180",
                "transactionsRoot": "0xd9ebc2fd5c7ce6f69ab2e427da495b0b0dff14386723b8c07b347449fd6293a6"
            }
          }
      }"#.to_string()
    }

    #[fixture]
    fn base_rpc_block_response() -> String {
        r#"{
        "jsonrpc":"2.0",
        "method":"eth_subscription",
        "params":{
            "subscription":"0xeb7d715d93964e22b2d99192791ca984",
            "result":{
                "baseFeePerGas":"0xaae54",
                "blobGasUsed":"0x0",
                "difficulty":"0x0",
                "excessBlobGas":"0x0",
                "extraData":"0x00000000fa00000002",
                "gasLimit":"0x7270e00",
                "gasUsed":"0x56fce26",
                "hash":"0x14575c65070d455e6d20d5ee17be124917a33ce4437dd8615a56d29e8279b7ad",
                "logsBloom":"0x02bcf67d7b87f2d884b8d56bbe3965f6becc9ed8f9637ffc67efdffcef446cf435ffec7e7ce8e4544fe782bb06ef37afc97687cbf3c7ee7e26dd12a8f1fd836bc17dd2fd64fce3ef03bc74d8faedb07dddafe6f2cedff3e6f5d8683cc2ef26f763dee76e7b6fdeeade8c8a7cec7a5fdca237be97be2efe67dc908df7ce3f94a3ce150b2a9f07776fa577d5c52dbffe5bfc38bbdfeefc305f0efaf37fba3a4cdabf366b17fcb3b881badbe571dfb2fd652e879fbf37e88dbedb6a6f9f4bb7aef528e81c1f3cda38f777cb0a2d6f0ddb8abcb3dda5d976541fa062dba6255a7b328b5fdf47e8d6fac2fc43d8bee5936e6e8f2bff33526fdf6637f3f2216d950fef",
                "miner":"0x4200000000000000000000000000000000000011",
                "mixHash":"0xeacd829463c5d21df523005d55f25a0ca20474f1310c5c7eb29ff2c479789e98",
                "nonce":"0x0000000000000000",
                "number":"0x1bca2ac",
                "parentBeaconBlockRoot":"0xfe4c48425a274a6716c569dfa9c238551330fc39d295123b12bc2461e6f41834",
                "parentHash":"0x9a6ad4ffb258faa47ecd5eea9e7a9d8fa1772aa6232bc7cb4bbad5bc30786258",
                "receiptsRoot":"0x5fc932dd358c33f9327a704585c83aafbe0d25d12b62c1cd8282df8b328aac16",
                "sha3Uncles":"0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                "stateRoot":"0xd2d3a6a219fb155bfc5afbde11f3161f1051d931432ccf32c33affe54176bb18",
                "timestamp":"0x6803a23b",
                "transactionsRoot":"0x59726fb9afc101cd49199c70bbdbc28385f4defa02949cb6e20493e16035a59d",
                "withdrawalsRoot":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"
            }
        }
      }"#.to_string()
    }

    #[fixture]
    fn arbitrum_rpc_block_response() -> String {
        // https://arbiscan.io/block/328014516
        r#"{
        "jsonrpc":"2.0",
        "method":"eth_subscription",
        "params":{
            "subscription":"0x0c5a0b38096440ef9a30a84837cf2012",
            "result":{
                "baseFeePerGas":"0x989680",
                "difficulty":"0x1",
                "extraData":"0xc66cd959dcdc1baf028efb61140d4461629c53c9643296cbda1c40723e97283b",
                "gasLimit":"0x4000000000000",
                "gasUsed":"0x17af4",
                "hash":"0x724a0af4720fd7624976f71b16163de25f8532e87d0e7058eb0c1d3f6da3c1f8",
                "miner":"0xa4b000000000000000000073657175656e636572",
                "mixHash":"0x0000000000023106000000000154528900000000000000200000000000000000",
                "nonce":"0x00000000001daa7c",
                "number":"0x138d1ab4",
                "parentHash":"0xe7176e201c2db109be479770074ad11b979de90ac850432ed38ed335803861b6",
                "receiptsRoot":"0xefb382e3a4e3169e57920fa2367fc81c98bbfbd13611f57767dee07d3b3f96d4",
                "sha3Uncles":"0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                "stateRoot":"0x57e5475675abf1ec4c763369342e327a04321d17eeaa730a4ca20a9cafeee380",
                "timestamp":"0x6803a606",
                "totalDifficulty":"0x123a3d6c",
                "transactionsRoot":"0x710b520177ecb31fa9092d16ee593b692070912b99ddd9fcf73eb4e9dd15193d"
            }
        }
      }"#.to_string()
    }

    #[rstest]
    fn test_block_set_chain() {
        let mut block = Block::new(
            "0x1234567890abcdef".to_string(),
            "0xabcdef1234567890".to_string(),
            12345,
            Ustr::from("0x742E4422b21FB8B4dF463F28689AC98bD56c39e0"),
            21000,
            20000,
            UnixNanos::from(1_640_995_200_000_000_000u64),
            None,
        );

        assert!(block.chain.is_none());

        let chain = Blockchain::Ethereum;
        block.set_chain(chain);

        assert_eq!(block.chain, Some(chain));
    }

    #[rstest]
    fn test_ethereum_block_parsing(eth_rpc_block_response: String) {
        let mut block =
            match serde_json::from_str::<RpcNodeWssResponse<Block>>(&eth_rpc_block_response) {
                Ok(rpc_response) => rpc_response.params.result,
                Err(e) => panic!("Failed to deserialize block response with error {e}"),
            };
        block.set_chain(Blockchain::Ethereum);

        assert_eq!(
            block.to_string(),
            "Block(chain=Ethereum, number=22294175, timestamp=2025-04-18T06:44:11+00:00, hash=0x71ece187051700b814592f62774e6ebd8ebdf5efbb54c90859a7d1522ce38e0a)".to_string(),
        );
        assert_eq!(
            block.hash,
            "0x71ece187051700b814592f62774e6ebd8ebdf5efbb54c90859a7d1522ce38e0a"
        );
        assert_eq!(
            block.parent_hash,
            "0x2abcce1ac985ebea2a2d6878a78387158f46de8d6db2cefca00ea36df4030a40"
        );
        assert_eq!(block.number, 22294175);
        assert_eq!(block.miner, "0x4838b106fce9647bdf1e7877bf73ce8b0bad5f97");
        // Timestamp of block is on Apr-18-2025 06:44:11 AM +UTC
        assert_eq!(
            block.timestamp,
            UnixNanos::from(Utc.with_ymd_and_hms(2025, 4, 18, 6, 44, 11).unwrap())
        );
        assert_eq!(block.gas_used, 14563593);
        assert_eq!(block.gas_limit, 35894433);

        assert_eq!(block.base_fee_per_gas, Some(U256::from(0x1862a795u64)));
        assert_eq!(block.blob_gas_used, Some(U256::from(0xc0000u64)));
        assert_eq!(block.excess_blob_gas, Some(U256::from(0x4840000u64)));
    }

    #[rstest]
    fn test_polygon_block_parsing(polygon_rpc_block_response: String) {
        let mut block =
            match serde_json::from_str::<RpcNodeWssResponse<Block>>(&polygon_rpc_block_response) {
                Ok(rpc_response) => rpc_response.params.result,
                Err(e) => panic!("Failed to deserialize block response with error {e}"),
            };
        block.set_chain(Blockchain::Polygon);

        assert_eq!(
            block.to_string(),
            "Block(chain=Polygon, number=70453741, timestamp=2025-04-18T13:17:09+00:00, hash=0x38ca655a2009e1748097f5559a0c20de7966243b804efeb53183614e4bebe199)".to_string(),
        );
        assert_eq!(
            block.hash,
            "0x38ca655a2009e1748097f5559a0c20de7966243b804efeb53183614e4bebe199"
        );
        assert_eq!(
            block.parent_hash,
            "0xf25e108267e3d6e1e4aaf4e329872273f2b1ad6186a4a22e370623aa8d021c50"
        );
        assert_eq!(block.number, 70453741);
        assert_eq!(block.miner, "0x0000000000000000000000000000000000000000");
        // Timestamp of block is on Apr-18-2025 01:17:09 PM +UTC
        assert_eq!(
            block.timestamp,
            UnixNanos::from(Utc.with_ymd_and_hms(2025, 4, 18, 13, 17, 9).unwrap())
        );
        assert_eq!(block.gas_used, 19336980);
        assert_eq!(block.gas_limit, 30000000);
        assert_eq!(block.base_fee_per_gas, Some(U256::from(0x19eu64)));
        assert!(block.blob_gas_used.is_none()); // Not applicable on Polygon
        assert!(block.excess_blob_gas.is_none()); // Not applicable on Polygon
    }

    #[rstest]
    fn test_base_block_parsing(base_rpc_block_response: String) {
        let mut block =
            match serde_json::from_str::<RpcNodeWssResponse<Block>>(&base_rpc_block_response) {
                Ok(rpc_response) => rpc_response.params.result,
                Err(e) => panic!("Failed to deserialize block response with error {e}"),
            };
        block.set_chain(Blockchain::Base);

        assert_eq!(
            block.to_string(),
            "Block(chain=Base, number=29139628, timestamp=2025-04-19T13:16:43+00:00, hash=0x14575c65070d455e6d20d5ee17be124917a33ce4437dd8615a56d29e8279b7ad)".to_string(),
        );
        assert_eq!(
            block.hash,
            "0x14575c65070d455e6d20d5ee17be124917a33ce4437dd8615a56d29e8279b7ad"
        );
        assert_eq!(
            block.parent_hash,
            "0x9a6ad4ffb258faa47ecd5eea9e7a9d8fa1772aa6232bc7cb4bbad5bc30786258"
        );
        assert_eq!(block.number, 29139628);
        assert_eq!(block.miner, "0x4200000000000000000000000000000000000011");
        // Timestamp of block is on Apr 19 2025 13:16:43 PM +UTC
        assert_eq!(
            block.timestamp,
            UnixNanos::from(Utc.with_ymd_and_hms(2025, 4, 19, 13, 16, 43).unwrap())
        );
        assert_eq!(block.gas_used, 91213350);
        assert_eq!(block.gas_limit, 120000000);

        assert_eq!(block.base_fee_per_gas, Some(U256::from(0xaae54u64)));
        assert_eq!(block.blob_gas_used, Some(U256::ZERO));
        assert_eq!(block.excess_blob_gas, Some(U256::ZERO));
    }

    #[rstest]
    fn test_arbitrum_block_parsing(arbitrum_rpc_block_response: String) {
        let mut block =
            match serde_json::from_str::<RpcNodeWssResponse<Block>>(&arbitrum_rpc_block_response) {
                Ok(rpc_response) => rpc_response.params.result,
                Err(e) => panic!("Failed to deserialize block response with error {e}"),
            };
        block.set_chain(Blockchain::Arbitrum);

        assert_eq!(
            block.to_string(),
            "Block(chain=Arbitrum, number=328014516, timestamp=2025-04-19T13:32:54+00:00, hash=0x724a0af4720fd7624976f71b16163de25f8532e87d0e7058eb0c1d3f6da3c1f8)".to_string(),
        );
        assert_eq!(
            block.hash,
            "0x724a0af4720fd7624976f71b16163de25f8532e87d0e7058eb0c1d3f6da3c1f8"
        );
        assert_eq!(
            block.parent_hash,
            "0xe7176e201c2db109be479770074ad11b979de90ac850432ed38ed335803861b6"
        );
        assert_eq!(block.number, 328014516);
        assert_eq!(block.miner, "0xa4b000000000000000000073657175656e636572");
        // Timestamp of block is on Apr-19-2025 13:32:54 PM +UTC
        assert_eq!(
            block.timestamp,
            UnixNanos::from(Utc.with_ymd_and_hms(2025, 4, 19, 13, 32, 54).unwrap())
        );
        assert_eq!(block.gas_used, 97012);
        assert_eq!(block.gas_limit, 1125899906842624);

        assert_eq!(block.base_fee_per_gas, Some(U256::from(0x989680u64)));
        assert!(block.blob_gas_used.is_none());
        assert!(block.excess_blob_gas.is_none());
    }

    #[rstest]
    fn test_block_builder_helpers() {
        let block = Block::new(
            "0xabc".into(),
            "0xdef".into(),
            1,
            Ustr::from("0x0000000000000000000000000000000000000000"),
            100_000,
            50_000,
            UnixNanos::from(1_700_000_000u64),
            Some(Blockchain::Arbitrum),
        );

        let block = block
            .with_base_fee(U256::from(1_000u64))
            .with_blob_gas(U256::from(0x10u8), U256::from(0x20u8))
            .with_l1_fee_components(U256::from(30_000u64), 1_234, 1_000_000);

        assert_eq!(block.chain, Some(chains::ARBITRUM.name));
        assert_eq!(block.base_fee_per_gas, Some(U256::from(1_000u64)));
        assert_eq!(block.blob_gas_used, Some(U256::from(0x10u8)));
        assert_eq!(block.excess_blob_gas, Some(U256::from(0x20u8)));
        assert_eq!(block.l1_gas_price, Some(U256::from(30_000u64)));
        assert_eq!(block.l1_gas_used, Some(1_234));
        assert_eq!(block.l1_fee_scalar, Some(1_000_000));
    }
}
