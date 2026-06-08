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

//! Trade module ABI encoder.
//!
//! Mirrors `derive_action_signing/module_data/trade.py::TradeModuleData`. The
//! ABI tuple is `(address, uint256, int256, int256, uint256, uint256, bool)`
//! corresponding to `(asset_address, sub_id, limit_price, amount, max_fee,
//! recipient_id, is_bid)`. Decimals are scaled to 1e18 fixed-point integers
//! before encoding (see [`crate::common::consts::DECIMAL_SCALE`]).
//!
//! Note that `limit_price` and `amount` are signed at the ABI level even
//! though prices are conventionally non-negative; this matches the venue
//! contract definition. `max_fee` is unsigned and rejects negative input.

use alloy::sol_types::SolValue;
use alloy_primitives::{Address, U256};
use rust_decimal::Decimal;

use crate::signing::{
    encoding::{decimal_to_scaled_i256, decimal_to_scaled_u256},
    modules::{ModuleData, ModuleEncodeError},
};

/// Trade-action module payload signed into every `private/order` request.
#[derive(Debug, Clone)]
pub struct TradeModuleData {
    /// ERC-20 asset address from the instrument ticker (`base_asset_address`).
    pub asset_address: Address,
    /// Sub-id from the instrument ticker (`base_asset_sub_id`).
    pub sub_id: U256,
    /// Limit price; scaled to 1e18 on encode.
    pub limit_price: Decimal,
    /// Order amount (base units); scaled to 1e18 on encode.
    pub amount: Decimal,
    /// Max-fee cap per contract in USDC; scaled to 1e18 on encode.
    pub max_fee: Decimal,
    /// Subaccount that receives the position (typically the signing subaccount).
    pub recipient_id: u64,
    /// `true` for bids, `false` for asks.
    pub is_bid: bool,
}

/// Errors raised while building the trade module payload.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TradeEncodeError {
    /// `limit_price`, `amount`, or `max_fee` could not be scaled into the
    /// venue's 1e18 fixed-point integer domain.
    #[error("trade module decimal scaling failed for {field}: {reason}")]
    DecimalOverflow {
        /// Field name.
        field: &'static str,
        /// Underlying overflow reason.
        reason: &'static str,
    },
}

impl TradeModuleData {
    /// Encode the trade payload into the ABI tuple consumed by the venue's
    /// trade module contract.
    ///
    /// # Errors
    ///
    /// Returns [`TradeEncodeError::DecimalOverflow`] when any decimal scales
    /// outside the signed/unsigned 256-bit range.
    pub fn encode(&self) -> Result<Vec<u8>, TradeEncodeError> {
        let limit_price = decimal_to_scaled_i256(self.limit_price).map_err(|reason| {
            TradeEncodeError::DecimalOverflow {
                field: "limit_price",
                reason,
            }
        })?;
        let amount = decimal_to_scaled_i256(self.amount).map_err(|reason| {
            TradeEncodeError::DecimalOverflow {
                field: "amount",
                reason,
            }
        })?;
        let max_fee = decimal_to_scaled_u256(self.max_fee).map_err(|reason| {
            TradeEncodeError::DecimalOverflow {
                field: "max_fee",
                reason,
            }
        })?;

        let tuple = (
            self.asset_address,
            self.sub_id,
            limit_price,
            amount,
            max_fee,
            U256::from(self.recipient_id),
            self.is_bid,
        );
        Ok(tuple.abi_encode())
    }
}

impl ModuleData for TradeModuleData {
    fn to_abi_encoded(&self) -> Result<Vec<u8>, ModuleEncodeError> {
        self.encode().map_err(|e| Box::new(e) as ModuleEncodeError)
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::keccak256;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn sample() -> TradeModuleData {
        TradeModuleData {
            asset_address: "0x000000000000000000000000000000000000abcd"
                .parse()
                .unwrap(),
            sub_id: U256::from(42),
            limit_price: dec!(100),
            amount: dec!(1),
            max_fee: dec!(1000),
            recipient_id: 30769,
            is_bid: true,
        }
    }

    #[rstest]
    fn test_encode_produces_seven_static_words() {
        // The ABI tuple is all static (no dynamic types), so encoding is the
        // concatenation of seven 32-byte words: address (left-padded),
        // sub_id, limit_price, amount, max_fee, recipient_id, is_bid.
        let bytes = sample().encode().unwrap();
        assert_eq!(
            bytes.len(),
            7 * 32,
            "expected 7 static words, was {}",
            bytes.len()
        );
    }

    #[rstest]
    fn test_encode_address_is_left_padded() {
        let bytes = sample().encode().unwrap();
        // First word: 12 zero bytes followed by the 20-byte address tail
        assert_eq!(&bytes[0..12], &[0u8; 12]);
        assert_eq!(
            &bytes[12..32],
            &hex_literal(&[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0xab, 0xcd,
            ])
        );
    }

    #[rstest]
    fn test_encode_sub_id_is_big_endian_uint256() {
        let bytes = sample().encode().unwrap();
        // sub_id = 42 sits in the second word; high-order bytes zero,
        // last byte 0x2a.
        assert_eq!(&bytes[32..63], &[0u8; 31]);
        assert_eq!(bytes[63], 0x2a);
    }

    #[rstest]
    fn test_encode_large_sub_id_is_uint256() {
        let mut data = sample();
        data.sub_id = U256::from_str_radix("39614082202024973918552016768", 10).unwrap();
        let bytes = data.encode().unwrap();
        let word = &bytes[32..64];
        let value = U256::from_be_slice(word);
        assert_eq!(value, data.sub_id);
    }

    #[rstest]
    fn test_encode_limit_price_is_one_hundred_scaled_to_1e18() {
        // limit_price = 100, scaled to 100 * 10^18
        // Hex: 100 * 10^18 = 0x56BC75E2D63100000
        let bytes = sample().encode().unwrap();
        let word = &bytes[64..96];
        let value = U256::from_be_slice(word);
        assert_eq!(value, U256::from(100_u128) * U256::from(10_u128.pow(18)));
    }

    #[rstest]
    fn test_encode_max_fee_is_thousand_scaled_to_1e18() {
        let bytes = sample().encode().unwrap();
        let word = &bytes[128..160];
        let value = U256::from_be_slice(word);
        assert_eq!(value, U256::from(1000_u128) * U256::from(10_u128.pow(18)));
    }

    #[rstest]
    fn test_encode_is_bid_true_packs_to_one() {
        let bytes = sample().encode().unwrap();
        let word = &bytes[192..224];
        // bool encodes to 31 zero bytes + 0x01 for true
        assert_eq!(&word[..31], &[0u8; 31]);
        assert_eq!(word[31], 0x01);
    }

    #[rstest]
    fn test_encode_is_bid_false_packs_to_zero() {
        let mut data = sample();
        data.is_bid = false;
        let bytes = data.encode().unwrap();
        let word = &bytes[192..224];
        assert_eq!(word, &[0u8; 32]);
    }

    #[rstest]
    fn test_encode_negative_amount_is_two_complement() {
        let mut data = sample();
        data.amount = dec!(-1);
        let bytes = data.encode().unwrap();
        // amount sits in word[3] (offset 96..128). int256(-1 * 1e18) =
        // two's-complement of (1e18); the high bytes will all be 0xff with
        // the low 64 bits encoding -1e18.
        let word = &bytes[96..128];
        assert_eq!(word[0], 0xff, "negative int256 must sign-extend high byte");
    }

    #[rstest]
    fn test_encode_rejects_negative_max_fee() {
        let mut data = sample();
        data.max_fee = dec!(-0.0001);
        let err = data.encode().expect_err("must reject negative max_fee");
        assert_eq!(
            err,
            TradeEncodeError::DecimalOverflow {
                field: "max_fee",
                reason: "unsigned scaled decimal must be non-negative",
            }
        );
    }

    #[rstest]
    fn test_module_data_trait_returns_same_bytes() {
        let data = sample();
        let direct = data.encode().unwrap();
        let via_trait = (&data as &dyn ModuleData).to_abi_encoded().unwrap();
        assert_eq!(direct, via_trait);
    }

    #[rstest]
    fn test_module_data_trait_propagates_encode_error() {
        let mut data = sample();
        data.max_fee = dec!(-1);
        let err = (&data as &dyn ModuleData)
            .to_abi_encoded()
            .expect_err("trait method must propagate, not panic");
        assert!(err.to_string().contains("max_fee"));
    }

    #[rstest]
    fn test_keccak_of_encoded_payload_is_stable() {
        // Smoke test: hashing the encoded payload returns a 32-byte digest.
        // This is the input to the EIP-712 action hash builder, so any
        // change to the encode shape must trip this test.
        let bytes = sample().encode().unwrap();
        let hash = keccak256(&bytes);
        assert_eq!(hash.0.len(), 32);
        // Lock down the exact hash; any encoding drift surfaces here.
        let expected = "0xc9adef7e1b0648c010e846ee4a30ad72a3320279ab75b986e296dd9b9cb39c10";
        assert_eq!(
            format!("{hash:?}"),
            expected,
            "encoding fingerprint changed"
        );
    }

    fn hex_literal(bytes: &[u8]) -> [u8; 20] {
        let mut out = [0u8; 20];
        out.copy_from_slice(bytes);
        out
    }
}
