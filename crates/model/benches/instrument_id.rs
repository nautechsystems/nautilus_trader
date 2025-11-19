use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::{hint::black_box, str::FromStr};
use nautilus_model::identifiers::instrument_id::InstrumentId;

fn bench_instrument_id_parse(c: &mut Criterion) {
    let samples = vec![
        "ETHUSDT.BINANCE",
        "BTC-PERP.OKX",
        "AAPL.NASDAQ",
        "ETH/USDT.BINANCE",
    ];

    c.bench_function("instrument_id_from_str", |b| {
        b.iter_batched(
            || samples.clone(),
            |ids| {
                for s in ids.iter() {
                    let id = InstrumentId::from_str(s).unwrap();
                    black_box(id);
                }
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("instrument_id_to_string", |b| {
        let ids: Vec<_> = samples
            .iter()
            .map(|s| InstrumentId::from_str(s).unwrap())
            .collect();
        b.iter(|| {
            for id in ids.iter() {
                let s = id.to_string();
                black_box(s);
            }
        });
    });
}

criterion_group!(benches, bench_instrument_id_parse);
criterion_main!(benches);
