// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fs;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use nautilus_model::data::{QuoteTick, TradeTick};
use nautilus_persistence::backend::session::{DataBackendSession, QueryResult};

fn single_stream_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_stream");
    group.sample_size(10);
    let chunk_size = 5000;
    // about 10 M records
    // let file_path = "../../bench_data/quotes_0005.parquet";
    let file_path = "../../bench_data/quotes_0005_high_precision.parquet";

    group.bench_function("persistence v2", |b| {
        b.iter_batched_ref(
            || {
                let mut catalog = DataBackendSession::new(chunk_size);
                catalog
                    .add_file::<QuoteTick>("quote_tick", file_path, None)
                    .unwrap();
                catalog.get_query_result()
            },
            |query_result: &mut QueryResult| {
                let count: usize = query_result.count();
                assert_eq!(count, 9_689_614);
            },
            BatchSize::SmallInput,
        );
    });
}

fn multi_stream_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_stream");
    group.sample_size(10);
    let chunk_size = 5000;
    // about 72 M records, with streams split across multiple files
    let dir_path = "../../bench_data/multi_stream_data";

    group.bench_function("persistence v2", |b| {
        b.iter_batched_ref(
            || {
                let mut catalog = DataBackendSession::new(chunk_size);

                for entry in fs::read_dir(dir_path).expect("No such directory") {
                    let entry = entry.expect("Failed to read directory");
                    let path = entry.path();

                    if path.is_file() && path.extension().unwrap() == "parquet" {
                        let file_name = path.file_stem().unwrap().to_str().unwrap();

                        if file_name.contains("quotes") {
                            catalog
                                .add_file::<QuoteTick>(file_name, path.to_str().unwrap(), None)
                                .unwrap();
                        } else if file_name.contains("trades") {
                            catalog
                                .add_file::<TradeTick>(file_name, path.to_str().unwrap(), None)
                                .unwrap();
                        }
                    }
                }

                catalog.get_query_result()
            },
            |query_result: &mut QueryResult| {
                let count: usize = query_result.count();
                assert_eq!(count, 72_536_038);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, single_stream_bench, multi_stream_bench);
criterion_main!(benches);
