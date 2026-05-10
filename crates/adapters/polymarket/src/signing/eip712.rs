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

use alloy::{
    signers::{SignerSync, local::PrivateKeySigner},
    sol_types::{SolStruct, eip712_domain},
};
use alloy_primitives::{Address, B256, FixedBytes, U256, address};
use rust_decimal::Decimal;

use crate::{
    common::{credential::EvmPrivateKey, enums::PolymarketOrderSide},
    http::{
        error::{Error, Result},
        models::PolymarketOrder,
    },
};

// L1 ClobAuth constants
const CLOB_AUTH_DOMAIN_NAME: &str = "ClobAuthDomain";
const CLOB_AUTH_DOMAIN_VERSION: &str = "1";
const CLOB_AUTH_MESSAGE: &str = "This message attests that I control the given wallet";

/// CTF Exchange contract address on Polygon mainnet (CLOB V2).
pub const CTF_EXCHANGE: Address = address!("0xE111180000d2663C0091e4f400237545B87B996B");

/// Neg Risk CTF Exchange contract address on Polygon mainnet (CLOB V2).
pub const NEG_RISK_CTF_EXCHANGE: Address = address!("0xe2222d279d744050d28e00520010520000310F59");

const DOMAIN_NAME: &str = "Polymarket CTF Exchange";
const DOMAIN_VERSION: &str = "2";
const POLYGON_CHAIN_ID: u64 = 137;

// EIP-712 ClobAuth struct for L1 API authentication.
//
// Reference: <https://docs.polymarket.com/api-reference/authentication#l1-authentication>
alloy::sol! {
    struct ClobAuth {
        address address;
        string timestamp;
        uint256 nonce;
        string message;
    }
}

// EIP-712 Order struct for CLOB V2 CTFExchange.
//
// Fees are set by the protocol at match time (not signed) and per-address
// uniqueness comes from `timestamp` (milliseconds) rather than `nonce`.
alloy::sol! {
    struct Order {
        uint256 salt;
        address maker;
        address signer;
        uint256 tokenId;
        uint256 makerAmount;
        uint256 takerAmount;
        uint8 side;
        uint8 signatureType;
        uint256 timestamp;
        bytes32 metadata;
        bytes32 builder;
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

/// Signs a ClobAuth EIP-712 message for L1 API authentication.
///
/// Used to create or derive API credentials via the CLOB `/auth/api-key`
/// and `/auth/derive-api-key` endpoints.
///
/// Returns `(signer_address_hex, signature_hex)`.
pub fn sign_clob_auth(
    private_key: &EvmPrivateKey,
    timestamp: &str,
    nonce: u64,
) -> Result<(String, String)> {
    let key_hex = private_key
        .as_hex()
        .strip_prefix("0x")
        .unwrap_or(private_key.as_hex());
    let signer = PrivateKeySigner::from_str(key_hex)
        .map_err(|e| Error::bad_request(format!("Failed to create signer: {e}")))?;

    let address = signer.address();

    let auth = ClobAuth {
        address,
        timestamp: timestamp.to_string(),
        nonce: U256::from(nonce),
        message: CLOB_AUTH_MESSAGE.to_string(),
    };

    let domain = eip712_domain! {
        name: CLOB_AUTH_DOMAIN_NAME,
        version: CLOB_AUTH_DOMAIN_VERSION,
        chain_id: POLYGON_CHAIN_ID,
    };

    let signing_hash = auth.eip712_signing_hash(&domain);
    let signature = signer
        .sign_hash_sync(&signing_hash)
        .map_err(|e| Error::bad_request(format!("Failed to sign ClobAuth: {e}")))?;

    let r = signature.r();
    let s = signature.s();
    let v = if signature.v() { 28u8 } else { 27u8 };

    Ok((
        format!("{address:#x}"),
        format!("0x{r:064x}{s:064x}{v:02x}"),
    ))
}

// Converts a PolymarketOrder to the EIP-712 Order struct
fn build_eip712_order(order: &PolymarketOrder) -> Result<Order> {
    Ok(Order {
        salt: U256::from(order.salt),
        maker: parse_address(&order.maker, "maker")?,
        signer: parse_address(&order.signer, "signer")?,
        tokenId: U256::from_str(order.token_id.as_str())
            .map_err(|e| Error::bad_request(format!("Invalid token ID: {e}")))?,
        makerAmount: decimal_to_u256(order.maker_amount, "maker_amount")?,
        takerAmount: decimal_to_u256(order.taker_amount, "taker_amount")?,
        side: order_side_to_u8(order.side),
        signatureType: order.signature_type as u8,
        timestamp: U256::from_str(&order.timestamp)
            .map_err(|e| Error::bad_request(format!("Invalid timestamp: {e}")))?,
        metadata: parse_bytes32(&order.metadata, "metadata")?,
        builder: parse_bytes32(&order.builder, "builder")?,
    })
}

fn parse_address(addr: &str, field: &str) -> Result<Address> {
    Address::from_str(addr).map_err(|e| Error::bad_request(format!("Invalid {field} address: {e}")))
}

fn parse_bytes32(value: &str, field: &str) -> Result<FixedBytes<32>> {
    FixedBytes::<32>::from_str(value)
        .map_err(|e| Error::bad_request(format!("Invalid {field} bytes32: {e}")))
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
    use alloy_primitives::{Signature, keccak256};
    use nautilus_core::hex;
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::common::enums::SignatureType;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    fn test_signer() -> OrderSigner {
        let pk = EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap();
        OrderSigner::new(&pk).unwrap()
    }

    const ZERO_BYTES32: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";

    fn test_order() -> PolymarketOrder {
        PolymarketOrder {
            salt: 123456789,
            maker: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_string(),
            signer: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_string(),
            token_id: Ustr::from(
                "71321045679252212594626385532706912750332728571942532289631379312455583992563",
            ),
            maker_amount: dec!(100000000),
            taker_amount: dec!(50000000),
            side: PolymarketOrderSide::Buy,
            signature_type: SignatureType::Eoa,
            expiration: "0".to_string(),
            timestamp: "1713398400000".to_string(),
            metadata: ZERO_BYTES32.to_string(),
            builder: ZERO_BYTES32.to_string(),
            signature: String::new(),
        }
    }

    #[rstest]
    fn test_order_typehash_matches_contract() {
        // ORDER_TYPEHASH from the CLOB V2 CTFExchange contract
        let expected = keccak256(
            "Order(uint256 salt,address maker,address signer,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint8 side,uint8 signatureType,uint256 timestamp,bytes32 metadata,bytes32 builder)",
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
        assert_eq!(eip712.timestamp, U256::from(1713398400000u128));
        assert_eq!(eip712.metadata, FixedBytes::<32>::ZERO);
        assert_eq!(eip712.builder, FixedBytes::<32>::ZERO);
    }

    #[rstest]
    fn test_build_eip712_order_with_builder_code() {
        let mut order = test_order();
        order.builder =
            "0x0000000000000000000000000000000000000000000000000000000000000001".to_string();
        let eip712 = build_eip712_order(&order).unwrap();

        let mut expected = [0u8; 32];
        expected[31] = 1;
        assert_eq!(eip712.builder, FixedBytes::<32>::from(expected));
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
    fn test_v2_contract_addresses_pinned() {
        // Pin the V2 contract addresses so a revert to V1 is caught by unit tests.
        // V1 addresses (must NOT match): 0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E,
        // 0xC5d563A36AE78145C45a50134d48A1215220f80a.
        assert_eq!(
            format!("{CTF_EXCHANGE:#x}"),
            "0xe111180000d2663c0091e4f400237545b87b996b"
        );
        assert_eq!(
            format!("{NEG_RISK_CTF_EXCHANGE:#x}"),
            "0xe2222d279d744050d28e00520010520000310f59"
        );
    }

    #[rstest]
    fn test_domain_version_is_v2() {
        // Domain version is embedded in the EIP-712 signing hash; a revert to
        // "1" would silently break V2 order acceptance.
        assert_eq!(DOMAIN_VERSION, "2");
    }

    #[rstest]
    fn test_sign_order_recoverable() {
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

    // Reference vectors generated with `py_clob_client_v2==1.0.0`'s
    // `ExchangeOrderBuilderV2`. Same test private key (Hardhat account #0),
    // same contract addresses and chain id. Locks our EIP-712 hash + ECDSA
    // signature output to the SDK's, so drift between the two signers (domain
    // typo, struct field reorder, bytes32 padding, etc.) is caught locally
    // before orders get sent to the venue.
    const PARITY_TOKEN_ID: &str =
        "71321045679252212594626385532706912750332728571942532289631379312455583992563";

    fn parity_order(
        salt: u64,
        side: PolymarketOrderSide,
        signature_type: SignatureType,
        maker_amount: Decimal,
        taker_amount: Decimal,
        timestamp: &str,
        builder: &str,
    ) -> PolymarketOrder {
        PolymarketOrder {
            salt,
            maker: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".to_string(),
            signer: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".to_string(),
            token_id: Ustr::from(PARITY_TOKEN_ID),
            maker_amount,
            taker_amount,
            side,
            signature_type,
            expiration: "0".to_string(),
            timestamp: timestamp.to_string(),
            metadata: ZERO_BYTES32.to_string(),
            builder: builder.to_string(),
            signature: String::new(),
        }
    }

    fn expected_signing_hash(order: &PolymarketOrder, neg_risk: bool) -> B256 {
        let eip712_order = build_eip712_order(order).unwrap();
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
        eip712_order.eip712_signing_hash(&domain)
    }

    #[rstest]
    #[case::buy_standard_eoa(
        parity_order(
            123456789,
            PolymarketOrderSide::Buy,
            SignatureType::Eoa,
            dec!(100000000),
            dec!(50000000),
            "1713398400000",
            ZERO_BYTES32,
        ),
        false,
        "0x32961c48ddac87ed3582f8e02097cd0eff4fcf80460306bd44b3710438dfa64c",
        "0x89f178136333c8ebb32a19146cb891233e3202d474be6ef730c24dbc06ae4d2a0c99948a86d9b57de0f2c0bb8ec6964aa244ecf17cfa3a95b86878d0b64ad78a1b",
    )]
    #[case::sell_neg_risk_eoa(
        parity_order(
            987654321,
            PolymarketOrderSide::Sell,
            SignatureType::Eoa,
            dec!(50000000),
            dec!(100000000),
            "1713398400000",
            ZERO_BYTES32,
        ),
        true,
        "0x8b878404bd92dea2bfea9975c9fcd816ec70a57ae431cb20d67bb773744aaef3",
        "0xf7d60d64364e2615b08d9f69f3ea9afd3b4f83ecfbf05ddd3ca83f4916277fb97d2d080758d17c8e91c928aceb8ff252477ebaec051b672832500f63b4d36b061c",
    )]
    #[case::buy_with_builder_code_eoa(
        parity_order(
            1,
            PolymarketOrderSide::Buy,
            SignatureType::Eoa,
            dec!(100000000),
            dec!(50000000),
            "1713398500000",
            "0x0000000000000000000000000000000000000000000000000000000000000001",
        ),
        false,
        "0x3df0b6f6ddfca837bc36964cae968b34ad35640b5d98f557c104da97e804e36a",
        "0xf4d2b34659e8bc07a9572d40ee5a1639a1157409613b4c21566b1f33fd8fe11a364b3f306668cae7248ca7cdf72378f9266bc5628585aa939644400030671e081c",
    )]
    #[case::buy_poly_proxy(
        // V2 unblocks EIP-1271 smart contract wallet signing. signatureType
        // enters the typed-data hash directly, so a regression that only
        // manifests for proxy/safe wallets is undetectable from the Eoa
        // cases above.
        parity_order(
            111_111_111,
            PolymarketOrderSide::Buy,
            SignatureType::PolyProxy,
            dec!(100000000),
            dec!(50000000),
            "1713398400000",
            ZERO_BYTES32,
        ),
        false,
        "0x8f88fe2fb3448f4b8ba639992029f0a47a01a14d15b5f2bf9833516571efd279",
        "0x71a63c85b730cc934688f23ea6374afffef57a61690eba63dcb97a706c8a8d0f3d2a8e0280f3e252eb83be088ebae6b461a8eda18e559e079f59050b90057afa1c",
    )]
    #[case::sell_neg_risk_poly_gnosis_safe(
        parity_order(
            222_222_222,
            PolymarketOrderSide::Sell,
            SignatureType::PolyGnosisSafe,
            dec!(50000000),
            dec!(100000000),
            "1713398400000",
            ZERO_BYTES32,
        ),
        true,
        "0xb34248702810a1d76580234a33f942a9801c3680de54cb3ef104572a8d482190",
        "0xab9d33aee8b578fe5588c4a4b16bbef6fa05fc757020f95b98212e877a919e360a90296f02e97ecbabf3c200271ffe88eb9fd86912e8376cd27237bdad5f3abc1c",
    )]
    fn test_signature_matches_py_clob_client_v2(
        #[case] order: PolymarketOrder,
        #[case] neg_risk: bool,
        #[case] expected_hash_hex: &str,
        #[case] expected_signature_hex: &str,
    ) {
        let signer = test_signer();

        let hash = expected_signing_hash(&order, neg_risk);
        assert_eq!(format!("{hash:#x}"), expected_hash_hex, "signing hash");

        let signature = signer.sign_order(&order, neg_risk).unwrap();
        assert_eq!(signature, expected_signature_hex, "signature");
    }
}
