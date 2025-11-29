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

//! Helper functions for parsing RPC log entries.
//!
//! These functions work with `RpcLog` from the standard Ethereum JSON-RPC format,
//! converting hex strings to the appropriate types.

use alloy::primitives::Address;
use nautilus_model::defi::rpc::RpcLog;

use crate::exchanges::parsing::core;

/// Decode hex string (with or without 0x prefix) to bytes.
///
/// # Errors
///
/// Returns an error if the hex string is invalid.
pub fn decode_hex(hex: &str) -> anyhow::Result<Vec<u8>> {
    hex::decode(hex.trim_start_matches("0x")).map_err(|e| anyhow::anyhow!("Invalid hex: {e}"))
}

/// Parse hex string to u64.
///
/// # Errors
///
/// Returns an error if the hex string cannot be parsed as u64.
pub fn parse_hex_u64(hex: &str) -> anyhow::Result<u64> {
    u64::from_str_radix(hex.trim_start_matches("0x"), 16)
        .map_err(|e| anyhow::anyhow!("Invalid hex u64: {e}"))
}

/// Parse hex string to u32.
///
/// # Errors
///
/// Returns an error if the hex string cannot be parsed as u32.
pub fn parse_hex_u32(hex: &str) -> anyhow::Result<u32> {
    u32::from_str_radix(hex.trim_start_matches("0x"), 16)
        .map_err(|e| anyhow::anyhow!("Invalid hex u32: {e}"))
}

/// Extract block number from RPC log.
///
/// # Errors
///
/// Returns an error if the block number is missing or cannot be parsed.
pub fn extract_block_number(log: &RpcLog) -> anyhow::Result<u64> {
    let hex = log
        .block_number
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing block number"))?;
    parse_hex_u64(hex)
}

/// Extract transaction hash from RPC log.
///
/// # Errors
///
/// Returns an error if the transaction hash is missing.
pub fn extract_transaction_hash(log: &RpcLog) -> anyhow::Result<String> {
    log.transaction_hash
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Missing transaction hash"))
}

/// Extract transaction index from RPC log.
///
/// # Errors
///
/// Returns an error if the transaction index is missing or cannot be parsed.
pub fn extract_transaction_index(log: &RpcLog) -> anyhow::Result<u32> {
    let hex = log
        .transaction_index
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing transaction index"))?;
    parse_hex_u32(hex)
}

/// Extract log index from RPC log.
///
/// # Errors
///
/// Returns an error if the log index is missing or cannot be parsed.
pub fn extract_log_index(log: &RpcLog) -> anyhow::Result<u32> {
    let hex = log
        .log_index
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing log index"))?;
    parse_hex_u32(hex)
}

/// Extract contract address from RPC log.
///
/// # Errors
///
/// Returns an error if the address is invalid.
pub fn extract_address(log: &RpcLog) -> anyhow::Result<Address> {
    let bytes = decode_hex(&log.address)?;
    Ok(Address::from_slice(&bytes))
}

/// Extract topic bytes at index.
///
/// # Errors
///
/// Returns an error if the topic at the specified index is missing.
pub fn extract_topic_bytes(log: &RpcLog, index: usize) -> anyhow::Result<Vec<u8>> {
    let hex = log
        .topics
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("Missing topic at index {index}"))?;
    decode_hex(hex)
}

/// Extract address from topic at index (using shared core logic).
///
/// # Errors
///
/// Returns an error if the topic is missing or the address extraction fails.
pub fn extract_address_from_topic(
    log: &RpcLog,
    index: usize,
    description: &str,
) -> anyhow::Result<Address> {
    let bytes = extract_topic_bytes(log, index)
        .map_err(|_| anyhow::anyhow!("Missing {description} address in topic{index}"))?;
    core::extract_address_from_bytes(&bytes)
}

/// Extract data bytes from RPC log.
///
/// # Errors
///
/// Returns an error if the hex decoding fails.
pub fn extract_data_bytes(log: &RpcLog) -> anyhow::Result<Vec<u8>> {
    decode_hex(&log.data)
}

/// Validate event signature from topic0.
///
/// # Errors
///
/// Returns an error if the signature doesn't match or topic0 is missing.
pub fn validate_event_signature(
    log: &RpcLog,
    expected_hash: &str,
    event_name: &str,
) -> anyhow::Result<()> {
    let sig_bytes = extract_topic_bytes(log, 0)?;
    core::validate_signature_bytes(&sig_bytes, expected_hash, event_name)
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;

    /// Real RPC log from Arbitrum PoolCreated event at block 185
    /// Pool: 0xB9Fc136980D98C034a529AadbD5651c087365D5f
    /// token0: 0x2E5353426C89F4eCD52D1036DA822D47E73376C4
    /// token1: 0x838930cFE7502dd36B0b1ebbef8001fbF94f3bFb
    /// fee: 3000, tickSpacing: 60
    #[fixture]
    fn log() -> RpcLog {
        RpcLog {
            removed: false,
            log_index: Some("0x0".to_string()),
            transaction_index: Some("0x0".to_string()),
            transaction_hash: Some(
                "0x24058dde7caf5b8b70041de8b27731f20f927365f210247c3e720e947b9098e7".to_string(),
            ),
            block_hash: Some(
                "0xd371b6c7b04ec33d6470f067a82e87d7b294b952bea7a46d7b939b4c7addc275".to_string(),
            ),
            block_number: Some("0xb9".to_string()),
            address: "0x1f98431c8ad98523631ae4a59f267346ea31f984".to_string(),
            data: "0x000000000000000000000000000000000000000000000000000000000000003c000000000000000000000000b9fc136980d98c034a529aadbd5651c087365d5f".to_string(),
            topics: vec![
                "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118".to_string(),
                "0x0000000000000000000000002e5353426c89f4ecd52d1036da822d47e73376c4".to_string(),
                "0x000000000000000000000000838930cfe7502dd36b0b1ebbef8001fbf94f3bfb".to_string(),
                "0x0000000000000000000000000000000000000000000000000000000000000bb8".to_string(),
            ],
        }
    }

    #[rstest]
    fn test_decode_hex_with_prefix() {
        let result = decode_hex("0x1234").unwrap();
        assert_eq!(result, vec![0x12, 0x34]);
    }

    #[rstest]
    fn test_decode_hex_without_prefix() {
        let result = decode_hex("1234").unwrap();
        assert_eq!(result, vec![0x12, 0x34]);
    }

    #[rstest]
    fn test_parse_hex_u64_block_185() {
        // Block 185 = 0xb9
        assert_eq!(parse_hex_u64("0xb9").unwrap(), 185);
        assert_eq!(parse_hex_u64("b9").unwrap(), 185);
    }

    #[rstest]
    fn test_parse_hex_u32() {
        assert_eq!(parse_hex_u32("0x0").unwrap(), 0);
        assert_eq!(parse_hex_u32("0xbb8").unwrap(), 3000); // fee from block 185
    }

    #[rstest]
    fn test_extract_block_number(log: RpcLog) {
        assert_eq!(extract_block_number(&log).unwrap(), 185);
    }

    #[rstest]
    fn test_extract_transaction_hash(log: RpcLog) {
        assert_eq!(
            extract_transaction_hash(&log).unwrap(),
            "0x24058dde7caf5b8b70041de8b27731f20f927365f210247c3e720e947b9098e7"
        );
    }

    #[rstest]
    fn test_extract_transaction_index(log: RpcLog) {
        assert_eq!(extract_transaction_index(&log).unwrap(), 0);
    }

    #[rstest]
    fn test_extract_log_index(log: RpcLog) {
        assert_eq!(extract_log_index(&log).unwrap(), 0);
    }

    #[rstest]
    fn test_extract_address(log: RpcLog) {
        let address = extract_address(&log).unwrap();
        // Uniswap V3 Factory address on Arbitrum
        assert_eq!(
            address.to_string().to_lowercase(),
            "0x1f98431c8ad98523631ae4a59f267346ea31f984"
        );
    }

    #[rstest]
    fn test_extract_address_from_topic_token0(log: RpcLog) {
        let address = extract_address_from_topic(&log, 1, "token0").unwrap();
        assert_eq!(
            address.to_string().to_lowercase(),
            "0x2e5353426c89f4ecd52d1036da822d47e73376c4"
        );
    }

    #[rstest]
    fn test_extract_address_from_topic_token1(log: RpcLog) {
        let address = extract_address_from_topic(&log, 2, "token1").unwrap();
        assert_eq!(
            address.to_string().to_lowercase(),
            "0x838930cfe7502dd36b0b1ebbef8001fbf94f3bfb"
        );
    }

    #[rstest]
    fn test_extract_data_bytes(log: RpcLog) {
        let data = extract_data_bytes(&log).unwrap();
        // Data contains tickSpacing (60 = 0x3c) and pool address
        assert_eq!(data.len(), 64); // 2 x 32 bytes
        // First 32 bytes: tickSpacing = 60 (0x3c)
        assert_eq!(data[31], 0x3c);
    }

    #[rstest]
    fn test_validate_event_signature_pool_created(log: RpcLog) {
        let expected = "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118";
        assert!(validate_event_signature(&log, expected, "PoolCreated").is_ok());
    }

    #[rstest]
    fn test_validate_event_signature_mismatch(log: RpcLog) {
        // Swap event signature instead of PoolCreated
        let wrong = "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";
        let result = validate_event_signature(&log, wrong, "Swap");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid event signature")
        );
    }
}
