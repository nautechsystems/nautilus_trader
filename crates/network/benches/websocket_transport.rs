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

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{DuplexStream, duplex};
use tokio_tungstenite::{
    WebSocketStream as TokioWebSocketStream,
    tungstenite::{Message as TokioMessage, protocol::Role},
};

const MESSAGE_COUNT: usize = 10_000;
const DUPLEX_BUFFER_SIZE: usize = 1 << 20;
const PAYLOAD_SIZES: [usize; 3] = [64, 512, 4_096];

type BenchResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn bench_receive_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("websocket_transport/receive_text");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    for payload_size in PAYLOAD_SIZES {
        group.throughput(Throughput::Bytes((payload_size * MESSAGE_COUNT) as u64));
        group.bench_with_input(
            BenchmarkId::new("tokio_tungstenite", payload_size),
            &payload_size,
            |b, &payload_size| {
                b.iter(|| {
                    rt.block_on(async {
                        tokio_tungstenite_receive_text(payload_size).await.unwrap();
                    });
                });
            },
        );
    }

    group.finish();
}

fn bench_send_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("websocket_transport/send_text");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    for payload_size in PAYLOAD_SIZES {
        group.throughput(Throughput::Bytes((payload_size * MESSAGE_COUNT) as u64));
        group.bench_with_input(
            BenchmarkId::new("tokio_tungstenite", payload_size),
            &payload_size,
            |b, &payload_size| {
                b.iter(|| {
                    rt.block_on(async {
                        tokio_tungstenite_send_text(payload_size).await.unwrap();
                    });
                });
            },
        );
    }

    group.finish();
}

fn bench_roundtrip_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("websocket_transport/roundtrip_text");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    for payload_size in PAYLOAD_SIZES {
        group.throughput(Throughput::Bytes((payload_size * MESSAGE_COUNT) as u64));
        group.bench_with_input(
            BenchmarkId::new("tokio_tungstenite", payload_size),
            &payload_size,
            |b, &payload_size| {
                b.iter(|| {
                    rt.block_on(async {
                        tokio_tungstenite_roundtrip_text(payload_size)
                            .await
                            .unwrap();
                    });
                });
            },
        );
    }

    group.finish();
}

async fn tokio_tungstenite_receive_text(payload_size: usize) -> BenchResult {
    let (client_io, server_io) = duplex(DUPLEX_BUFFER_SIZE);
    let mut client = tokio_tungstenite_client(client_io).await;
    let mut server = tokio_tungstenite_server(server_io).await;
    let payload = "x".repeat(payload_size);

    let server_task = tokio::spawn(async move {
        for _ in 0..MESSAGE_COUNT {
            server
                .send(TokioMessage::Text(payload.clone().into()))
                .await?;
        }

        BenchResult::Ok(())
    });

    for _ in 0..MESSAGE_COUNT {
        let message = client.next().await.transpose()?.unwrap();
        match message {
            TokioMessage::Text(data) => {
                black_box(data);
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    server_task.await??;
    Ok(())
}

async fn tokio_tungstenite_send_text(payload_size: usize) -> BenchResult {
    let (client_io, server_io) = duplex(DUPLEX_BUFFER_SIZE);
    let mut client = tokio_tungstenite_client(client_io).await;
    let mut server = tokio_tungstenite_server(server_io).await;
    let payload = "x".repeat(payload_size);

    let server_task = tokio::spawn(async move {
        for _ in 0..MESSAGE_COUNT {
            let message = server.next().await.transpose()?.unwrap();
            match message {
                TokioMessage::Text(data) => {
                    black_box(data);
                }
                other => panic!("unexpected message: {other:?}"),
            }
        }

        BenchResult::Ok(())
    });

    for _ in 0..MESSAGE_COUNT {
        client
            .send(TokioMessage::Text(payload.clone().into()))
            .await?;
    }

    server_task.await??;
    Ok(())
}

async fn tokio_tungstenite_roundtrip_text(payload_size: usize) -> BenchResult {
    let (client_io, server_io) = duplex(DUPLEX_BUFFER_SIZE);
    let mut client = tokio_tungstenite_client(client_io).await;
    let mut server = tokio_tungstenite_server(server_io).await;
    let payload = "x".repeat(payload_size);

    let server_task = tokio::spawn(async move {
        for _ in 0..MESSAGE_COUNT {
            let message = server.next().await.transpose()?.unwrap();
            server.send(message).await?;
        }

        BenchResult::Ok(())
    });

    for _ in 0..MESSAGE_COUNT {
        client
            .send(TokioMessage::Text(payload.clone().into()))
            .await?;
        let message = client.next().await.transpose()?.unwrap();
        match message {
            TokioMessage::Text(data) => {
                black_box(data);
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    server_task.await??;
    Ok(())
}

async fn tokio_tungstenite_client(stream: DuplexStream) -> TokioWebSocketStream<DuplexStream> {
    TokioWebSocketStream::from_raw_socket(stream, Role::Client, None).await
}

async fn tokio_tungstenite_server(stream: DuplexStream) -> TokioWebSocketStream<DuplexStream> {
    TokioWebSocketStream::from_raw_socket(stream, Role::Server, None).await
}

criterion_group!(
    benches,
    bench_receive_text,
    bench_send_text,
    bench_roundtrip_text,
);
criterion_main!(benches);
