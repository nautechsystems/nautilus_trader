use std::fs;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use nautilus_model::data::tick::{QuoteTick, TradeTick};
use nautilus_persistence::session::{DataBackendSession, QueryResult};
use pyo3_asyncio::tokio::get_runtime;

fn single_stream_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_stream");
    group.sample_size(10);
    let chunk_size = 5000;
    // about 10 M records
    let file_path = "../../bench_data/quotes_0005.parquet";

    group.bench_function("persistence v2", |b| {
        b.iter_batched(
            || {
                let rt = get_runtime();
                let mut catalog = DataBackendSession::new(chunk_size);
                rt.block_on(catalog.add_file_default_query::<QuoteTick>("quote_tick", file_path))
                    .unwrap();
                let _guard = rt.enter();
                catalog.get_query_result()
            },
            |query_result: QueryResult| {
                let rt = get_runtime();
                let _guard = rt.enter();
                let count: usize = query_result.map(|vec| vec.len()).sum();
                assert_eq!(count, 9689614);
            },
            BatchSize::SmallInput,
        )
    });
}

fn multi_stream_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_stream");
    group.sample_size(10);
    let chunk_size = 5000;
    // about 72 M records, with streams split across multiple files
    let dir_path = "../../bench_data/multi_stream_data";

    group.bench_function("persistence v2", |b| {
        b.iter_batched(
            || {
                let rt = get_runtime();
                let mut catalog = DataBackendSession::new(chunk_size);

                for entry in fs::read_dir(dir_path).expect("No such directory") {
                    let entry = entry.expect("Failed to read directory");
                    let path = entry.path();

                    if path.is_file() && path.extension().unwrap() == "parquet" {
                        let file_name = path.file_stem().unwrap().to_str().unwrap();

                        if file_name.contains("quotes") {
                            rt.block_on(catalog.add_file_default_query::<QuoteTick>(
                                file_name,
                                path.to_str().unwrap(),
                            ))
                            .unwrap();
                        } else if file_name.contains("trades") {
                            rt.block_on(catalog.add_file_default_query::<TradeTick>(
                                file_name,
                                path.to_str().unwrap(),
                            ))
                            .unwrap();
                        }
                    }
                }

                let _guard = rt.enter();
                catalog.get_query_result()
            },
            |query_result: QueryResult| {
                let rt = get_runtime();
                let _guard = rt.enter();
                let count: usize = query_result.map(|vec| vec.len()).sum();
                assert_eq!(count, 72536038);
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, single_stream_bench, multi_stream_bench);
criterion_main!(benches);
