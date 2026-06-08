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

//! Crypto-path micro-benches.
//!
//! Polymarket has two distinct signing surfaces:
//! - L1 EIP-712 order signing (`OrderSigner::sign_order`) on the hot order path
//! - L2 HMAC-SHA256 request signing (`Credential::sign`) on every authenticated
//!   REST call
//!
//! Numbers here decompose the EIP-712 cost (typed-data hash + ECDSA) from the
//! HMAC path so a regression in either is localisable.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_polymarket::{
    common::{
        credential::{Credential, EvmPrivateKey},
        enums::{PolymarketOrderSide, SignatureType},
    },
    http::models::PolymarketOrder,
    signing::eip712::{OrderSigner, order_hash, sign_clob_auth},
};
use rust_decimal_macros::dec;
use ustr::Ustr;

const TEST_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const TEST_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const TEST_TOKEN_ID: &str =
    "71321045679252212594626385532706912750332728571942532289631379312455583992563";
const ZERO_BYTES32: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";
const TEST_API_SECRET: &str = "dGVzdC1zZWNyZXQtMzItYnl0ZXMtbG9uZy12YWx1ZS0wMQ==";

fn signer() -> OrderSigner {
    let key = EvmPrivateKey::new(TEST_KEY).unwrap();
    OrderSigner::new(&key).unwrap()
}

fn sample_order() -> PolymarketOrder {
    PolymarketOrder {
        salt: 123_456_789,
        maker: TEST_ADDRESS.to_string(),
        signer: TEST_ADDRESS.to_string(),
        token_id: Ustr::from(TEST_TOKEN_ID),
        maker_amount: dec!(50000000),
        taker_amount: dec!(100000000),
        side: PolymarketOrderSide::Buy,
        signature_type: SignatureType::Eoa,
        expiration: "0".to_string(),
        timestamp: "1713398400000".to_string(),
        metadata: ZERO_BYTES32.to_string(),
        builder: ZERO_BYTES32.to_string(),
        signature: String::new(),
    }
}

fn bench_sign_order(c: &mut Criterion) {
    let signer = signer();
    let order = sample_order();
    c.bench_function("sign_order", |b| {
        b.iter(|| {
            let sig = signer.sign_order(black_box(&order), false).unwrap();
            black_box(sig);
        });
    });
}

fn bench_sign_order_neg_risk(c: &mut Criterion) {
    let signer = signer();
    let order = sample_order();
    c.bench_function("sign_order_neg_risk", |b| {
        b.iter(|| {
            let sig = signer.sign_order(black_box(&order), true).unwrap();
            black_box(sig);
        });
    });
}

fn bench_order_hash(c: &mut Criterion) {
    let order = sample_order();
    c.bench_function("order_hash", |b| {
        b.iter(|| {
            let h = order_hash(black_box(&order), false).unwrap();
            black_box(h);
        });
    });
}

fn bench_signer_construction(c: &mut Criterion) {
    let key = EvmPrivateKey::new(TEST_KEY).unwrap();
    c.bench_function("signer_construction", |b| {
        b.iter(|| {
            let s = OrderSigner::new(black_box(&key)).unwrap();
            black_box(s);
        });
    });
}

fn bench_sign_clob_auth(c: &mut Criterion) {
    let key = EvmPrivateKey::new(TEST_KEY).unwrap();
    let timestamp = "1713398400";
    let nonce: u64 = 0;
    c.bench_function("sign_clob_auth", |b| {
        b.iter(|| {
            let (addr, sig) = sign_clob_auth(black_box(&key), black_box(timestamp), nonce).unwrap();
            black_box((addr, sig));
        });
    });
}

fn bench_hmac_sign(c: &mut Criterion) {
    let credential = Credential::new(
        "00000000-0000-0000-0000-000000000001",
        TEST_API_SECRET,
        "test-passphrase".to_string(),
    )
    .unwrap();
    let body = r#"{"order":{"salt":123},"owner":"00000000-0000-0000-0000-000000000001","orderType":"GTC"}"#;
    c.bench_function("hmac_l2_sign", |b| {
        b.iter(|| {
            let sig = credential.sign(
                black_box("1713398400"),
                black_box("POST"),
                black_box("/order"),
                black_box(body),
            );
            black_box(sig);
        });
    });
}

criterion_group!(
    benches,
    bench_sign_order,
    bench_sign_order_neg_risk,
    bench_order_hash,
    bench_signer_construction,
    bench_sign_clob_auth,
    bench_hmac_sign,
);
criterion_main!(benches);
