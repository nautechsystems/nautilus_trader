// SPDX-License-Identifier: MPL-2.0
// Derived from Envio's hypersync-client. See licenses/THIRD_PARTY_LICENSES.md.

use alloy::primitives::{Address, B256, Bytes, U64, U256};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A contract log returned by HyperSync.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HypersyncLog {
    /// Whether a chain reorganization removed the event.
    pub removed: Option<bool>,
    /// Index of the log within the block.
    #[serde(
        default,
        serialize_with = "serialize_log_quantity",
        deserialize_with = "deserialize_log_quantity"
    )]
    pub log_index: Option<u64>,
    /// Index of the transaction within the block.
    #[serde(
        default,
        serialize_with = "serialize_log_quantity",
        deserialize_with = "deserialize_log_quantity"
    )]
    pub transaction_index: Option<u64>,
    /// Hash of the transaction that emitted the log.
    pub transaction_hash: Option<B256>,
    /// Hash of the block that contains the log, when selected.
    pub block_hash: Option<B256>,
    /// Number of the block that contains the log.
    #[serde(
        default,
        serialize_with = "serialize_log_quantity",
        deserialize_with = "deserialize_log_quantity"
    )]
    pub block_number: Option<u64>,
    /// Contract address that emitted the log.
    pub address: Option<Address>,
    /// Non-indexed event data.
    pub data: Option<Bytes>,
    /// Indexed event fields in topic order.
    #[serde(default)]
    pub topics: Vec<Option<B256>>,
}

/// Selected block fields returned by HyperSync.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct HypersyncBlock {
    /// Block number.
    #[serde(default, deserialize_with = "optional_quantity")]
    pub number: Option<u64>,
    /// Block hash.
    pub hash: Option<B256>,
    /// Parent block hash.
    pub parent_hash: Option<B256>,
    /// Block beneficiary.
    pub miner: Option<Address>,
    /// Block gas limit.
    #[serde(default, deserialize_with = "optional_quantity")]
    pub gas_limit: Option<u64>,
    /// Gas used by the block.
    #[serde(default, deserialize_with = "optional_quantity")]
    pub gas_used: Option<u64>,
    /// Block timestamp in Unix seconds.
    #[serde(default, deserialize_with = "optional_quantity")]
    pub timestamp: Option<u64>,
    /// Base fee per gas when present on the chain.
    pub base_fee_per_gas: Option<U256>,
    /// Blob gas used when present on the chain.
    pub blob_gas_used: Option<U256>,
    /// Excess blob gas when present on the chain.
    pub excess_blob_gas: Option<U256>,
}

fn optional_quantity<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    Option::<U64>::deserialize(deserializer).map(|value| value.map(|value| value.to::<u64>()))
}

fn serialize_log_quantity<S: Serializer>(
    value: &Option<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value
        .map(|value| format!("{value:#x}"))
        .serialize(serializer)
}

fn deserialize_log_quantity<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    if deserializer.is_human_readable() {
        return optional_quantity(deserializer);
    }

    Option::<String>::deserialize(deserializer)?
        .map(|value| {
            value
                .parse::<U64>()
                .map(|value| value.to::<u64>())
                .map_err(serde::de::Error::custom)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use nautilus_core::serialization::{FromMsgPack, Serializable, ToMsgPack};

    use super::*;

    impl Serializable for HypersyncLog {}

    #[rstest::rstest]
    fn test_log_quantities_round_trip() {
        let log = HypersyncLog {
            log_index: Some(7),
            transaction_index: Some(16),
            block_number: Some(u64::MAX),
            ..Default::default()
        };

        let json = serde_json::to_value(&log).unwrap();
        let restored_json = serde_json::from_value::<HypersyncLog>(json.clone()).unwrap();
        let msgpack = log.to_msgpack_bytes().unwrap();
        let restored_msgpack = HypersyncLog::from_msgpack_bytes(&msgpack).unwrap();

        assert_eq!(json["log_index"], "0x7");
        assert_eq!(json["transaction_index"], "0x10");
        assert_eq!(json["block_number"], "0xffffffffffffffff");
        assert_eq!(restored_json, log);
        assert_eq!(restored_msgpack, log);
    }
}
