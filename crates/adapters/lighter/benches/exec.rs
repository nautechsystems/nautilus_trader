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
//! `exec_pipeline`: typed order intent -> signed wire bytes ready to POST.
//! For Lighter that means assembling a `CreateOrderTxInfo` /
//! `CancelOrderTxInfo` / `ModifyOrderTxInfo`, running [`sign_tx`] over it
//! (Poseidon2 body hash + optional attribute aggregation + Schnorr sign),
//! and rendering the wire JSON via [`TxInfoJson`]. This is the full critical
//! path between strategy command and the L2 sequencer.
//!
//! Dispatch (cloid translation, terminal eviction, mass-status fan-out)
//! lives behind `pub(crate)` surfaces on Lighter and is exercised indirectly
//! by the `inbound_pipeline/order_status` and `inbound_pipeline/fill_report`
//! benches in `data.rs`.

mod common;

use std::hint::black_box;

use common::{CHAIN_ID, cancel_order_tx, fixed_k, fixed_sk};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_lighter::{
    common::consts::LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX,
    signing::tx::{
        CancelOrderTxInfo, CreateOrderTxInfo, L2TxAttributes, ModifyOrderTxInfo, OrderInfo,
        TxContext, TxInfoJson, sign_tx,
    },
};

fn limit_create_order() -> CreateOrderTxInfo {
    CreateOrderTxInfo {
        context: TxContext {
            account_index: 12345,
            api_key_index: 5,
            nonce: 100,
            expired_at: 1_777_809_907_000,
        },
        order: OrderInfo {
            market_index: 1,
            client_order_index: 7,
            base_amount: 1_000,
            price: 9_257_200,
            is_ask: false,
            order_type: 0,    // Limit
            time_in_force: 1, // GoodTillTime
            reduce_only: false,
            trigger_price: 0,
            order_expiry: 1_780_000_000_000,
        },
        attributes: integrator_attrs(),
    }
}

fn market_create_order() -> CreateOrderTxInfo {
    let mut tx = limit_create_order();
    tx.context.nonce = 101;
    tx.order.client_order_index = 8;
    tx.order.order_type = 1; // Market
    tx.order.time_in_force = 0; // ImmediateOrCancel
    tx.order.order_expiry = 0;
    tx
}

fn stop_market_create_order() -> CreateOrderTxInfo {
    let mut tx = limit_create_order();
    tx.context.nonce = 102;
    tx.order.client_order_index = 9;
    tx.order.is_ask = true;
    tx.order.order_type = 2; // StopLoss
    tx.order.trigger_price = 9_000_000;
    tx
}

fn modify_order_tx() -> ModifyOrderTxInfo {
    ModifyOrderTxInfo {
        context: TxContext {
            account_index: 12345,
            api_key_index: 5,
            nonce: 110,
            expired_at: 1_777_809_907_000,
        },
        market_index: 1,
        index: 281_476_929_510_110,
        base_amount: 1_100,
        price: 9_257_300,
        trigger_price: 0,
        attributes: integrator_attrs(),
    }
}

fn cancel_tx_for_bench() -> CancelOrderTxInfo {
    let mut tx = cancel_order_tx();
    tx.context.nonce = 120;
    tx.index = 281_476_929_510_110;
    tx
}

fn bench_submit_market(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let tx = market_create_order();

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_market", |b| {
        b.iter(|| {
            let signed = sign_tx(black_box(&tx), CHAIN_ID, &sk, k);
            let json = TxInfoJson::create_order(&tx, &signed);
            black_box(json);
        });
    });
    group.finish();
}

fn bench_submit_limit(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let tx = limit_create_order();

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_limit", |b| {
        b.iter(|| {
            let signed = sign_tx(black_box(&tx), CHAIN_ID, &sk, k);
            let json = TxInfoJson::create_order(&tx, &signed);
            black_box(json);
        });
    });
    group.finish();
}

fn bench_submit_stop_market(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let tx = stop_market_create_order();

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_stop_market", |b| {
        b.iter(|| {
            let signed = sign_tx(black_box(&tx), CHAIN_ID, &sk, k);
            let json = TxInfoJson::create_order(&tx, &signed);
            black_box(json);
        });
    });
    group.finish();
}

fn bench_cancel(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let tx = cancel_tx_for_bench();

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("cancel", |b| {
        b.iter(|| {
            let signed = sign_tx(black_box(&tx), CHAIN_ID, &sk, k);
            let json = TxInfoJson::cancel_order(&tx, &signed);
            black_box(json);
        });
    });
    group.finish();
}

fn bench_modify(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let tx = modify_order_tx();

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("modify", |b| {
        b.iter(|| {
            let signed = sign_tx(black_box(&tx), CHAIN_ID, &sk, k);
            let json = TxInfoJson::modify_order(&tx, &signed);
            black_box(json);
        });
    });
    group.finish();
}

fn integrator_attrs() -> L2TxAttributes {
    L2TxAttributes {
        integrator_account_index: LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX,
        integrator_taker_fee: 0,
        integrator_maker_fee: 0,
        skip_nonce: 0,
    }
}

criterion_group!(
    benches,
    bench_submit_market,
    bench_submit_limit,
    bench_submit_stop_market,
    bench_cancel,
    bench_modify,
);
criterion_main!(benches);
