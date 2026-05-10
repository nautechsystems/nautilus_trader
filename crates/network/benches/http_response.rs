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

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use nautilus_network::http::InnerHttpClient;

fn bench_send_request_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("http/send_request_roundtrip");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let addr = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let router = axum::Router::new().route("/", axum::routing::get(|| async { "ok" }));
            axum::serve(listener, router).await.unwrap();
        });

        addr
    });

    let url = format!("http://{addr}/");
    let client = InnerHttpClient::default();

    for label in ["GET_no_params", "GET_no_headers"] {
        group.bench_function(BenchmarkId::new("method", label), |b| {
            b.iter(|| {
                rt.block_on(async {
                    black_box(
                        client
                            .send_request(
                                reqwest::Method::GET,
                                black_box(url.clone()),
                                None,
                                None,
                                None,
                                None,
                            )
                            .await
                            .unwrap(),
                    )
                })
            });
        });
    }

    group.finish();
}

fn bench_send_request_with_headers(c: &mut Criterion) {
    let mut group = c.benchmark_group("http/send_request_with_headers");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let addr = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let router = axum::Router::new().route("/", axum::routing::get(|| async { "ok" }));
            axum::serve(listener, router).await.unwrap();
        });

        addr
    });

    let url = format!("http://{addr}/");
    let client = InnerHttpClient::default();

    for num_headers in [0, 2, 5, 10] {
        let headers: std::collections::HashMap<String, String> = (0..num_headers)
            .map(|i| (format!("x-custom-header-{i}"), format!("value-{i}")))
            .collect();

        let headers_opt = if headers.is_empty() {
            None
        } else {
            Some(headers)
        };

        group.bench_with_input(
            BenchmarkId::new("headers", num_headers),
            &headers_opt,
            |b, headers| {
                b.iter(|| {
                    rt.block_on(async {
                        black_box(
                            client
                                .send_request(
                                    reqwest::Method::GET,
                                    black_box(url.clone()),
                                    None,
                                    headers.clone(),
                                    None,
                                    None,
                                )
                                .await
                                .unwrap(),
                        )
                    })
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_send_request_roundtrip,
    bench_send_request_with_headers,
);
criterion_main!(benches);
