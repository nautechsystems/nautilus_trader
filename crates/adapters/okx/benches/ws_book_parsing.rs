use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::identifiers::InstrumentId;
use nautilus_okx::websocket::{messages::OKXWebSocketEvent, parse::parse_book_msg_vec};
use serde_json::from_str;

const BOOK_SNAPSHOT: &str = include_str!("../test_data/ws_books_snapshot.json");
const BOOK_UPDATE: &str = include_str!("../test_data/ws_books_update.json");

fn bench_book_snapshot(c: &mut Criterion) {
    c.bench_function("parse_book_snapshot", |b| {
        b.iter_batched(
            || from_str::<OKXWebSocketEvent>(BOOK_SNAPSHOT).expect("snapshot event"),
            |event| match event {
                OKXWebSocketEvent::BookData { data, action, .. } => {
                    let instrument_id = InstrumentId::from("BTC-USDT.OKX");
                    let payload = parse_book_msg_vec(
                        data,
                        &instrument_id,
                        2,
                        1,
                        action,
                        UnixNanos::default(),
                    )
                    .expect("snapshot parsing");
                    black_box(payload);
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_book_update(c: &mut Criterion) {
    c.bench_function("parse_book_update", |b| {
        b.iter_batched(
            || from_str::<OKXWebSocketEvent>(BOOK_UPDATE).expect("update event"),
            |event| match event {
                OKXWebSocketEvent::BookData { data, action, .. } => {
                    let instrument_id = InstrumentId::from("BTC-USDT.OKX");
                    let payload = parse_book_msg_vec(
                        data,
                        &instrument_id,
                        2,
                        1,
                        action,
                        UnixNanos::default(),
                    )
                    .expect("update parsing");
                    black_box(payload);
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        );
    });
}

fn benches(c: &mut Criterion) {
    bench_book_snapshot(c);
    bench_book_update(c);
}

criterion_group!(okx_books, benches);
criterion_main!(okx_books);
