// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashMap, time::Instant};

use criterion::{criterion_group, criterion_main, Criterion};
use hyper::Method;
use nautilus_network::http::HttpClient;

const CONCURRENCY: usize = 256;
const TOTAL: usize = 1_000_000;

fn http_client_benchmark(c: &mut Criterion) {
    let client = HttpClient::new(Vec::new());
    let mut reqs = Vec::new();

    c.bench_function("http_client", |b| {
        b.iter(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let start_time = Instant::now();

                for _ in 0..(TOTAL / CONCURRENCY) {
                    for _ in 0..CONCURRENCY {
                        reqs.push(client.send_request(
                            Method::GET,
                            "http://127.0.0.1:3000".to_string(),
                            HashMap::new(),
                            None,
                        ));
                    }

                    let resp = futures::future::join_all(reqs.drain(0..)).await;
                    assert!(resp.iter().all(|res| if let Ok(resp) = res {
                        resp.status == 200
                    } else {
                        false
                    }));
                }

                let end_time = Instant::now(); // End timing here
                let duration = end_time.duration_since(start_time);

                duration
            })
        })
    });
}

criterion_group!(benches, http_client_benchmark);
criterion_main!(benches);
