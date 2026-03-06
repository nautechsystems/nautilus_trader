//! Benchmarks for Ax WebSocket message deserialization.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_architect_ax::websocket::{
    messages::{AxMdBookL1, AxMdBookL2, AxMdBookL3, AxMdTrade},
    parse::parse_md_message,
};

const MD_BOOK_L1: &str = include_str!("../test_data/ws_md_book_l1_captured.json");
const MD_BOOK_L2: &str = include_str!("../test_data/ws_md_book_l2_captured.json");
const MD_BOOK_L3: &str = include_str!("../test_data/ws_md_book_l3_captured.json");
const MD_TRADE: &str = include_str!("../test_data/ws_md_trade_captured.json");
const MD_TICKER: &str = include_str!("../test_data/ws_md_ticker_captured.json");
const MD_HEARTBEAT: &str = include_str!("../test_data/ws_md_heartbeat_captured.json");

fn bench_md_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("ax_md_parse");

    group.bench_function("book_l1", |b| {
        b.iter(|| parse_md_message(black_box(MD_BOOK_L1)).unwrap());
    });

    group.bench_function("book_l2", |b| {
        b.iter(|| parse_md_message(black_box(MD_BOOK_L2)).unwrap());
    });

    group.bench_function("book_l3", |b| {
        b.iter(|| parse_md_message(black_box(MD_BOOK_L3)).unwrap());
    });

    group.bench_function("trade", |b| {
        b.iter(|| parse_md_message(black_box(MD_TRADE)).unwrap());
    });

    group.bench_function("ticker", |b| {
        b.iter(|| parse_md_message(black_box(MD_TICKER)).unwrap());
    });

    group.bench_function("heartbeat", |b| {
        b.iter(|| parse_md_message(black_box(MD_HEARTBEAT)).unwrap());
    });

    group.finish();
}

fn bench_direct_struct_deser(c: &mut Criterion) {
    let mut group = c.benchmark_group("ax_direct_struct_deser");

    group.bench_function("book_l1", |b| {
        b.iter(|| serde_json::from_str::<AxMdBookL1>(black_box(MD_BOOK_L1)).unwrap());
    });

    group.bench_function("book_l2", |b| {
        b.iter(|| serde_json::from_str::<AxMdBookL2>(black_box(MD_BOOK_L2)).unwrap());
    });

    group.bench_function("book_l3", |b| {
        b.iter(|| serde_json::from_str::<AxMdBookL3>(black_box(MD_BOOK_L3)).unwrap());
    });

    group.bench_function("trade", |b| {
        b.iter(|| serde_json::from_str::<AxMdTrade>(black_box(MD_TRADE)).unwrap());
    });

    group.finish();
}

criterion_group!(benches, bench_md_parse, bench_direct_struct_deser);
criterion_main!(benches);
