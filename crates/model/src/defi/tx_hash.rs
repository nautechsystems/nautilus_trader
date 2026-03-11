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

use alloy_primitives::{B256, keccak256};

/// Strictly decodes signed raw transaction hex into bytes.
///
/// # Errors
///
/// Returns an error if the input is empty, odd-length, or contains non-hex characters.
pub fn decode_raw_tx_hex(raw_tx_hex: &str) -> anyhow::Result<Vec<u8>> {
    let normalized = raw_tx_hex
        .strip_prefix("0x")
        .or_else(|| raw_tx_hex.strip_prefix("0X"))
        .unwrap_or(raw_tx_hex);
    if normalized.is_empty() {
        anyhow::bail!("raw transaction hex cannot be empty");
    }
    if normalized.len() % 2 != 0 {
        anyhow::bail!("raw transaction hex must contain an even number of characters");
    }

    hex::decode(normalized).map_err(|e| anyhow::anyhow!("invalid raw transaction hex: {e}"))
}

/// Computes the canonical transaction hash from signer-returned raw transaction bytes.
///
/// This hashes the exact decoded raw bytes as returned by the signer. For typed transactions
/// (EIP-2718), this includes the type prefix byte (`0x02`, etc.).
///
/// # Errors
///
/// Returns an error when hex decoding fails.
pub fn tx_hash_from_raw_tx_hex(raw_tx_hex: &str) -> anyhow::Result<B256> {
    let raw_tx_bytes = decode_raw_tx_hex(raw_tx_hex)?;
    Ok(keccak256(raw_tx_bytes))
}

/// Computes the canonical transaction hash and returns it as a lowercase `0x`-prefixed hex string.
///
/// # Errors
///
/// Returns an error when hex decoding fails.
pub fn tx_hash_hex_from_raw_tx_hex(raw_tx_hex: &str) -> anyhow::Result<String> {
    Ok(format!(
        "0x{}",
        hex::encode(tx_hash_from_raw_tx_hex(raw_tx_hex)?)
    ))
}

#[cfg(test)]
mod tests {
    use alloy_primitives::keccak256;
    use rstest::rstest;

    use super::*;

    // Real BSC type-2 transaction fixture:
    // block: 0x50e62a1
    // tx: 0xd729e4ae5bd5523a55013e5bc7f0a88168e9ee7c1bc6cd20a9fbd896ac21138f
    const TYPE2_RAW_TX_HEX: &str = "0x02f87138508402faf0808402faf08082520894e200c20a7175b33e6f82ccd3d27ce075eedb832c8729278ceec6130080c001a0f1b79e020540f412279e7595264d332a5a8ec4399af62a9f5b4c44ddf3b48050a061b8a6cadefcb956f998e808f3e37898a9ab9346a7603ed63e92351c39326f49";
    const TYPE2_EXPECTED_TX_HASH: &str =
        "0xd729e4ae5bd5523a55013e5bc7f0a88168e9ee7c1bc6cd20a9fbd896ac21138f";
    const TYPE2_EXPECTED_HASH_WITHOUT_TYPE_BYTE: &str =
        "0xcaef2faee385c16640761f2742c1f87316cc2ad6794023de5233ba5dcc61498b";

    // Real BSC legacy transaction fixture:
    // block: 0x50e62fa
    // tx: 0x7e6d6957fb80c869e78329d8c27028b5393ca9c816b69cc795cf99b5a17af310
    const LEGACY_RAW_TX_HEX: &str = "0xf86e830248b88402faf080829c40941266c6be60392a8ff346e8d5eccd3e69dd9c5f20870136729d1a75fb808194a02493f55c3aa68191b2c2da07171c3217a3e255b47a9a316e009066bcd36a3f21a05ca55be28680a49de251aa548cb35bc712e416fd0a798abeb66bc743c359d4b9";
    const LEGACY_EXPECTED_TX_HASH: &str =
        "0x7e6d6957fb80c869e78329d8c27028b5393ca9c816b69cc795cf99b5a17af310";

    #[rstest]
    fn test_tx_hash_keccak_raw_bytes_matches_known_type2_vector() {
        let tx_hash = tx_hash_hex_from_raw_tx_hex(TYPE2_RAW_TX_HEX).unwrap();
        assert_eq!(tx_hash, TYPE2_EXPECTED_TX_HASH);
    }

    #[rstest]
    fn test_tx_hash_keccak_raw_bytes_matches_known_legacy_vector() {
        let tx_hash = tx_hash_hex_from_raw_tx_hex(LEGACY_RAW_TX_HEX).unwrap();
        assert_eq!(tx_hash, LEGACY_EXPECTED_TX_HASH);
    }

    #[rstest]
    fn test_tx_hash_includes_type_prefix_byte_for_typed_transactions() {
        let raw_with_type = decode_raw_tx_hex(TYPE2_RAW_TX_HEX).unwrap();
        let raw_without_type = raw_with_type[1..].to_vec();

        let with_type_hash = format!("0x{}", hex::encode(keccak256(raw_with_type)));
        let without_type_hash = format!("0x{}", hex::encode(keccak256(raw_without_type)));

        assert_eq!(with_type_hash, TYPE2_EXPECTED_TX_HASH);
        assert_eq!(without_type_hash, TYPE2_EXPECTED_HASH_WITHOUT_TYPE_BYTE);
        assert_ne!(with_type_hash, without_type_hash);
    }

    #[rstest]
    fn test_decode_raw_tx_hex_rejects_odd_length() {
        let err = decode_raw_tx_hex("0xabc").unwrap_err().to_string();
        assert!(err.contains("even number of characters"));
    }

    #[rstest]
    fn test_decode_raw_tx_hex_rejects_non_hex() {
        let err = decode_raw_tx_hex("0xzz").unwrap_err().to_string();
        assert!(err.contains("invalid raw transaction hex"));
    }

    #[rstest]
    fn test_decode_raw_tx_hex_accepts_uppercase_prefix() {
        let with_lower_prefix = decode_raw_tx_hex(TYPE2_RAW_TX_HEX).unwrap();
        let with_upper_prefix =
            decode_raw_tx_hex(&TYPE2_RAW_TX_HEX.replacen("0x", "0X", 1)).unwrap();

        assert_eq!(with_upper_prefix, with_lower_prefix);
    }

    #[rstest]
    #[case("")]
    #[case("0x")]
    #[case("0X")]
    fn test_decode_raw_tx_hex_rejects_empty_payloads(#[case] raw_tx_hex: &str) {
        let err = decode_raw_tx_hex(raw_tx_hex).unwrap_err().to_string();
        assert!(err.contains("cannot be empty"));
    }
}
