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

//! Shared core extraction functions for parsing event logs.
//!
//! These functions operate on raw bytes and are used by both HyperSync and RPC parsers
//! to ensure consistent extraction logic.

use alloy::primitives::Address;

/// Extract address from 32-byte topic (address in last 20 bytes).
///
/// In Ethereum event logs, indexed address parameters are stored as 32-byte
/// values with the 20-byte address right-aligned (padded with zeros on the left).
///
/// # Errors
///
/// Returns an error if the byte slice is shorter than 32 bytes.
pub fn extract_address_from_bytes(bytes: &[u8]) -> anyhow::Result<Address> {
    anyhow::ensure!(
        bytes.len() >= 32,
        "Topic must be at least 32 bytes, got {}",
        bytes.len()
    );
    Ok(Address::from_slice(&bytes[12..32]))
}

/// Extract u32 from 32-byte topic (value in last 4 bytes, big-endian).
///
/// In Ethereum event logs, indexed numeric parameters are stored as 32-byte
/// values with the number right-aligned in big-endian format.
///
/// # Errors
///
/// Returns an error if the byte slice is shorter than 32 bytes.
pub fn extract_u32_from_bytes(bytes: &[u8]) -> anyhow::Result<u32> {
    anyhow::ensure!(
        bytes.len() >= 32,
        "Topic must be at least 32 bytes, got {}",
        bytes.len()
    );
    Ok(u32::from_be_bytes(bytes[28..32].try_into()?))
}

/// Extract i32 from 32-byte topic (value in last 4 bytes, big-endian, signed).
///
/// In Ethereum event logs, indexed signed numeric parameters (like tick values)
/// are stored as 32-byte values with the number right-aligned in big-endian format.
///
/// # Errors
///
/// Returns an error if the byte slice is shorter than 32 bytes.
pub fn extract_i32_from_bytes(bytes: &[u8]) -> anyhow::Result<i32> {
    anyhow::ensure!(
        bytes.len() >= 32,
        "Topic must be at least 32 bytes, got {}",
        bytes.len()
    );
    Ok(i32::from_be_bytes(bytes[28..32].try_into()?))
}

/// Validate event signature matches expected hash.
///
/// The first topic (topic0) of an Ethereum event log contains the keccak256 hash
/// of the event signature. This function validates that the actual signature
/// matches the expected one.
///
/// # Errors
///
/// Returns an error if the signatures don't match.
pub fn validate_signature_bytes(
    actual: &[u8],
    expected_hex: &str,
    event_name: &str,
) -> anyhow::Result<()> {
    let actual_hex = hex::encode(actual);
    anyhow::ensure!(
        actual_hex == expected_hex,
        "Invalid event signature for '{event_name}': expected {expected_hex}, got {actual_hex}",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_extract_address_token0() {
        // token0 address from PoolCreated event topic1 at block 185
        let bytes = hex::decode("0000000000000000000000002e5353426c89f4ecd52d1036da822d47e73376c4")
            .unwrap();

        let address = extract_address_from_bytes(&bytes).unwrap();
        assert_eq!(
            address.to_string().to_lowercase(),
            "0x2e5353426c89f4ecd52d1036da822d47e73376c4"
        );
    }

    #[rstest]
    fn test_extract_address_token1_block() {
        // token1 address from PoolCreated event topic2 at block 185
        let bytes = hex::decode("000000000000000000000000838930cfe7502dd36b0b1ebbef8001fbf94f3bfb")
            .unwrap();

        let address = extract_address_from_bytes(&bytes).unwrap();
        assert_eq!(
            address.to_string().to_lowercase(),
            "0x838930cfe7502dd36b0b1ebbef8001fbf94f3bfb"
        );
    }

    #[rstest]
    fn test_extract_address_from_bytes_too_short() {
        let bytes = vec![0u8; 31];
        let result = extract_address_from_bytes(&bytes);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Topic must be at least 32 bytes")
        );
    }

    #[rstest]
    fn test_extract_u32_fee_3000() {
        let bytes = hex::decode("0000000000000000000000000000000000000000000000000000000000000bb8")
            .unwrap();

        let value = extract_u32_from_bytes(&bytes).unwrap();
        assert_eq!(value, 3000);
    }

    #[rstest]
    fn test_extract_u32_fee_500() {
        let bytes = hex::decode("00000000000000000000000000000000000000000000000000000000000001f4")
            .unwrap();

        let value = extract_u32_from_bytes(&bytes).unwrap();
        assert_eq!(value, 500);
    }

    #[rstest]
    fn test_extract_i32_tick_spacing_60() {
        let bytes = hex::decode("000000000000000000000000000000000000000000000000000000000000003c")
            .unwrap();

        let value = extract_i32_from_bytes(&bytes).unwrap();
        assert_eq!(value, 60);
    }

    #[rstest]
    fn test_extract_i32_tick_spacing_10() {
        let bytes = hex::decode("000000000000000000000000000000000000000000000000000000000000000a")
            .unwrap();

        let value = extract_i32_from_bytes(&bytes).unwrap();
        assert_eq!(value, 10);
    }

    #[rstest]
    fn test_extract_i32_from_bytes_negative() {
        let bytes = hex::decode("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffc4")
            .unwrap();

        let value = extract_i32_from_bytes(&bytes).unwrap();
        assert_eq!(value, -60);
    }

    #[rstest]
    fn test_validate_signature_pool_created() {
        let pool_created_signature =
            hex::decode("783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118")
                .unwrap();
        let expected = "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118";

        let result = validate_signature_bytes(&pool_created_signature, expected, "PoolCreated");
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_validate_signature_bytes_mismatch() {
        let pool_created_signature =
            hex::decode("783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118")
                .unwrap();
        let swap_expected = "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";

        let result = validate_signature_bytes(&pool_created_signature, swap_expected, "Swap");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid event signature for 'Swap'")
        );
    }
}
