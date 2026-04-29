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

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_hyperliquid::{
    common::credential::EvmPrivateKey,
    http::models::{
        HyperliquidExecAction, HyperliquidExecGrouping, HyperliquidExecLimitParams,
        HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest, HyperliquidExecTif,
    },
    signing::{HyperliquidActionType, HyperliquidEip712Signer, SignRequest, TimeNonce},
};
use rust_decimal_macros::dec;

const TEST_KEY: &str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";

fn make_signer() -> HyperliquidEip712Signer {
    let key = EvmPrivateKey::new(TEST_KEY).unwrap();
    HyperliquidEip712Signer::new(&key).unwrap()
}

fn make_order_action() -> HyperliquidExecAction {
    HyperliquidExecAction::Order {
        orders: vec![HyperliquidExecPlaceOrderRequest {
            asset: 3,
            is_buy: true,
            price: dec!(92572.0),
            size: dec!(0.001),
            reduce_only: false,
            kind: HyperliquidExecOrderKind::Limit {
                limit: HyperliquidExecLimitParams {
                    tif: HyperliquidExecTif::Gtc,
                },
            },
            cloid: None,
        }],
        grouping: HyperliquidExecGrouping::Na,
        builder: None,
    }
}

fn make_sign_request(action: &HyperliquidExecAction) -> SignRequest {
    let action_bytes = rmp_serde::to_vec_named(action).unwrap();
    let action_value = serde_json::to_value(action).unwrap();
    SignRequest {
        action: action_value,
        action_bytes: Some(action_bytes),
        time_nonce: TimeNonce::from_millis(1733833200000),
        action_type: HyperliquidActionType::L1,
        is_testnet: false,
        vault_address: None,
    }
}

fn bench_sign_l1_action(c: &mut Criterion) {
    let signer = make_signer();
    let action = make_order_action();
    let request = make_sign_request(&action);

    c.bench_function("sign_l1_action", |b| {
        b.iter(|| signer.sign(black_box(&request)));
    });
}

fn bench_signer_construction(c: &mut Criterion) {
    let key = EvmPrivateKey::new(TEST_KEY).unwrap();

    c.bench_function("signer_construction", |b| {
        b.iter(|| HyperliquidEip712Signer::new(black_box(&key)));
    });
}

fn bench_msgpack_serialize(c: &mut Criterion) {
    let action = make_order_action();

    c.bench_function("msgpack_serialize_action", |b| {
        b.iter(|| rmp_serde::to_vec_named(black_box(&action)));
    });
}

fn bench_json_serialize(c: &mut Criterion) {
    let action = make_order_action();

    c.bench_function("json_serialize_action", |b| {
        b.iter(|| serde_json::to_value(black_box(&action)));
    });
}

fn bench_sign_l1_with_vault(c: &mut Criterion) {
    let signer = make_signer();
    let action = make_order_action();
    let action_bytes = rmp_serde::to_vec_named(&action).unwrap();
    let action_value = serde_json::to_value(&action).unwrap();

    let request = SignRequest {
        action: action_value,
        action_bytes: Some(action_bytes),
        time_nonce: TimeNonce::from_millis(1733833200000),
        action_type: HyperliquidActionType::L1,
        is_testnet: false,
        vault_address: Some("0xAbCdEf0123456789AbCdEf0123456789AbCdEf01".to_string()),
    };

    c.bench_function("sign_l1_action_with_vault", |b| {
        b.iter(|| signer.sign(black_box(&request)));
    });
}

criterion_group!(
    benches,
    bench_sign_l1_action,
    bench_signer_construction,
    bench_msgpack_serialize,
    bench_json_serialize,
    bench_sign_l1_with_vault,
);
criterion_main!(benches);
