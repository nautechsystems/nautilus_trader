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

//! Canonical exec pipeline benches.
//!
//! `exec_pipeline`: strategy intent (limit/market submit / cancel) ->
//! per-request JSON body + L2 HMAC-SHA256 signature, the variable-cost
//! portion of every authenticated CLOB call. Covers maker/taker amount math,
//! EIP-712 order signing (submits only), JSON body serialization, and the
//! HMAC body signature `auth_headers` attaches via `Credential::sign`.
//!
//! The fixed-cost work `auth_headers` does around the signature (formatting a
//! timestamp string and constructing the five `POLY_*` header entries) is
//! omitted: it's constant overhead unrelated to the regressions these benches
//! are meant to catch. Polymarket has no in-place modify on the CLOB
//! (cancel-replace is two independent ops), so there's no `modify` row here.

mod common;

use std::hint::black_box;

use common::{API_KEY, YES_TOKEN_ID, bench_credential};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_polymarket::{
    common::{
        credential::{Credential, EvmPrivateKey},
        enums::{PolymarketOrderSide, PolymarketOrderType, SignatureType},
    },
    execution::order_builder::PolymarketOrderBuilder,
    http::models::PolymarketOrder,
    signing::eip712::OrderSigner,
};
use rust_decimal_macros::dec;
use serde::Serialize;

// Hardhat account #0; used in upstream EIP-712 vector tests for the crate.
const TEST_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const MAKER_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const TICK_DECIMALS: u32 = 2;
// Matches the value `auth_headers` builds for L2 signing: the request
// timestamp is unix-seconds as a string. Pinned so per-iteration variance
// doesn't bleed into the HMAC step.
const HMAC_TIMESTAMP: &str = "1713398400";
const POST_ORDER_PATH: &str = "/order";
const CANCEL_ORDER_PATH: &str = "/order";

fn order_builder() -> PolymarketOrderBuilder {
    let key = EvmPrivateKey::new(TEST_KEY).unwrap();
    let signer = OrderSigner::new(&key).unwrap();
    PolymarketOrderBuilder::new(
        signer,
        MAKER_ADDRESS.to_string(),
        MAKER_ADDRESS.to_string(),
        SignatureType::Eoa,
    )
}

// Mirrors `PostOrderBody` from `http::clob` (which is private to the crate) so
// the bench can measure the same wire-shape serialization without taking a
// dependency on a non-public type.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PostOrderBody<'a> {
    order: &'a PolymarketOrder,
    owner: &'a str,
    order_type: PolymarketOrderType,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    post_only: bool,
}

#[derive(Serialize)]
struct CancelOrderBody<'a> {
    #[serde(rename = "orderID")]
    order_id: &'a str,
}

fn serialize_post_order(
    order: &PolymarketOrder,
    order_type: PolymarketOrderType,
    post_only: bool,
) -> Vec<u8> {
    let body = PostOrderBody {
        order,
        owner: API_KEY,
        order_type,
        post_only,
    };
    serde_json::to_vec(&body).unwrap()
}

fn hmac_sign(credential: &Credential, method: &str, path: &str, body: &[u8]) -> String {
    let body_str = std::str::from_utf8(body).expect("body is valid UTF-8 JSON");
    credential.sign(HMAC_TIMESTAMP, method, path, body_str)
}

fn bench_submit_limit(c: &mut Criterion) {
    let builder = order_builder();
    let credential = bench_credential();

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_limit", |b| {
        b.iter(|| {
            let order = builder
                .build_limit_order(
                    YES_TOKEN_ID,
                    PolymarketOrderSide::Buy,
                    dec!(0.50),
                    dec!(100),
                    "0",
                    false,
                    TICK_DECIMALS,
                )
                .unwrap();
            let bytes = serialize_post_order(black_box(&order), PolymarketOrderType::GTC, false);
            let sig = hmac_sign(&credential, "POST", POST_ORDER_PATH, &bytes);
            black_box((bytes, sig));
        });
    });
    group.finish();
}

fn bench_submit_market(c: &mut Criterion) {
    let builder = order_builder();
    let credential = bench_credential();

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_market", |b| {
        b.iter(|| {
            // Market BUY: amount is pUSD to spend.
            let order = builder
                .build_market_order(
                    YES_TOKEN_ID,
                    PolymarketOrderSide::Buy,
                    dec!(0.50),
                    dec!(50),
                    false,
                    TICK_DECIMALS,
                )
                .unwrap();
            let bytes = serialize_post_order(black_box(&order), PolymarketOrderType::FOK, false);
            let sig = hmac_sign(&credential, "POST", POST_ORDER_PATH, &bytes);
            black_box((bytes, sig));
        });
    });
    group.finish();
}

fn bench_submit_limit_neg_risk(c: &mut Criterion) {
    // Pinned separately because the `verifyingContract` selection sits inside
    // the EIP-712 hash; a regression in the neg-risk path is silently masked
    // by the standard-CTF case.
    let builder = order_builder();
    let credential = bench_credential();

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_limit_neg_risk", |b| {
        b.iter(|| {
            let order = builder
                .build_limit_order(
                    YES_TOKEN_ID,
                    PolymarketOrderSide::Sell,
                    dec!(0.50),
                    dec!(100),
                    "0",
                    true,
                    TICK_DECIMALS,
                )
                .unwrap();
            let bytes = serialize_post_order(black_box(&order), PolymarketOrderType::GTC, false);
            let sig = hmac_sign(&credential, "POST", POST_ORDER_PATH, &bytes);
            black_box((bytes, sig));
        });
    });
    group.finish();
}

fn bench_cancel(c: &mut Criterion) {
    let order_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let credential = bench_credential();

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("cancel", |b| {
        b.iter(|| {
            let body = CancelOrderBody {
                order_id: black_box(order_id),
            };
            let bytes = serde_json::to_vec(&body).unwrap();
            let sig = hmac_sign(&credential, "DELETE", CANCEL_ORDER_PATH, &bytes);
            black_box((bytes, sig));
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_submit_limit,
    bench_submit_market,
    bench_submit_limit_neg_risk,
    bench_cancel,
);
criterion_main!(benches);
