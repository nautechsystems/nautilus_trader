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

//! HMAC-SHA256 signing benches for the OKX HTTP auth path. WS order ops do not
//! sign per-message; this isolates the HTTP-only cost so callers can compare
//! the per-request signing overhead against the JSON serialize numbers in
//! `exec.rs`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_okx::common::credential::Credential;

const API_KEY: &str = "985d5b66-57ce-40fb-b714-afc0b9787083";
const API_SECRET: &str = "chNOOS4KvNXR_Xq4k4c9qsfoKWvnDecLATCRlcBwyKDYnWgO";
const API_PASSPHRASE: &str = "1234567890";

const TIMESTAMP: &str = "2025-01-20T10:30:45.123Z";
const METHOD: &str = "POST";
const PATH_ORDER: &str = "/api/v5/trade/order";
const PATH_ORDER_ALGO: &str = "/api/v5/trade/order-algo";

const BODY_LIMIT: &[u8] = br#"{"instId":"BTC-USDT-SWAP","tdMode":"cross","ccy":"USDT","clOrdId":"O-BENCH-LIM","tag":"nautilus","side":"buy","ordType":"limit","sz":"0.001","px":"92572.0","reduceOnly":false}"#;
const BODY_ALGO: &[u8] = br#"[{"instId":"BTC-USDT-SWAP","tdMode":"cross","side":"sell","ordType":"trigger","sz":"0.001","algoClOrdId":"O-BENCH-STP","triggerPx":"90000.0","orderPx":"-1","triggerPxType":"last","tag":"nautilus","reduceOnly":false}]"#;

fn credential() -> Credential {
    Credential::new(
        API_KEY.to_string(),
        API_SECRET.to_string(),
        API_PASSPHRASE.to_string(),
    )
}

fn bench_sign_order(c: &mut Criterion) {
    let cred = credential();
    c.bench_function("sign_order", |b| {
        b.iter(|| {
            let sig = cred.sign_bytes(
                black_box(TIMESTAMP),
                black_box(METHOD),
                black_box(PATH_ORDER),
                Some(black_box(BODY_LIMIT)),
            );
            black_box(sig);
        });
    });
}

fn bench_sign_algo(c: &mut Criterion) {
    let cred = credential();
    c.bench_function("sign_order_algo", |b| {
        b.iter(|| {
            let sig = cred.sign_bytes(
                black_box(TIMESTAMP),
                black_box(METHOD),
                black_box(PATH_ORDER_ALGO),
                Some(black_box(BODY_ALGO)),
            );
            black_box(sig);
        });
    });
}

fn bench_sign_no_body(c: &mut Criterion) {
    let cred = credential();
    c.bench_function("sign_get_no_body", |b| {
        b.iter(|| {
            let sig = cred.sign_bytes(
                black_box(TIMESTAMP),
                black_box("GET"),
                black_box("/api/v5/account/balance"),
                None,
            );
            black_box(sig);
        });
    });
}

criterion_group!(
    benches,
    bench_sign_order,
    bench_sign_algo,
    bench_sign_no_body
);
criterion_main!(benches);
