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

//! Component-level Databento microbenches.
//!
//! `record_decode` measures already-decoded Databento records -> Nautilus
//! domain values. `record_dispatch` measures the generic `RecordRef` routing
//! path used by the loader and live feed handler. `atom` isolates primitive
//! conversion and identifier construction costs that can dominate tiny records.

mod common;

use std::{ffi::c_char, hint::black_box};

use criterion::{Criterion, criterion_group, criterion_main};
use databento::dbn::{self, Record, record::RecordHeader};
use nautilus_databento::decode::{
    decode_bbo_msg, decode_cmbp1_msg, decode_imbalance_msg, decode_mbo_msg, decode_mbp1_msg,
    decode_mbp10_msg, decode_ohlcv_msg, decode_price_increment, decode_price_or_undef,
    decode_quantity, decode_record, decode_statistics_msg, decode_status_msg, decode_tbbo_msg,
    decode_trade_msg, precision_from_raw,
};
use nautilus_model::identifiers::TradeId;

fn bench_record_decode(c: &mut Criterion) {
    let instrument_id = common::instrument_id();
    let price_precision = common::PRICE_PRECISION;

    let mbo_delta = common::first_record::<dbn::MboMsg>("test_data.mbo.dbn.zst");
    let mut mbo_trade = mbo_delta.clone();
    mbo_trade.action = b'T' as c_char;
    mbo_trade.side = b'A' as c_char;
    mbo_trade.size = 5;

    let mbp1_quote = common::first_record::<dbn::Mbp1Msg>("test_data.mbp-1.dbn.zst");
    let mut mbp1_trade = mbp1_quote.clone();
    mbp1_trade.action = b'T' as c_char;
    mbp1_trade.side = b'B' as c_char;
    mbp1_trade.size = 5;

    let mbp10 = common::first_record::<dbn::Mbp10Msg>("test_data.mbp-10.dbn.zst");
    let bbo = common::first_record::<dbn::Bbo1SMsg>("test_data.bbo-1s.dbn.zst");
    let cmbp_quote = common::first_record::<dbn::Cmbp1Msg>("test_data.cmbp-1.dbn.zst");
    let mut cmbp_trade = cmbp_quote.clone();
    cmbp_trade.action = b'T' as c_char;
    cmbp_trade.side = b'B' as c_char;
    cmbp_trade.size = 5;

    let tbbo = common::first_record::<dbn::TbboMsg>("test_data.tbbo.dbn.zst");
    let ohlcv = common::first_record::<dbn::OhlcvMsg>("test_data.ohlcv-1s.dbn.zst");
    let status = common::first_record::<dbn::StatusMsg>("test_data.status.dbn.zst");
    let imbalance = common::first_record::<dbn::ImbalanceMsg>("test_data.imbalance.dbn.zst");
    let statistics = common::first_record::<dbn::StatMsg>("test_data.statistics.dbn.zst");

    let mut group = c.benchmark_group("record_decode");

    group.bench_function("mbo_delta", |b| {
        b.iter(|| {
            let delta = decode_mbo_msg(
                black_box(&mbo_delta),
                instrument_id,
                price_precision,
                None,
                false,
            )
            .unwrap();
            black_box(delta);
        });
    });

    group.bench_function("mbo_trade", |b| {
        b.iter(|| {
            let trade = decode_mbo_msg(
                black_box(&mbo_trade),
                instrument_id,
                price_precision,
                None,
                true,
            )
            .unwrap();
            black_box(trade);
        });
    });

    group.bench_function("trade", |b| {
        let msg = common::first_record::<dbn::TradeMsg>("test_data.trades.dbn.zst");
        b.iter(|| {
            let trade =
                decode_trade_msg(black_box(&msg), instrument_id, price_precision, None).unwrap();
            black_box(trade);
        });
    });

    group.bench_function("mbp1_quote", |b| {
        b.iter(|| {
            let quote = decode_mbp1_msg(
                black_box(&mbp1_quote),
                instrument_id,
                price_precision,
                None,
                false,
            )
            .unwrap();
            black_box(quote);
        });
    });

    group.bench_function("mbp1_trade", |b| {
        b.iter(|| {
            let output = decode_mbp1_msg(
                black_box(&mbp1_trade),
                instrument_id,
                price_precision,
                None,
                true,
            )
            .unwrap();
            black_box(output);
        });
    });

    group.bench_function("mbp10_depth", |b| {
        b.iter(|| {
            let depth =
                decode_mbp10_msg(black_box(&mbp10), instrument_id, price_precision, None).unwrap();
            black_box(depth);
        });
    });

    group.bench_function("bbo_quote", |b| {
        b.iter(|| {
            let quote =
                decode_bbo_msg(black_box(&bbo), instrument_id, price_precision, None).unwrap();
            black_box(quote);
        });
    });

    group.bench_function("cmbp_quote", |b| {
        b.iter(|| {
            let output = decode_cmbp1_msg(
                black_box(&cmbp_quote),
                instrument_id,
                price_precision,
                None,
                false,
            )
            .unwrap();
            black_box(output);
        });
    });

    group.bench_function("cmbp_trade", |b| {
        b.iter(|| {
            let output = decode_cmbp1_msg(
                black_box(&cmbp_trade),
                instrument_id,
                price_precision,
                None,
                true,
            )
            .unwrap();
            black_box(output);
        });
    });

    group.bench_function("tbbo", |b| {
        b.iter(|| {
            let output =
                decode_tbbo_msg(black_box(&tbbo), instrument_id, price_precision, None).unwrap();
            black_box(output);
        });
    });

    group.bench_function("ohlcv", |b| {
        b.iter(|| {
            let bar = decode_ohlcv_msg(
                black_box(&ohlcv),
                instrument_id,
                price_precision,
                None,
                true,
            )
            .unwrap();
            black_box(bar);
        });
    });

    group.bench_function("status", |b| {
        b.iter(|| {
            let item = decode_status_msg(black_box(&status), instrument_id, None).unwrap();
            black_box(item);
        });
    });

    group.bench_function("imbalance", |b| {
        b.iter(|| {
            let item =
                decode_imbalance_msg(black_box(&imbalance), instrument_id, price_precision, None)
                    .unwrap();
            black_box(item);
        });
    });

    group.bench_function("statistics", |b| {
        b.iter(|| {
            let item =
                decode_statistics_msg(black_box(&statistics), instrument_id, price_precision, None)
                    .unwrap();
            black_box(item);
        });
    });

    group.finish();
}

fn bench_record_dispatch(c: &mut Criterion) {
    let instrument_id = common::instrument_id();
    let price_precision = common::PRICE_PRECISION;
    let trade = common::first_record::<dbn::TradeMsg>("test_data.trades.dbn.zst");
    let mbp10 = common::first_record::<dbn::Mbp10Msg>("test_data.mbp-10.dbn.zst");
    let ohlcv = common::first_record::<dbn::OhlcvMsg>("test_data.ohlcv-1s.dbn.zst");

    let mut group = c.benchmark_group("record_dispatch");

    group.bench_function("trade", |b| {
        b.iter(|| {
            let record = dbn::RecordRef::from(black_box(&trade));
            let output =
                decode_record(&record, instrument_id, price_precision, None, false, true).unwrap();
            black_box(output);
        });
    });

    group.bench_function("mbp10_depth", |b| {
        b.iter(|| {
            let record = dbn::RecordRef::from(black_box(&mbp10));
            let output =
                decode_record(&record, instrument_id, price_precision, None, false, true).unwrap();
            black_box(output);
        });
    });

    group.bench_function("ohlcv", |b| {
        b.iter(|| {
            let record = dbn::RecordRef::from(black_box(&ohlcv));
            let output =
                decode_record(&record, instrument_id, price_precision, None, false, true).unwrap();
            black_box(output);
        });
    });

    group.finish();
}

fn bench_atoms(c: &mut Criterion) {
    c.bench_function("atom/decode_price_or_undef", |b| {
        b.iter(|| {
            let price =
                decode_price_or_undef(black_box(3_720_250_000_000), common::PRICE_PRECISION);
            black_box(price);
        });
    });

    c.bench_function("atom/decode_price_increment", |b| {
        b.iter(|| {
            let price = decode_price_increment(black_box(10_000_000), common::PRICE_PRECISION);
            black_box(price);
        });
    });

    c.bench_function("atom/decode_quantity", |b| {
        b.iter(|| {
            let qty = decode_quantity(black_box(125));
            black_box(qty);
        });
    });

    c.bench_function("atom/precision_from_raw", |b| {
        b.iter(|| {
            let precision = precision_from_raw(black_box(3_906_250));
            black_box(precision);
        });
    });

    c.bench_function("atom/trade_id_from_sequence", |b| {
        b.iter(|| {
            let id = TradeId::new(itoa::Buffer::new().format(black_box(123_456_789u32)));
            black_box(id);
        });
    });

    c.bench_function("atom/record_header_ref", |b| {
        let msg = common::first_record::<dbn::TradeMsg>("test_data.trades.dbn.zst");
        b.iter(|| {
            let record = dbn::RecordRef::from(black_box(&msg));
            let header: &RecordHeader = record.header();
            black_box(header);
        });
    });
}

criterion_group!(
    benches,
    bench_record_decode,
    bench_record_dispatch,
    bench_atoms
);
criterion_main!(benches);
