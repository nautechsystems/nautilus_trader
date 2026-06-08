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

//! Signing benches for the Derive auth paths.
//!
//! `sign_trade_action` isolates the EIP-712 order signature (ABI encode +
//! keccak + secp256k1) that the order-submit path pays per order.
//! `rest_auth_headers` isolates the EIP-191 timestamp signature the HTTP read
//! path pays per request. The remaining benches break out the pieces:
//! signer construction (secp256k1 key expansion), ABI encoding of the trade
//! module data, and nonce allocation.

use std::{hint::black_box, str::FromStr};

use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{Address, B256, U256};
use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_derive::{
    common::{
        consts::{ACTION_TYPEHASH, domain_separator_for, trade_module_address_for},
        enums::DeriveEnvironment,
    },
    signing::{
        auth::build_rest_auth_headers_at,
        eip712::{ActionContext, SignedAction},
        encoding::{parse_address_const, parse_b256_const},
        modules::trade::TradeModuleData,
        nonce::NonceManager,
    },
};
use rust_decimal_macros::dec;

const SESSION_KEY: &str = "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
const WALLET: &str = "0x000000000000000000000000000000000000aaaa";
const SUBACCOUNT_ID: u64 = 30769;
const NONCE: u64 = 1_700_000_000_000_000;
const EXPIRY_SEC: i64 = 1_900_000_000; // far-future so the EIP-712 expiry guard passes
const NOW_MS: u64 = 1_700_000_000_000;

fn sample_trade() -> TradeModuleData {
    TradeModuleData {
        asset_address: "0x000000000000000000000000000000000000abcd"
            .parse()
            .unwrap(),
        sub_id: U256::from(42u64),
        limit_price: dec!(3500),
        amount: dec!(1),
        max_fee: dec!(1000),
        recipient_id: SUBACCOUNT_ID,
        is_bid: true,
    }
}

fn action_context(signer: Address, module: Address) -> ActionContext {
    ActionContext {
        subaccount_id: SUBACCOUNT_ID,
        nonce: NONCE,
        module_address: module,
        signature_expiry_sec: EXPIRY_SEC,
        owner: WALLET.parse().unwrap(),
        signer,
    }
}

fn domain() -> B256 {
    parse_b256_const(
        domain_separator_for(DeriveEnvironment::Mainnet),
        "domain_separator",
    )
    .unwrap()
}

fn typehash() -> B256 {
    parse_b256_const(ACTION_TYPEHASH, "action_typehash").unwrap()
}

fn module() -> Address {
    parse_address_const(
        trade_module_address_for(DeriveEnvironment::Mainnet),
        "trade_module_address",
    )
    .unwrap()
}

fn bench_sign_trade_action(c: &mut Criterion) {
    let signer = PrivateKeySigner::from_str(SESSION_KEY).unwrap();
    let trade = sample_trade();
    let module = module();
    let ctx = action_context(signer.address(), module);
    let domain = domain();
    let typehash = typehash();

    c.bench_function("sign_trade_action", |b| {
        b.iter(|| {
            let mut action = SignedAction::new(ctx.clone(), &trade, domain, typehash);
            let sig = action.sign(black_box(&signer)).unwrap();
            black_box(sig);
        });
    });
}

fn bench_signer_from_key(c: &mut Criterion) {
    c.bench_function("signer_from_key", |b| {
        b.iter(|| {
            let signer = PrivateKeySigner::from_str(black_box(SESSION_KEY)).unwrap();
            black_box(signer);
        });
    });
}

fn bench_abi_encode_trade(c: &mut Criterion) {
    let trade = sample_trade();

    c.bench_function("abi_encode_trade", |b| {
        b.iter(|| {
            let bytes = black_box(&trade).encode().unwrap();
            black_box(bytes);
        });
    });
}

fn bench_rest_auth_headers(c: &mut Criterion) {
    let signer = PrivateKeySigner::from_str(SESSION_KEY).unwrap();

    c.bench_function("rest_auth_headers", |b| {
        b.iter(|| {
            let headers = build_rest_auth_headers_at(black_box(WALLET), &signer, NOW_MS).unwrap();
            black_box(headers);
        });
    });
}

fn bench_nonce_next(c: &mut Criterion) {
    let manager = NonceManager::new();

    c.bench_function("nonce_next", |b| {
        b.iter(|| {
            let nonce = manager.next_nonce_at(black_box(WALLET), SUBACCOUNT_ID, NOW_MS);
            black_box(nonce);
        });
    });
}

criterion_group!(
    benches,
    bench_sign_trade_action,
    bench_signer_from_key,
    bench_abi_encode_trade,
    bench_rest_auth_headers,
    bench_nonce_next,
);
criterion_main!(benches);
