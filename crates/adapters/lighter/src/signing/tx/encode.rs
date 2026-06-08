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

//! Hash + sign pipeline for L2 transactions.
//!
//! Given any [`LighterTx`]:
//!
//! 1. Build the body field-element preimage (`[chain_id, tx_type, nonce,
//!    expired_at, account_index, api_key_index, ...body]`) and Poseidon2-hash
//!    it into a single `Fp5` digest.
//! 2. If the per-tx [`L2TxAttributes`] are non-empty, hash the
//!    `(type, value)` pair sequence into a second `Fp5` and Poseidon2 again
//!    over `body_digest || attributes_digest`. Empty attributes short-circuit
//!    to the body digest.
//! 3. Encode the resulting `Fp5` to 40 canonical little-endian bytes; this is
//!    the signed message hash and the venue's `tx_hash`.
//! 4. Sign with the caller-supplied `(sk, k)` to produce 80 sig bytes.
//! 5. Render the wire `tx_info` JSON with the same field order the upstream
//!    Go signer marshals (Sig is base64).
//!
//! Step 2's aggregation order — `body || attributes` — and the ascending-type
//! sort over attributes are both load-bearing for byte equality with the
//! sequencer; both come straight from `txtypes.L2TxAttributes.AggregateTxHash`.

use std::fmt::Write;

use base64::{Engine, engine::general_purpose::STANDARD as B64};

use super::types::{
    ApproveIntegratorTxInfo, CancelAllOrdersTxInfo, CancelOrderTxInfo, CreateOrderTxInfo,
    L2TxAttributes, LighterTx, ModifyOrderTxInfo, NB_ATTRIBUTES_PER_TX, OrderInfo, TxContext,
    UpdateLeverageTxInfo,
};
use crate::signing::{
    field::{Fp, Fp5},
    hash::{hash_to_quintic_extension, hash_two_to_quintic},
    schnorr::{PrivateKey, SIG_BYTES, Signature},
};

/// Canonical wire length of a Lighter L2 message hash: 40-byte LE `Fp5`.
pub const TX_HASH_BYTES: usize = 40;

/// Compute the signed message hash for any [`LighterTx`].
///
/// Combines the body Poseidon2 hash with the attribute hash when attributes
/// are populated; otherwise returns the body hash directly. The 40-byte LE
/// encoding is the venue-side `tx_hash` and the message [`PrivateKey::sign`]
/// consumes.
#[must_use]
pub fn compute_tx_hash<T: LighterTx>(tx: &T, chain_id: u32) -> [u8; TX_HASH_BYTES] {
    compute_tx_hash_fp5(tx, chain_id).to_le_bytes()
}

fn compute_tx_hash_fp5<T: LighterTx>(tx: &T, chain_id: u32) -> Fp5 {
    let body_elems = tx.hash_elements(chain_id);
    let body_digest = hash_to_quintic_extension(&body_elems);

    let attrs = tx.attributes();
    if attrs.is_empty() {
        return body_digest;
    }

    let attr_digest = hash_attributes(&attrs);
    hash_two_to_quintic(body_digest, attr_digest)
}

/// Hash the attribute table into an `Fp5` digest.
///
/// Mirrors `txtypes.L2TxAttributes.Hash`: emit the normalised
/// `(type, value)` pairs over [`NB_ATTRIBUTES_PER_TX`] slots, then run the
/// length-2N preimage through [`hash_to_quintic_extension`].
fn hash_attributes(attrs: &L2TxAttributes) -> Fp5 {
    let pairs = attrs.normalized_pairs();
    let mut elems = [Fp::ZERO; NB_ATTRIBUTES_PER_TX * 2];
    for (i, (ty, val)) in pairs.iter().enumerate() {
        elems[i * 2] = Fp::from_u64_reduce(u64::from(*ty));
        elems[i * 2 + 1] = Fp::from_u64_reduce(*val);
    }
    hash_to_quintic_extension(&elems)
}

/// Sign any [`LighterTx`] under `(sk, k)` and return the 80-byte signature
/// alongside the 40-byte tx hash.
///
/// `k` MUST be drawn from a cryptographic RNG and used at most once per key;
/// see [`PrivateKey::sign`] for the full nonce contract. The wire signature is
/// laid out as `s_le || e_le`.
#[must_use]
pub fn sign_tx<T: LighterTx>(
    tx: &T,
    chain_id: u32,
    sk: &PrivateKey,
    k: crate::signing::curve::Scalar,
) -> SignedTx {
    let hashed_msg = compute_tx_hash_fp5(tx, chain_id);
    let sig = sk.sign(hashed_msg, k);
    SignedTx {
        tx_hash: hashed_msg.to_le_bytes(),
        sig,
        sig_bytes: sig.to_le_bytes(),
    }
}

/// Outcome of [`sign_tx`]: the deterministic message hash plus the
/// `(s, e)` Schnorr signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SignedTx {
    /// 40-byte LE message hash that was signed; matches the venue `tx_hash`.
    pub tx_hash: [u8; TX_HASH_BYTES],
    /// `(s, e)` Schnorr signature.
    pub sig: Signature,
    /// `s_le || e_le` 80-byte wire encoding of [`Self::sig`].
    pub sig_bytes: [u8; SIG_BYTES],
}

/// JSON renderer for the L2 tx_info wire payload.
///
/// Field order and base64-encoded `Sig` match the upstream Go marshalling so
/// the resulting string is byte-equivalent (modulo the random `Sig`) to what
/// the closed signer emits, and is what the sequencer expects on `sendTx`.
#[derive(Debug)]
pub struct TxInfoJson;

impl TxInfoJson {
    /// Render a signed `UpdateLeverage` to its JSON payload.
    ///
    /// Wire field names mirror the upstream `txtypes.L2UpdateLeverageTxInfo`
    /// Go struct. The FFI wrapper for this kind passes only `SkipNonce`, so
    /// `L2TxAttributes` is `null` or a single `{"4":1}` entry.
    #[must_use]
    pub fn update_leverage(tx: &UpdateLeverageTxInfo, signed: &SignedTx) -> String {
        let mut out = String::with_capacity(256);
        out.push('{');
        write_ctx_lead(&mut out, tx.context);
        write_kv_i64(&mut out, "MarketIndex", i64::from(tx.market_index));
        write_kv_u64(
            &mut out,
            "InitialMarginFraction",
            u64::from(tx.initial_margin_fraction),
        ); // u16 widens to u64
        write_kv_u64(&mut out, "MarginMode", u64::from(tx.margin_mode));
        write_ctx_tail(&mut out, tx.context);
        write_sig(&mut out, signed);
        write_attributes_skip_nonce_only(&mut out, &tx.attributes());
        out.push('}');
        out
    }

    /// Render a signed `CreateOrder` to its JSON payload.
    #[must_use]
    pub fn create_order(tx: &CreateOrderTxInfo, signed: &SignedTx) -> String {
        let mut out = String::with_capacity(384);
        out.push('{');
        write_ctx_lead(&mut out, tx.context);
        write_order_info(&mut out, &tx.order);
        write_ctx_tail(&mut out, tx.context);
        write_sig(&mut out, signed);
        write_attributes_with_integrator(&mut out, &tx.attributes);
        out.push('}');
        out
    }

    /// Render a signed `ModifyOrder` to its JSON payload.
    #[must_use]
    pub fn modify_order(tx: &ModifyOrderTxInfo, signed: &SignedTx) -> String {
        let mut out = String::with_capacity(320);
        out.push('{');
        write_ctx_lead(&mut out, tx.context);
        write_kv_i64(&mut out, "MarketIndex", i64::from(tx.market_index));
        write_kv_i64(&mut out, "Index", tx.index);
        write_kv_i64(&mut out, "BaseAmount", tx.base_amount);
        write_kv_u64(&mut out, "Price", u64::from(tx.price));
        write_kv_u64(&mut out, "TriggerPrice", u64::from(tx.trigger_price));
        write_ctx_tail(&mut out, tx.context);
        write_sig(&mut out, signed);
        write_attributes_with_integrator(&mut out, &tx.attributes);
        out.push('}');
        out
    }

    /// Render a signed `CancelOrder` to its JSON payload.
    ///
    /// `CancelOrder` only accepts the `skip_nonce` L2 attribute.
    #[must_use]
    pub fn cancel_order(tx: &CancelOrderTxInfo, signed: &SignedTx) -> String {
        let mut out = String::with_capacity(256);
        out.push('{');
        write_ctx_lead(&mut out, tx.context);
        write_kv_i64(&mut out, "MarketIndex", i64::from(tx.market_index));
        write_kv_i64(&mut out, "Index", tx.index);
        write_ctx_tail(&mut out, tx.context);
        write_sig(&mut out, signed);
        write_attributes_skip_nonce_only(&mut out, &tx.attributes());
        out.push('}');
        out
    }

    /// Render a signed `CancelAllOrders` to its JSON payload.
    ///
    /// Wire field names mirror the upstream `txtypes.L2CancelAllOrdersTxInfo`
    /// Go struct. The FFI wrapper for this kind passes only `SkipNonce`, so
    /// `L2TxAttributes` is `null` or a single `{"4":1}` entry.
    #[must_use]
    pub fn cancel_all_orders(tx: &CancelAllOrdersTxInfo, signed: &SignedTx) -> String {
        let mut out = String::with_capacity(256);
        out.push('{');
        write_ctx_lead(&mut out, tx.context);
        write_kv_u64(&mut out, "TimeInForce", u64::from(tx.time_in_force));
        write_kv_i64(&mut out, "Time", tx.scheduled_time_ms);
        write_ctx_tail(&mut out, tx.context);
        write_sig(&mut out, signed);
        write_attributes_skip_nonce_only(&mut out, &tx.attributes());
        out.push('}');
        out
    }

    /// Render a signed `ApproveIntegrator` to its JSON payload.
    ///
    /// Pass an empty `l1_sig` when no L1 signature is present.
    /// `L2TxAttributes` uses the same null-or-`skip_nonce` shape as `CancelOrder`.
    #[must_use]
    pub fn approve_integrator(
        tx: &ApproveIntegratorTxInfo,
        signed: &SignedTx,
        l1_sig: &str,
    ) -> String {
        let mut out = String::with_capacity(384);
        out.push('{');
        write_ctx_lead(&mut out, tx.context);
        write_kv_i64(
            &mut out,
            "IntegratorAccountIndex",
            tx.integrator_account_index,
        );
        write_kv_u64(
            &mut out,
            "MaxPerpsTakerFee",
            u64::from(tx.max_perps_taker_fee),
        );
        write_kv_u64(
            &mut out,
            "MaxPerpsMakerFee",
            u64::from(tx.max_perps_maker_fee),
        );
        write_kv_u64(
            &mut out,
            "MaxSpotTakerFee",
            u64::from(tx.max_spot_taker_fee),
        );
        write_kv_u64(
            &mut out,
            "MaxSpotMakerFee",
            u64::from(tx.max_spot_maker_fee),
        );
        write_kv_i64(&mut out, "ApprovalExpiry", tx.approval_expiry);
        write_ctx_tail(&mut out, tx.context);
        write_sig(&mut out, signed);
        out.push_str("\"L1Sig\":\"");
        out.push_str(l1_sig);
        out.push_str("\",");
        write_attributes_skip_nonce_only(&mut out, &tx.attributes());
        out.push('}');
        out
    }
}

fn write_ctx_lead(out: &mut String, ctx: TxContext) {
    write_kv_i64(out, "AccountIndex", ctx.account_index);
    write_kv_u64(out, "ApiKeyIndex", u64::from(ctx.api_key_index));
}

fn write_ctx_tail(out: &mut String, ctx: TxContext) {
    write_kv_i64(out, "ExpiredAt", ctx.expired_at);
    write_kv_i64(out, "Nonce", ctx.nonce);
}

fn write_order_info(out: &mut String, order: &OrderInfo) {
    write_kv_i64(out, "MarketIndex", i64::from(order.market_index));
    write_kv_i64(out, "ClientOrderIndex", order.client_order_index);
    write_kv_i64(out, "BaseAmount", order.base_amount);
    write_kv_u64(out, "Price", u64::from(order.price));
    write_kv_u64(out, "IsAsk", u64::from(u8::from(order.is_ask)));
    write_kv_u64(out, "Type", u64::from(order.order_type));
    write_kv_u64(out, "TimeInForce", u64::from(order.time_in_force));
    write_kv_u64(out, "ReduceOnly", u64::from(u8::from(order.reduce_only)));
    write_kv_u64(out, "TriggerPrice", u64::from(order.trigger_price));
    write_kv_i64(out, "OrderExpiry", order.order_expiry);
}

fn write_sig(out: &mut String, signed: &SignedTx) {
    out.push_str("\"Sig\":\"");
    out.push_str(&B64.encode(signed.sig_bytes));
    out.push_str("\",");
}

// Match upstream marshalling: Create/Modify always emit integrator keys 1-3
fn write_attributes_with_integrator(out: &mut String, attrs: &L2TxAttributes) {
    out.push_str("\"L2TxAttributes\":{");
    let mut first = true;
    write_attr_pair(out, &mut first, "1", attrs.integrator_account_index);
    write_attr_pair(out, &mut first, "2", u64::from(attrs.integrator_taker_fee));
    write_attr_pair(out, &mut first, "3", u64::from(attrs.integrator_maker_fee));
    if attrs.skip_nonce != 0 {
        write_attr_pair(out, &mut first, "4", u64::from(attrs.skip_nonce));
    }
    out.push('}');
}

// Cancel/CancelAll/Withdraw/etc.: the FFI wrapper passes only `skip_nonce`,
// so the marshalled value is `null` when nothing is set, otherwise a single
// `{"4":1}` entry.
fn write_attributes_skip_nonce_only(out: &mut String, attrs: &L2TxAttributes) {
    if attrs.skip_nonce == 0 {
        out.push_str("\"L2TxAttributes\":null");
        return;
    }
    out.push_str("\"L2TxAttributes\":{\"4\":");
    write_u64(out, u64::from(attrs.skip_nonce));
    out.push('}');
}

fn write_attr_pair(out: &mut String, first: &mut bool, key: &str, value: u64) {
    if !*first {
        out.push(',');
    }
    *first = false;
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
    write_u64(out, value);
}

fn write_kv_i64(out: &mut String, key: &str, value: i64) {
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
    write_i64(out, value);
    out.push(',');
}

fn write_kv_u64(out: &mut String, key: &str, value: u64) {
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
    write_u64(out, value);
    out.push(',');
}

fn write_i64(out: &mut String, value: i64) {
    write!(out, "{value}").expect("writing into String never fails");
}

fn write_u64(out: &mut String, value: u64) {
    write!(out, "{value}").expect("writing into String never fails");
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;
    use serde::Deserialize;

    use super::*;
    use crate::signing::{
        curve::{SCALAR_BYTES, Scalar},
        field::Fp,
        fixtures::{arb_scalar_nonzero, bytes_to_hex, decode_scalar_bytes, hex_to_bytes},
        tx::types::{NB_ATTRIBUTES_PER_TX, OrderInfo, TxContext},
    };

    const ORACLE_JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_data/signing_tx_oracle.json",
    ));

    #[derive(Debug, Deserialize)]
    struct OracleFile {
        vectors: Vec<OracleVector>,
    }

    #[derive(Debug, Deserialize)]
    struct OracleVector {
        kind: String,
        chain_id: u32,
        sk: String,
        account_index: i64,
        api_key_index: u8,
        nonce: i64,
        expired_at: i64,
        fields: serde_json::Value,
        tx_type: u8,
        tx_info: String,
        tx_hash: String,
        sig: String,
    }

    fn ctx_for(v: &OracleVector) -> TxContext {
        TxContext {
            account_index: v.account_index,
            api_key_index: v.api_key_index,
            nonce: v.nonce,
            expired_at: v.expired_at,
        }
    }

    fn attrs_from(fields: &serde_json::Value) -> L2TxAttributes {
        L2TxAttributes {
            integrator_account_index: fields["integrator_account_index"].as_u64().unwrap_or(0),
            integrator_taker_fee: fields["integrator_taker_fee"].as_u64().unwrap_or(0) as u32,
            integrator_maker_fee: fields["integrator_maker_fee"].as_u64().unwrap_or(0) as u32,
            skip_nonce: fields["skip_nonce"].as_u64().unwrap_or(0) as u8,
        }
    }

    fn expect_create_order(v: &OracleVector) -> CreateOrderTxInfo {
        let f = &v.fields;
        CreateOrderTxInfo {
            context: ctx_for(v),
            order: OrderInfo {
                market_index: f["market_index"].as_i64().unwrap() as i16,
                client_order_index: f["client_order_index"].as_i64().unwrap(),
                base_amount: f["base_amount"].as_i64().unwrap(),
                price: f["price"].as_u64().unwrap() as u32,
                is_ask: f["is_ask"].as_bool().unwrap(),
                order_type: f["order_type"].as_u64().unwrap() as u8,
                time_in_force: f["time_in_force"].as_u64().unwrap() as u8,
                reduce_only: f["reduce_only"].as_bool().unwrap(),
                trigger_price: f["trigger_price"].as_u64().unwrap() as u32,
                order_expiry: f["order_expiry"].as_i64().unwrap(),
            },
            attributes: attrs_from(f),
        }
    }

    fn expect_cancel_order(v: &OracleVector) -> CancelOrderTxInfo {
        let f = &v.fields;
        CancelOrderTxInfo {
            context: ctx_for(v),
            market_index: f["market_index"].as_i64().unwrap() as i16,
            index: f["index"].as_i64().unwrap(),
            skip_nonce: f["skip_nonce"].as_u64().unwrap_or(0) as u8,
        }
    }

    fn expect_modify_order(v: &OracleVector) -> ModifyOrderTxInfo {
        let f = &v.fields;
        ModifyOrderTxInfo {
            context: ctx_for(v),
            market_index: f["market_index"].as_i64().unwrap() as i16,
            index: f["index"].as_i64().unwrap(),
            base_amount: f["base_amount"].as_i64().unwrap(),
            price: f["price"].as_u64().unwrap() as u32,
            trigger_price: f["trigger_price"].as_u64().unwrap() as u32,
            attributes: attrs_from(f),
        }
    }

    fn expect_approve_integrator(v: &OracleVector) -> ApproveIntegratorTxInfo {
        let f = &v.fields;
        ApproveIntegratorTxInfo {
            context: ctx_for(v),
            integrator_account_index: f["integrator_account_index"].as_i64().unwrap(),
            max_perps_taker_fee: f["max_perps_taker_fee"].as_u64().unwrap() as u32,
            max_perps_maker_fee: f["max_perps_maker_fee"].as_u64().unwrap() as u32,
            max_spot_taker_fee: f["max_spot_taker_fee"].as_u64().unwrap() as u32,
            max_spot_maker_fee: f["max_spot_maker_fee"].as_u64().unwrap() as u32,
            approval_expiry: f["approval_expiry"].as_i64().unwrap(),
            skip_nonce: f["skip_nonce"].as_u64().unwrap_or(0) as u8,
        }
    }

    fn assert_hash_matches<T: LighterTx>(tx: &T, v: &OracleVector) {
        let got = compute_tx_hash(tx, v.chain_id);
        assert_eq!(
            bytes_to_hex(&got),
            v.tx_hash,
            "{}: tx_hash diverged",
            v.kind,
        );
    }

    fn assert_oracle_sig_verifies<T: LighterTx>(tx: &T, v: &OracleVector) {
        let sig_bytes = hex_to_bytes(&v.sig);
        assert_eq!(sig_bytes.len(), SIG_BYTES);
        let mut buf = [0u8; SIG_BYTES];
        buf.copy_from_slice(&sig_bytes);
        let sig = Signature::from_le_bytes_reduce(buf);

        let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
        let pk = sk.public_key();
        let tx_hash = compute_tx_hash(tx, v.chain_id);
        let hashed = Fp5::try_from_le_bytes(tx_hash).expect("oracle hash must be canonical");

        assert!(
            pk.verify(hashed, &sig),
            "{}: oracle sig must verify against the recomputed hash",
            v.kind,
        );
    }

    fn assert_round_trip_sign<T: LighterTx>(tx: &T, v: &OracleVector) {
        let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
        // Pick a nonzero, fixture-derived `k` — any non-zero canonical scalar
        // is valid. Guarding against `k == 0` and non-canonical limbs makes
        // the helper fail loudly on the test scaffold rather than producing
        // an undefined signature if the XOR happens to land on a bad value.
        let mut k_bytes = decode_scalar_bytes(&v.sk);
        k_bytes[0] ^= 0x01;
        let k = Scalar::from_le_bytes_reduce(k_bytes);
        assert!(!k.is_zero(), "{}: derived k must be non-zero", v.kind);
        assert!(k.is_canonical(), "{}: derived k must be canonical", v.kind,);

        let signed = sign_tx(tx, v.chain_id, &sk, k);
        assert_eq!(
            bytes_to_hex(&signed.tx_hash),
            v.tx_hash,
            "{}: sign_tx tx_hash diverged",
            v.kind,
        );
        let pk = sk.public_key();
        let hashed = Fp5::try_from_le_bytes(signed.tx_hash).unwrap();
        assert!(
            pk.verify(hashed, &signed.sig),
            "{}: round-trip sig must verify",
            v.kind,
        );
    }

    #[rstest]
    fn oracle_tx_hash_matches_create_order() {
        let suite: OracleFile = serde_json::from_str(ORACLE_JSON).expect("parse oracle");
        for v in suite.vectors.iter().filter(|v| v.kind == "create_order") {
            assert_eq!(v.tx_type, 14);
            let tx = expect_create_order(v);
            assert_hash_matches(&tx, v);
            assert_oracle_sig_verifies(&tx, v);
            assert_round_trip_sign(&tx, v);
        }
    }

    #[rstest]
    fn oracle_tx_hash_matches_cancel_order() {
        let suite: OracleFile = serde_json::from_str(ORACLE_JSON).expect("parse oracle");
        for v in suite.vectors.iter().filter(|v| v.kind == "cancel_order") {
            assert_eq!(v.tx_type, 15);
            let tx = expect_cancel_order(v);
            assert_hash_matches(&tx, v);
            assert_oracle_sig_verifies(&tx, v);
            assert_round_trip_sign(&tx, v);
        }
    }

    #[rstest]
    fn oracle_tx_hash_matches_modify_order() {
        let suite: OracleFile = serde_json::from_str(ORACLE_JSON).expect("parse oracle");
        for v in suite.vectors.iter().filter(|v| v.kind == "modify_order") {
            assert_eq!(v.tx_type, 17);
            let tx = expect_modify_order(v);
            assert_hash_matches(&tx, v);
            assert_oracle_sig_verifies(&tx, v);
            assert_round_trip_sign(&tx, v);
        }
    }

    #[rstest]
    fn oracle_tx_hash_matches_approve_integrator() {
        let suite: OracleFile = serde_json::from_str(ORACLE_JSON).expect("parse oracle");
        for v in suite
            .vectors
            .iter()
            .filter(|v| v.kind == "approve_integrator")
        {
            assert_eq!(v.tx_type, 45);
            let tx = expect_approve_integrator(v);
            assert_hash_matches(&tx, v);
            assert_oracle_sig_verifies(&tx, v);
            assert_round_trip_sign(&tx, v);
        }
    }

    /// Replace the random-`k`-driven `Sig` block with a stable placeholder
    /// so two JSON renderings of the same body can be compared byte-for-byte
    /// regardless of which `k` produced them.
    fn redact_sig(json: &str) -> String {
        let start = json.find("\"Sig\":\"").expect("Sig key missing");
        let after_open = start + "\"Sig\":\"".len();
        let close = json[after_open..]
            .find('"')
            .map(|i| after_open + i)
            .expect("Sig value not closed");
        let mut out = String::with_capacity(json.len());
        out.push_str(&json[..after_open]);
        out.push_str("REDACTED");
        out.push_str(&json[close..]);
        out
    }

    fn signed_with_fixture_k(
        v: &OracleVector,
        sk: &PrivateKey,
        signed_tx: impl Fn(Scalar) -> SignedTx,
    ) -> SignedTx {
        let _ = sk;
        let mut k_bytes = decode_scalar_bytes(&v.sk);
        k_bytes[0] ^= 0x01;
        signed_tx(Scalar::from_le_bytes_reduce(k_bytes))
    }

    #[rstest]
    fn create_order_json_byte_equals_oracle_modulo_sig() {
        let suite: OracleFile = serde_json::from_str(ORACLE_JSON).expect("parse oracle");
        for v in suite.vectors.iter().filter(|v| v.kind == "create_order") {
            let tx = expect_create_order(v);
            let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
            let signed = signed_with_fixture_k(v, &sk, |k| sign_tx(&tx, v.chain_id, &sk, k));
            let json = TxInfoJson::create_order(&tx, &signed);
            assert_eq!(
                redact_sig(&json),
                redact_sig(&v.tx_info),
                "create_order tx_info diverged",
            );
        }
    }

    #[rstest]
    fn cancel_order_json_emits_null_attributes_when_empty() {
        let suite: OracleFile = serde_json::from_str(ORACLE_JSON).expect("parse oracle");
        for v in suite.vectors.iter().filter(|v| v.kind == "cancel_order") {
            let tx = expect_cancel_order(v);
            let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
            let signed = signed_with_fixture_k(v, &sk, |k| sign_tx(&tx, v.chain_id, &sk, k));
            let json = TxInfoJson::cancel_order(&tx, &signed);
            assert_eq!(
                redact_sig(&json),
                redact_sig(&v.tx_info),
                "cancel_order tx_info diverged",
            );
        }
    }

    #[rstest]
    fn modify_order_json_byte_equals_oracle_modulo_sig() {
        let suite: OracleFile = serde_json::from_str(ORACLE_JSON).expect("parse oracle");
        for v in suite.vectors.iter().filter(|v| v.kind == "modify_order") {
            let tx = expect_modify_order(v);
            let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
            let signed = signed_with_fixture_k(v, &sk, |k| sign_tx(&tx, v.chain_id, &sk, k));
            let json = TxInfoJson::modify_order(&tx, &signed);
            assert_eq!(
                redact_sig(&json),
                redact_sig(&v.tx_info),
                "modify_order tx_info diverged",
            );
        }
    }

    fn stub_signed() -> SignedTx {
        SignedTx {
            tx_hash: [0u8; TX_HASH_BYTES],
            sig: Signature {
                s: Scalar::from_le_bytes_reduce([0u8; SCALAR_BYTES]),
                e: Scalar::from_le_bytes_reduce([0u8; SCALAR_BYTES]),
            },
            sig_bytes: [0u8; SIG_BYTES],
        }
    }

    fn stub_context() -> TxContext {
        TxContext {
            account_index: 12_345,
            api_key_index: 5,
            nonce: 7,
            expired_at: 1_777_804_395_089,
        }
    }

    #[rstest]
    fn cancel_all_orders_json_pins_field_order() {
        // Pins wire layout for `txtypes.L2CancelAllOrdersTxInfo`. No oracle
        // vector covers this kind; live testnet replay is the only path that
        // verifies byte-equality with the closed Go signer.
        let tx = CancelAllOrdersTxInfo {
            context: stub_context(),
            time_in_force: 0,
            scheduled_time_ms: 0,
            skip_nonce: 0,
        };
        let json = TxInfoJson::cancel_all_orders(&tx, &stub_signed());
        let expected = concat!(
            r#"{"AccountIndex":12345,"ApiKeyIndex":5,"#,
            r#""TimeInForce":0,"Time":0,"#,
            r#""ExpiredAt":1777804395089,"Nonce":7,"#,
            r#""Sig":"REDACTED","L2TxAttributes":null}"#,
        );
        assert_eq!(redact_sig(&json), expected);
    }

    #[rstest]
    fn cancel_all_orders_json_emits_skip_nonce_attr_when_set() {
        let tx = CancelAllOrdersTxInfo {
            context: stub_context(),
            time_in_force: 1, // Scheduled
            scheduled_time_ms: 1_800_000_000_000,
            skip_nonce: 1,
        };
        let json = TxInfoJson::cancel_all_orders(&tx, &stub_signed());
        let expected = concat!(
            r#"{"AccountIndex":12345,"ApiKeyIndex":5,"#,
            r#""TimeInForce":1,"Time":1800000000000,"#,
            r#""ExpiredAt":1777804395089,"Nonce":7,"#,
            r#""Sig":"REDACTED","L2TxAttributes":{"4":1}}"#,
        );
        assert_eq!(redact_sig(&json), expected);
    }

    #[rstest]
    fn update_leverage_json_pins_field_order() {
        // Pins wire layout for `txtypes.L2UpdateLeverageTxInfo`. No oracle
        // vector covers this kind; live testnet replay verifies byte-equality.
        let tx = UpdateLeverageTxInfo {
            context: stub_context(),
            market_index: 3,
            initial_margin_fraction: 500,
            margin_mode: 1,
            skip_nonce: 0,
        };
        let json = TxInfoJson::update_leverage(&tx, &stub_signed());
        let expected = concat!(
            r#"{"AccountIndex":12345,"ApiKeyIndex":5,"#,
            r#""MarketIndex":3,"InitialMarginFraction":500,"MarginMode":1,"#,
            r#""ExpiredAt":1777804395089,"Nonce":7,"#,
            r#""Sig":"REDACTED","L2TxAttributes":null}"#,
        );
        assert_eq!(redact_sig(&json), expected);
    }

    #[rstest]
    fn approve_integrator_json_byte_equals_oracle_modulo_sig() {
        let suite: OracleFile = serde_json::from_str(ORACLE_JSON).expect("parse oracle");
        for v in suite
            .vectors
            .iter()
            .filter(|v| v.kind == "approve_integrator")
        {
            let tx = expect_approve_integrator(v);
            let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
            let signed = signed_with_fixture_k(v, &sk, |k| sign_tx(&tx, v.chain_id, &sk, k));
            let json = TxInfoJson::approve_integrator(&tx, &signed, "");
            assert_eq!(
                redact_sig(&json),
                redact_sig(&v.tx_info),
                "approve_integrator tx_info diverged",
            );
        }
    }

    fn arb_tx_context() -> impl Strategy<Value = TxContext> {
        (any::<i64>(), any::<u8>(), any::<i64>(), any::<i64>()).prop_map(
            |(account_index, api_key_index, nonce, expired_at)| TxContext {
                account_index,
                api_key_index,
                nonce,
                expired_at,
            },
        )
    }

    fn arb_order_info() -> impl Strategy<Value = OrderInfo> {
        (
            any::<i16>(),
            any::<i64>(),
            any::<i64>(),
            any::<u32>(),
            any::<bool>(),
            any::<u8>(),
            any::<u8>(),
            any::<bool>(),
            any::<u32>(),
            any::<i64>(),
        )
            .prop_map(
                |(
                    market_index,
                    client_order_index,
                    base_amount,
                    price,
                    is_ask,
                    order_type,
                    time_in_force,
                    reduce_only,
                    trigger_price,
                    order_expiry,
                )| OrderInfo {
                    market_index,
                    client_order_index,
                    base_amount,
                    price,
                    is_ask,
                    order_type,
                    time_in_force,
                    reduce_only,
                    trigger_price,
                    order_expiry,
                },
            )
    }

    fn arb_l2_attributes() -> impl Strategy<Value = L2TxAttributes> {
        (any::<u64>(), any::<u32>(), any::<u32>(), any::<u8>()).prop_map(
            |(integrator_account_index, integrator_taker_fee, integrator_maker_fee, skip_nonce)| {
                L2TxAttributes {
                    integrator_account_index,
                    integrator_taker_fee,
                    integrator_maker_fee,
                    skip_nonce,
                }
            },
        )
    }

    fn arb_create_order() -> impl Strategy<Value = CreateOrderTxInfo> {
        (arb_tx_context(), arb_order_info(), arb_l2_attributes()).prop_map(
            |(context, order, attributes)| CreateOrderTxInfo {
                context,
                order,
                attributes,
            },
        )
    }

    proptest! {
        /// `compute_tx_hash` is deterministic over identical input.
        #[rstest]
        fn prop_compute_tx_hash_deterministic(tx in arb_create_order(), chain_id in any::<u32>()) {
            prop_assert_eq!(compute_tx_hash(&tx, chain_id), compute_tx_hash(&tx, chain_id));
        }

        /// Empty attributes short-circuit through the body-only branch:
        /// `compute_tx_hash` matches `hash_to_quintic_extension(body_elems)`
        /// directly when `attributes.is_empty()`.
        #[rstest]
        fn prop_empty_attrs_branch_uses_body_hash_only(
            mut tx in arb_create_order(),
            chain_id in any::<u32>(),
        ) {
            tx.attributes = L2TxAttributes::default();
            let from_pipeline = compute_tx_hash(&tx, chain_id);
            let from_body =
                hash_to_quintic_extension(&tx.hash_elements(chain_id)).to_le_bytes();
            prop_assert_eq!(from_pipeline, from_body);
        }

        /// Non-empty attributes engage the aggregation branch: the pipeline
        /// hash matches `hash_two_to_quintic(body_digest, attr_digest)`
        /// computed explicitly from the preimage. Pins the branch and the
        /// aggregation formula deterministically (no collision-resistance
        /// assumption).
        #[rstest]
        fn prop_non_empty_attrs_matches_aggregation_formula(
            mut tx in arb_create_order(),
            chain_id in any::<u32>(),
        ) {
            // Force at least one attribute slot populated so the aggregation
            // branch fires.
            tx.attributes = L2TxAttributes {
                integrator_account_index: 0,
                integrator_taker_fee: 0,
                integrator_maker_fee: 0,
                skip_nonce: 1,
            };
            let from_pipeline = compute_tx_hash(&tx, chain_id);
            let expected = explicit_tx_hash(&tx, chain_id);
            prop_assert_eq!(from_pipeline, expected);
        }
    }

    /// Recompute the tx hash explicitly from `hash_elements` and the
    /// normalized attribute pairs, mirroring the two branches inside
    /// `compute_tx_hash_fp5`. Used by the proptests below to assert the
    /// pipeline hash equals the explicit branch formula without making a
    /// collision-resistance assumption.
    fn explicit_tx_hash<T: LighterTx>(tx: &T, chain_id: u32) -> [u8; TX_HASH_BYTES] {
        let body_elems = tx.hash_elements(chain_id);
        let body_digest = hash_to_quintic_extension(&body_elems);
        let attrs = tx.attributes();
        let result = if attrs.is_empty() {
            body_digest
        } else {
            let pairs = attrs.normalized_pairs();
            let mut elems = [Fp::from_u64_reduce(0); NB_ATTRIBUTES_PER_TX * 2];
            for (i, (ty, val)) in pairs.iter().enumerate() {
                elems[i * 2] = Fp::from_u64_reduce(u64::from(*ty));
                elems[i * 2 + 1] = Fp::from_u64_reduce(*val);
            }
            let attr_digest = hash_to_quintic_extension(&elems);
            hash_two_to_quintic(body_digest, attr_digest)
        };
        result.to_le_bytes()
    }

    /// Mutator selector for `prop_field_change_changes_hash`. Each variant
    /// names a body, attribute, or context field that participates in the
    /// signed hash; mutating it MUST change the hash.
    #[derive(Debug, Clone, Copy)]
    enum CreateOrderField {
        ChainId,
        AccountIndex,
        ApiKeyIndex,
        Nonce,
        ExpiredAt,
        MarketIndex,
        ClientOrderIndex,
        BaseAmount,
        Price,
        IsAsk,
        OrderType,
        TimeInForce,
        ReduceOnly,
        TriggerPrice,
        OrderExpiry,
        IntegratorAccountIndex,
        IntegratorTakerFee,
        IntegratorMakerFee,
        SkipNonce,
    }

    fn arb_create_order_field() -> impl Strategy<Value = CreateOrderField> {
        prop_oneof![
            Just(CreateOrderField::ChainId),
            Just(CreateOrderField::AccountIndex),
            Just(CreateOrderField::ApiKeyIndex),
            Just(CreateOrderField::Nonce),
            Just(CreateOrderField::ExpiredAt),
            Just(CreateOrderField::MarketIndex),
            Just(CreateOrderField::ClientOrderIndex),
            Just(CreateOrderField::BaseAmount),
            Just(CreateOrderField::Price),
            Just(CreateOrderField::IsAsk),
            Just(CreateOrderField::OrderType),
            Just(CreateOrderField::TimeInForce),
            Just(CreateOrderField::ReduceOnly),
            Just(CreateOrderField::TriggerPrice),
            Just(CreateOrderField::OrderExpiry),
            Just(CreateOrderField::IntegratorAccountIndex),
            Just(CreateOrderField::IntegratorTakerFee),
            Just(CreateOrderField::IntegratorMakerFee),
            Just(CreateOrderField::SkipNonce),
        ]
    }

    /// Apply a `+ 1` (or `!` for booleans) mutation to the named field.
    /// `chain_id` is mutated in-place via the second tuple element; all
    /// other fields are mutated on the returned `CreateOrderTxInfo`.
    fn mutate_create_order_field(
        base: CreateOrderTxInfo,
        chain_id: u32,
        field: CreateOrderField,
    ) -> (CreateOrderTxInfo, u32) {
        let mut alt = base;
        let mut chain = chain_id;
        match field {
            CreateOrderField::ChainId => chain = chain.wrapping_add(1),
            CreateOrderField::AccountIndex => {
                alt.context.account_index = alt.context.account_index.wrapping_add(1);
            }
            CreateOrderField::ApiKeyIndex => {
                alt.context.api_key_index = alt.context.api_key_index.wrapping_add(1);
            }
            CreateOrderField::Nonce => alt.context.nonce = alt.context.nonce.wrapping_add(1),
            CreateOrderField::ExpiredAt => {
                alt.context.expired_at = alt.context.expired_at.wrapping_add(1);
            }
            CreateOrderField::MarketIndex => {
                alt.order.market_index = alt.order.market_index.wrapping_add(1);
            }
            CreateOrderField::ClientOrderIndex => {
                alt.order.client_order_index = alt.order.client_order_index.wrapping_add(1);
            }
            CreateOrderField::BaseAmount => {
                alt.order.base_amount = alt.order.base_amount.wrapping_add(1);
            }
            CreateOrderField::Price => alt.order.price = alt.order.price.wrapping_add(1),
            CreateOrderField::IsAsk => alt.order.is_ask = !alt.order.is_ask,
            CreateOrderField::OrderType => {
                alt.order.order_type = alt.order.order_type.wrapping_add(1);
            }
            CreateOrderField::TimeInForce => {
                alt.order.time_in_force = alt.order.time_in_force.wrapping_add(1);
            }
            CreateOrderField::ReduceOnly => alt.order.reduce_only = !alt.order.reduce_only,
            CreateOrderField::TriggerPrice => {
                alt.order.trigger_price = alt.order.trigger_price.wrapping_add(1);
            }
            CreateOrderField::OrderExpiry => {
                alt.order.order_expiry = alt.order.order_expiry.wrapping_add(1);
            }
            CreateOrderField::IntegratorAccountIndex => {
                alt.attributes.integrator_account_index =
                    alt.attributes.integrator_account_index.wrapping_add(1);
            }
            CreateOrderField::IntegratorTakerFee => {
                alt.attributes.integrator_taker_fee =
                    alt.attributes.integrator_taker_fee.wrapping_add(1);
            }
            CreateOrderField::IntegratorMakerFee => {
                alt.attributes.integrator_maker_fee =
                    alt.attributes.integrator_maker_fee.wrapping_add(1);
            }
            CreateOrderField::SkipNonce => {
                alt.attributes.skip_nonce = alt.attributes.skip_nonce.wrapping_add(1);
            }
        }
        (alt, chain)
    }

    proptest! {
        /// Mutating any single body, attribute, or context field changes the
        /// signed *preimage* (`hash_elements` plus `attributes()` for
        /// attribute fields). Pins body-element ordering and attribute-slot
        /// participation deterministically — a regression that drops or
        /// swaps a field makes at least one mutation a no-op on the
        /// preimage. Asserting on the preimage rather than the digest
        /// avoids the hash-collision overreach (Poseidon compresses to
        /// ~320 bits, so universal-distinctness on digests is not a
        /// primitive contract).
        #[rstest]
        fn prop_field_change_changes_preimage(
            base in arb_create_order(),
            chain_id in any::<u32>(),
            field in arb_create_order_field(),
        ) {
            let (alt, chain) = mutate_create_order_field(base, chain_id, field);
            let base_preimage = (base.hash_elements(chain_id), base.attributes());
            let alt_preimage = (alt.hash_elements(chain), alt.attributes());
            prop_assert_ne!(
                base_preimage,
                alt_preimage,
                "mutation {:?} did not change preimage",
                field,
            );
        }

        /// `compute_tx_hash` matches the explicit branch formula derived
        /// from `hash_elements` and `normalized_pairs`. Pins both branches
        /// (empty / non-empty attributes) and the branch selector
        /// (`L2TxAttributes::is_empty`) deterministically.
        #[rstest]
        fn prop_compute_tx_hash_matches_explicit_formula(
            tx in arb_create_order(),
            chain_id in any::<u32>(),
        ) {
            prop_assert_eq!(compute_tx_hash(&tx, chain_id), explicit_tx_hash(&tx, chain_id));
        }

        /// `sign_tx` followed by `verify` succeeds for any well-formed
        /// CreateOrder body and any non-zero canonical nonce / secret key.
        #[rstest]
        fn prop_sign_then_verify_for_arbitrary_tx(
            tx in arb_create_order(),
            chain_id in any::<u32>(),
            sk in arb_scalar_nonzero(),
            k in arb_scalar_nonzero(),
        ) {
            let private_key = PrivateKey::from_scalar(sk);
            let pk = private_key.public_key();
            let signed = sign_tx(&tx, chain_id, &private_key, k);

            // Recomputing the hash through `compute_tx_hash` must match
            // the value returned by `sign_tx`.
            prop_assert_eq!(signed.tx_hash, compute_tx_hash(&tx, chain_id));

            let hashed = Fp5::try_from_le_bytes(signed.tx_hash)
                .expect("tx hash must encode a canonical Fp5");
            prop_assert!(pk.verify(hashed, &signed.sig));
        }
    }

    #[rstest]
    fn cancel_order_json_emits_skip_nonce_only_attribute() {
        // Synthesised case: skip_nonce=1, no integrator slots
        let tx = CancelOrderTxInfo {
            context: TxContext {
                account_index: 1,
                api_key_index: 0,
                nonce: 0,
                expired_at: 0,
            },
            market_index: 0,
            index: 1,
            skip_nonce: 1,
        };
        let sk = PrivateKey::from_le_bytes_reduce([0x42; SCALAR_BYTES]);
        let signed = sign_tx(&tx, 300, &sk, Scalar::ONE);
        let json = TxInfoJson::cancel_order(&tx, &signed);
        assert!(
            json.ends_with(",\"L2TxAttributes\":{\"4\":1}}"),
            "was {json}"
        );
    }
}
