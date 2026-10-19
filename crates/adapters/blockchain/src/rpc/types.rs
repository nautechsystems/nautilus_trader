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

use alloy::primitives::{Address, B256, Bytes, U256};
use nautilus_model::defi::{Block, DexType, rpc::RpcLog};
use serde::Deserialize;
use serde_json::Value;

use crate::events::{
    burn::BurnEvent, collect::CollectEvent, fee_protocol_collect::FeeProtocolCollectEvent,
    fee_protocol_update::FeeProtocolUpdateEvent, flash::FlashEvent, mint::MintEvent,
    swap::SwapEvent,
};

/// Represents normalized blockchain messages.
#[derive(Debug, Clone)]
pub enum BlockchainMessage {
    Block(Block),
    SwapEvent(SwapEvent),
    MintEvent(MintEvent),
    BurnEvent(BurnEvent),
    CollectEvent(CollectEvent),
    FlashEvent(FlashEvent),
    FeeProtocolUpdateEvent(FeeProtocolUpdateEvent),
    FeeProtocolCollectEvent(FeeProtocolCollectEvent),
}

/// Represents the types of events that can be subscribed to via the blockchain RPC interface.
///
/// This enum defines the various event types that the application can subscribe to using
/// the WebSocket-based RPC subscription.
#[derive(Debug, Clone, Copy, Hash, PartialOrd, Ord, PartialEq, Eq)]
pub enum RpcEventType {
    NewBlock,
    PoolSwap(DexType),
    PoolMint(DexType),
    PoolBurn(DexType),
    PoolCollect(DexType),
    PoolFlash(DexType),
    PoolFeeProtocolUpdate(DexType),
    PoolFeeProtocolCollect(DexType),
}

/// Result of an explicit-height `eth_call` before contract-specific output decoding.
#[cfg(feature = "hypersync")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RpcCallResult {
    Success(Bytes),
    Reverted,
}

/// Represents the minimal block view required for execution fee derivation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcBlock {
    /// The block number.
    #[serde(deserialize_with = "deserialize_hex_u64")]
    pub number: u64,
    /// The canonical hash of the block.
    pub hash: B256,
    /// The canonical hash of the parent block.
    pub parent_hash: B256,
    /// The block timestamp in seconds since the Unix epoch.
    #[serde(deserialize_with = "deserialize_hex_u64")]
    pub timestamp: u64,
    /// The block base fee per gas in wei (`None` on pre-London chains).
    #[serde(default, deserialize_with = "deserialize_hex_u128_opt")]
    pub base_fee_per_gas: Option<u128>,
    /// Full transactions when requested, otherwise empty.
    #[serde(skip)]
    pub transactions: Vec<RpcTransaction>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RpcBlockResponse {
    #[serde(flatten)]
    pub block: RpcBlock,
    #[serde(default)]
    pub transactions: Vec<Value>,
}

/// Represents the transaction identity required for signer-nonce reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransaction {
    /// The transaction hash.
    pub hash: B256,
    /// The signer address.
    pub from: Address,
    /// The signer nonce.
    #[serde(deserialize_with = "deserialize_hex_u64")]
    pub nonce: u64,
    /// The chain ID authenticated by the transaction signature.
    #[serde(default, deserialize_with = "deserialize_hex_u64_opt")]
    pub chain_id: Option<u64>,
    /// The EIP-2718 transaction type.
    #[serde(default, rename = "type", deserialize_with = "deserialize_hex_u8_opt")]
    pub transaction_type: Option<u8>,
    /// The destination address, or `None` for contract creation.
    pub to: Option<Address>,
    /// The transaction calldata.
    pub input: Bytes,
    /// The native value in wei.
    pub value: U256,
    /// The transaction gas limit.
    #[serde(default, deserialize_with = "deserialize_hex_u64_opt")]
    pub gas: Option<u64>,
    /// The EIP-1559 maximum fee per gas in wei.
    pub max_fee_per_gas: Option<U256>,
    /// The EIP-1559 maximum priority fee per gas in wei.
    pub max_priority_fee_per_gas: Option<U256>,
}

/// Represents the minimal transaction receipt view required for inclusion observation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransactionReceipt {
    /// The transaction hash.
    pub transaction_hash: B256,
    /// The canonical block hash reported with the receipt.
    pub block_hash: B256,
    /// The block number that included the transaction.
    #[serde(deserialize_with = "deserialize_hex_u64")]
    pub block_number: u64,
    /// The gas used by the transaction.
    #[serde(deserialize_with = "deserialize_hex_u64")]
    pub gas_used: u64,
    /// The effective gas price charged in wei.
    pub effective_gas_price: U256,
    /// The transaction index within the block.
    #[serde(deserialize_with = "deserialize_hex_u64")]
    pub transaction_index: u64,
    /// Whether the transaction executed successfully (status `0x1`).
    #[serde(deserialize_with = "deserialize_hex_bool")]
    pub status: bool,
    /// Logs emitted by the transaction.
    #[serde(default)]
    pub logs: Vec<RpcLog>,
}

/// The call operation reported by Geth's `callTracer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RpcCallType {
    Call,
    Callcode,
    Delegatecall,
    Staticcall,
    Create,
    Create2,
    Selfdestruct,
}

/// A normalized frame from Geth's `callTracer`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcCallTrace {
    /// The EVM call operation.
    #[serde(rename = "type")]
    pub call_type: RpcCallType,
    /// The frame caller.
    pub from: Address,
    /// The frame target, when the operation has one.
    pub to: Option<Address>,
    /// The native value supplied to the frame.
    #[serde(default)]
    pub value: U256,
    /// The frame gas allowance.
    #[serde(deserialize_with = "deserialize_hex_u64")]
    pub gas: u64,
    /// The gas consumed by the frame.
    #[serde(deserialize_with = "deserialize_hex_u64")]
    pub gas_used: u64,
    /// The input bytes supplied to the frame.
    #[serde(default)]
    pub input: Bytes,
    /// The output bytes returned by the frame.
    #[serde(default)]
    pub output: Bytes,
    /// Whether the frame reported an execution error.
    #[serde(default)]
    pub error: Option<String>,
    /// Child frames in execution order.
    #[serde(default)]
    pub calls: Vec<Self>,
}

fn deserialize_hex_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let value = parse_hex_quantity(&s).map_err(serde::de::Error::custom)?;
    u64::try_from(value).map_err(serde::de::Error::custom)
}

fn deserialize_hex_u64_opt<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?
        .map(|value| parse_hex_quantity(&value))
        .transpose()
        .map_err(serde::de::Error::custom)?;
    value
        .map(u64::try_from)
        .transpose()
        .map_err(serde::de::Error::custom)
}

fn deserialize_hex_u8_opt<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?
        .map(|value| parse_hex_quantity(&value))
        .transpose()
        .map_err(serde::de::Error::custom)?;
    value
        .map(u8::try_from)
        .transpose()
        .map_err(serde::de::Error::custom)
}

fn deserialize_hex_u128_opt<'de, D>(deserializer: D) -> Result<Option<u128>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    s.map(|s| parse_hex_quantity(&s).map_err(serde::de::Error::custom))
        .transpose()
}

fn deserialize_hex_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "0x0" => Ok(false),
        "0x1" => Ok(true),
        _ => Err(serde::de::Error::custom(
            "invalid transaction receipt status; expected 0x0 or 0x1",
        )),
    }
}

fn parse_hex_quantity(s: &str) -> anyhow::Result<u128> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    u128::from_str_radix(stripped, 16)
        .map_err(|e| anyhow::anyhow!("Failed to parse hex quantity '{s}': {e}"))
}
