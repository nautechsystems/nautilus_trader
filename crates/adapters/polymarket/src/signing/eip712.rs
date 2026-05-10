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

//! EIP-712 order signing for the Polymarket CTF Exchange.
//!
//! Orders on Polymarket are signed typed structured data (EIP-712) against the
//! CTF Exchange contract on Polygon. Two exchange contracts exist:
//! - [`CTF_EXCHANGE`]: Standard binary markets.
//! - [`NEG_RISK_CTF_EXCHANGE`]: Negative-risk (multi-outcome) markets.
//!
//! Both share the same EIP-712 domain name and version; only the
//! `verifyingContract` differs.

use std::str::FromStr;

use alloy_primitives::{Address, B256, U256, address};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{SolStruct, eip712_domain};
use rust_decimal::Decimal;

use crate::{
    common::{credential::EvmPrivateKey, enums::PolymarketOrderSide},
    http::{
        error::{Error, Result},
        models::PolymarketOrder,
    },
};

/// CTF Exchange contract address on Polygon mainnet.
pub const CTF_EXCHANGE: Address = address!("0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E");

/// Neg Risk CTF Exchange contract address on Polygon mainnet.
pub const NEG_RISK_CTF_EXCHANGE: Address = address!("0xC5d563A36AE78145C45a50134d48A1215220f80a");

const DOMAIN_NAME: &str = "Polymarket CTF Exchange";
const DOMAIN_VERSION: &str = "1";
const POLYGON_CHAIN_ID: u64 = 137;

// EIP-712 Order struct matching the CTFExchange contract.
//
// Reference: <https://github.com/Polymarket/ctf-exchange/blob/main/src/exchange/libraries/OrderStructs.sol>
alloy_sol_types::sol! {
    struct Order {
        uint256 salt;
        address maker;
        address signer;
        address taker;
        uint256 tokenId;
        uint256 makerAmount;
        uint256 takerAmount;
        uint256 expiration;
        uint256 nonce;
        uint256 feeRateBps;
        uint8 side;
        uint8 signatureType;
    }
}

/// EIP-712 order signer for the Polymarket CTF Exchange.
#[derive(Debug)]
pub struct OrderSigner {
    signer: PrivateKeySigner,
}

impl OrderSigner {
    /// Creates a new [`OrderSigner`] from an EVM private key.
    pub fn new(private_key: &EvmPrivateKey) -> Result<Self> {
        let key_hex = private_key
            .as_hex()
            .strip_prefix("0x")
            .unwrap_or(private_key.as_hex());
        let signer = PrivateKeySigner::from_str(key_hex)
            .map_err(|e| Error::bad_request(format!("Failed to create signer: {e}")))?;
        Ok(Self { signer })
    }

    /// Returns the signer's Ethereum address.
    #[must_use]
    pub fn address(&self) -> Address {
        self.signer.address()
    }

    /// Signs a [`PolymarketOrder`] and returns the hex-encoded ECDSA signature.
    ///
    /// The `neg_risk` flag selects which exchange contract to use as the
    /// EIP-712 `verifyingContract`.
    ///
    /// # Errors
    ///
    /// Returns an error if `order.signer` does not match this signer's address.
    pub fn sign_order(&self, order: &PolymarketOrder, neg_risk: bool) -> Result<String> {
        let order_signer = parse_address(&order.signer, "signer")?;
        if order_signer != self.signer.address() {
            return Err(Error::bad_request(format!(
                "Order signer {order_signer} does not match local signer {}",
                self.signer.address(),
            )));
        }

        let eip712_order = build_eip712_order(order)?;

        let contract = if neg_risk {
            NEG_RISK_CTF_EXCHANGE
        } else {
            CTF_EXCHANGE
        };

        let domain = eip712_domain! {
            name: DOMAIN_NAME,
            version: DOMAIN_VERSION,
            chain_id: POLYGON_CHAIN_ID,
            verifying_contract: contract,
        };

        let signing_hash = eip712_order.eip712_signing_hash(&domain);
        self.sign_hash(&signing_hash.0)
    }

    fn sign_hash(&self, hash: &[u8; 32]) -> Result<String> {
        let hash_b256 = B256::from(*hash);
        let signature = self
            .signer
            .sign_hash_sync(&hash_b256)
            .map_err(|e| Error::bad_request(format!("Failed to sign order: {e}")))?;

        let r = signature.r();
        let s = signature.s();
        let v = if signature.v() { 28u8 } else { 27u8 };

        Ok(format!("0x{r:064x}{s:064x}{v:02x}"))
    }
}

// Converts a PolymarketOrder to the EIP-712 Order struct
fn build_eip712_order(order: &PolymarketOrder) -> Result<Order> {
    Ok(Order {
        salt: U256::from(order.salt),
        maker: parse_address(&order.maker, "maker")?,
        signer: parse_address(&order.signer, "signer")?,
        taker: parse_address(&order.taker, "taker")?,
        tokenId: U256::from_str(order.token_id.as_str())
            .map_err(|e| Error::bad_request(format!("Invalid token ID: {e}")))?,
        makerAmount: decimal_to_u256(order.maker_amount, "maker_amount")?,
        takerAmount: decimal_to_u256(order.taker_amount, "taker_amount")?,
        expiration: U256::from_str(&order.expiration)
            .map_err(|e| Error::bad_request(format!("Invalid expiration: {e}")))?,
        nonce: U256::from_str(&order.nonce)
            .map_err(|e| Error::bad_request(format!("Invalid nonce: {e}")))?,
        feeRateBps: decimal_to_u256(order.fee_rate_bps, "fee_rate_bps")?,
        side: order_side_to_u8(order.side),
        signatureType: order.signature_type as u8,
    })
}

fn parse_address(addr: &str, field: &str) -> Result<Address> {
    Address::from_str(addr).map_err(|e| Error::bad_request(format!("Invalid {field} address: {e}")))
}

fn decimal_to_u256(d: Decimal, field: &str) -> Result<U256> {
    let normalized = d.normalize();
    if normalized.scale() != 0 {
        return Err(Error::bad_request(format!("{field} must be an integer")));
    }
    let mantissa = normalized.mantissa();
    if mantissa < 0 {
        return Err(Error::bad_request(format!("{field} must be non-negative")));
    }
    Ok(U256::from(mantissa as u128))
}

fn order_side_to_u8(side: PolymarketOrderSide) -> u8 {
    match side {
        PolymarketOrderSide::Buy => 0,
        PolymarketOrderSide::Sell => 1,
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::keccak256;
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::common::enums::SignatureType;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    fn test_signer() -> OrderSigner {
        let pk = EvmPrivateKey::new(TEST_PRIVATE_KEY.to_string()).unwrap();
        OrderSigner::new(&pk).unwrap()
    }

    fn test_order() -> PolymarketOrder {
        PolymarketOrder {
            salt: 123456789,
            maker: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_string(),
            signer: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_string(),
            taker: "0x0000000000000000000000000000000000000000".to_string(),
            token_id: Ustr::from(
                "71321045679252212594626385532706912750332728571942532289631379312455583992563",
            ),
            maker_amount: dec!(100000000),
            taker_amount: dec!(50000000),
            expiration: "0".to_string(),
            nonce: "0".to_string(),
            fee_rate_bps: dec!(0),
            side: PolymarketOrderSide::Buy,
            signature_type: SignatureType::Eoa,
            signature: String::new(),
        }
    }

    #[rstest]
    fn test_order_typehash_matches_contract() {
        // ORDER_TYPEHASH from the CTFExchange Solidity contract
        let expected = keccak256(
            "Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)",
        );
        let order = test_order();
        let eip712_order = build_eip712_order(&order).unwrap();
        assert_eq!(eip712_order.eip712_type_hash(), expected);
    }

    #[rstest]
    fn test_signer_address_derivation() {
        let signer = test_signer();
        // Hardhat account #0
        let expected = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").unwrap();
        assert_eq!(signer.address(), expected);
    }

    #[rstest]
    fn test_sign_order_format() {
        let signer = test_signer();
        let order = test_order();

        let sig = signer.sign_order(&order, false).unwrap();

        assert!(sig.starts_with("0x"));
        assert_eq!(sig.len(), 132); // 0x + r(64) + s(64) + v(2)
    }

    #[rstest]
    fn test_sign_order_deterministic() {
        let signer = test_signer();
        let order = test_order();

        let sig1 = signer.sign_order(&order, false).unwrap();
        let sig2 = signer.sign_order(&order, false).unwrap();
        assert_eq!(sig1, sig2);
    }

    #[rstest]
    fn test_sign_order_neg_risk_differs() {
        let signer = test_signer();
        let order = test_order();

        let sig_normal = signer.sign_order(&order, false).unwrap();
        let sig_neg_risk = signer.sign_order(&order, true).unwrap();
        assert_ne!(sig_normal, sig_neg_risk);
    }

    #[rstest]
    fn test_sign_order_sell_side() {
        let signer = test_signer();
        let mut order = test_order();
        let sig_buy = signer.sign_order(&order, false).unwrap();

        order.side = PolymarketOrderSide::Sell;
        let sig_sell = signer.sign_order(&order, false).unwrap();
        assert_ne!(sig_buy, sig_sell);
    }

    #[rstest]
    fn test_sign_order_different_amounts() {
        let signer = test_signer();
        let mut order = test_order();
        let sig1 = signer.sign_order(&order, false).unwrap();

        order.maker_amount = dec!(200000000);
        let sig2 = signer.sign_order(&order, false).unwrap();
        assert_ne!(sig1, sig2);
    }

    #[rstest]
    fn test_build_eip712_order() {
        let order = test_order();
        let eip712 = build_eip712_order(&order).unwrap();

        assert_eq!(eip712.salt, U256::from(123456789u64));
        assert_eq!(
            eip712.maker,
            Address::from_str("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266").unwrap()
        );
        assert_eq!(eip712.makerAmount, U256::from(100000000u128));
        assert_eq!(eip712.takerAmount, U256::from(50000000u128));
        assert_eq!(eip712.side, 0); // BUY
        assert_eq!(eip712.signatureType, 0); // EOA
    }

    #[rstest]
    fn test_decimal_to_u256_integer() {
        let result = decimal_to_u256(dec!(100000000), "test").unwrap();
        assert_eq!(result, U256::from(100000000u128));
    }

    #[rstest]
    fn test_decimal_to_u256_zero() {
        let result = decimal_to_u256(dec!(0), "test").unwrap();
        assert_eq!(result, U256::ZERO);
    }

    #[rstest]
    fn test_decimal_to_u256_rejects_fractional() {
        let result = decimal_to_u256(dec!(100.5), "test");
        assert!(result.is_err());
    }

    #[rstest]
    fn test_decimal_to_u256_rejects_negative() {
        let result = decimal_to_u256(dec!(-1), "test");
        assert!(result.is_err());
    }

    #[rstest]
    fn test_order_side_mapping() {
        assert_eq!(order_side_to_u8(PolymarketOrderSide::Buy), 0);
        assert_eq!(order_side_to_u8(PolymarketOrderSide::Sell), 1);
    }

    #[rstest]
    fn test_contract_addresses_nonzero() {
        assert_ne!(CTF_EXCHANGE, Address::ZERO);
        assert_ne!(NEG_RISK_CTF_EXCHANGE, Address::ZERO);
        assert_ne!(CTF_EXCHANGE, NEG_RISK_CTF_EXCHANGE);
    }

    #[rstest]
    fn test_sign_order_recoverable() {
        use alloy_primitives::Signature;

        let signer = test_signer();
        let order = test_order();
        let sig_hex = signer.sign_order(&order, false).unwrap();

        let sig_bytes = hex::decode(&sig_hex[2..]).unwrap();
        assert_eq!(sig_bytes.len(), 65);

        let r = U256::from_be_slice(&sig_bytes[..32]);
        let s = U256::from_be_slice(&sig_bytes[32..64]);
        let v = sig_bytes[64];
        let y_parity = v == 28;

        let signature = Signature::new(r, s, y_parity);

        let eip712_order = build_eip712_order(&order).unwrap();
        let domain = eip712_domain! {
            name: DOMAIN_NAME,
            version: DOMAIN_VERSION,
            chain_id: POLYGON_CHAIN_ID,
            verifying_contract: CTF_EXCHANGE,
        };
        let signing_hash = eip712_order.eip712_signing_hash(&domain);

        let recovered = signature
            .recover_address_from_prehash(&signing_hash)
            .unwrap();
        assert_eq!(recovered, signer.address());
    }
}
