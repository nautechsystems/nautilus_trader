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

use std::{env, hint::black_box, time::Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::io::{DuplexStream, duplex};
use tokio_tungstenite::{
    WebSocketStream as TokioWebSocketStream,
    tungstenite::{Message as TokioMessage, protocol::Role},
};

const DEFAULT_MESSAGE_COUNT: usize = 50_000;
const DEFAULT_PAYLOAD_SIZES: [usize; 3] = [64, 512, 4_096];
const DUPLEX_BUFFER_SIZE: usize = 1 << 20;
const WARMUP_MESSAGES: usize = 1_000;

type BenchResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
struct LatencySummary {
    p50_ns: u64,
    p90_ns: u64,
    p99_ns: u64,
    p999_ns: u64,
    max_ns: u64,
    mean_ns: u64,
}

impl LatencySummary {
    fn from_samples(mut samples: Vec<u64>) -> Self {
        samples.sort_unstable();

        let total: u128 = samples.iter().map(|value| u128::from(*value)).sum();
        let mean_ns = (total / samples.len() as u128) as u64;

        Self {
            p50_ns: percentile(&samples, 0.50),
            p90_ns: percentile(&samples, 0.90),
            p99_ns: percentile(&samples, 0.99),
            p999_ns: percentile(&samples, 0.999),
            max_ns: samples[samples.len() - 1],
            mean_ns,
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> BenchResult {
    let message_count = message_count();
    let payload_sizes = payload_sizes();

    println!("messages: {message_count}");
    println!("payload_sizes: {payload_sizes:?}");
    println!();

    println!("roundtrip_text");
    print_header();

    for payload_size in payload_sizes.iter().copied() {
        let tokio_summary =
            tokio_tungstenite_roundtrip_text_latency(payload_size, message_count).await?;
        print_row(payload_size, "tokio_tungstenite", &tokio_summary);
    }

    println!();
    println!("one_way_binary_burst");
    print_header();

    for payload_size in payload_sizes {
        let tokio_summary =
            tokio_tungstenite_one_way_binary_latency(payload_size, message_count).await?;
        print_row(payload_size, "tokio_tungstenite", &tokio_summary);
    }

    Ok(())
}

async fn tokio_tungstenite_roundtrip_text_latency(
    payload_size: usize,
    message_count: usize,
) -> BenchResult<LatencySummary> {
    let (client_io, server_io) = duplex(DUPLEX_BUFFER_SIZE);
    let mut client = tokio_tungstenite_client(client_io).await;
    let mut server = tokio_tungstenite_server(server_io).await;
    let payload = "x".repeat(payload_size);
    let total_messages = message_count + WARMUP_MESSAGES;
    let mut samples = Vec::with_capacity(message_count);

    let server_task = tokio::spawn(async move {
        for _ in 0..total_messages {
            let message = server.next().await.transpose()?.unwrap();
            server.send(message).await?;
        }

        BenchResult::Ok(())
    });

    for index in 0..total_messages {
        let started = Instant::now();
        client
            .send(TokioMessage::Text(payload.clone().into()))
            .await?;

        let message = client.next().await.transpose()?.unwrap();
        let elapsed_ns = started.elapsed().as_nanos() as u64;

        match message {
            TokioMessage::Text(data) => {
                black_box(data);
            }
            other => panic!("unexpected message: {other:?}"),
        }

        if index >= WARMUP_MESSAGES {
            samples.push(elapsed_ns);
        }
    }

    server_task.await??;
    Ok(LatencySummary::from_samples(samples))
}

async fn tokio_tungstenite_one_way_binary_latency(
    payload_size: usize,
    message_count: usize,
) -> BenchResult<LatencySummary> {
    let (client_io, server_io) = duplex(DUPLEX_BUFFER_SIZE);
    let mut client = tokio_tungstenite_client(client_io).await;
    let mut server = tokio_tungstenite_server(server_io).await;
    let start = Instant::now();
    let total_messages = message_count + WARMUP_MESSAGES;
    let mut samples = Vec::with_capacity(message_count);

    let server_task = tokio::spawn(async move {
        for _ in 0..total_messages {
            let payload = timestamped_payload(payload_size, start);
            server.send(TokioMessage::Binary(payload.into())).await?;
        }

        BenchResult::Ok(())
    });

    for index in 0..total_messages {
        let message = client.next().await.transpose()?.unwrap();
        let received_ns = start.elapsed().as_nanos() as u64;

        match message {
            TokioMessage::Binary(data) => {
                let sent_ns = payload_timestamp_ns(data.as_ref());
                if index >= WARMUP_MESSAGES {
                    samples.push(received_ns.saturating_sub(sent_ns));
                }
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    server_task.await??;
    Ok(LatencySummary::from_samples(samples))
}

async fn tokio_tungstenite_client(stream: DuplexStream) -> TokioWebSocketStream<DuplexStream> {
    TokioWebSocketStream::from_raw_socket(stream, Role::Client, None).await
}

async fn tokio_tungstenite_server(stream: DuplexStream) -> TokioWebSocketStream<DuplexStream> {
    TokioWebSocketStream::from_raw_socket(stream, Role::Server, None).await
}

fn timestamped_payload(payload_size: usize, start: Instant) -> Vec<u8> {
    let mut payload = vec![b'x'; payload_size.max(size_of::<u64>())];
    let elapsed_ns = start.elapsed().as_nanos() as u64;
    payload[..size_of::<u64>()].copy_from_slice(&elapsed_ns.to_le_bytes());
    payload
}

fn payload_timestamp_ns(payload: &[u8]) -> u64 {
    let mut timestamp = [0_u8; size_of::<u64>()];
    timestamp.copy_from_slice(&payload[..size_of::<u64>()]);
    u64::from_le_bytes(timestamp)
}

fn percentile(sorted_samples: &[u64], percentile: f64) -> u64 {
    let len = sorted_samples.len();
    let rank = ((len as f64 * percentile).ceil() as usize).saturating_sub(1);
    sorted_samples[rank.min(len - 1)]
}

fn message_count() -> usize {
    env::var("NAUTILUS_WS_LATENCY_MESSAGES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MESSAGE_COUNT)
}

fn payload_sizes() -> Vec<usize> {
    env::var("NAUTILUS_WS_LATENCY_PAYLOADS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .filter(|value| *value > 0)
                .collect()
        })
        .filter(|values: &Vec<usize>| !values.is_empty())
        .unwrap_or_else(|| DEFAULT_PAYLOAD_SIZES.to_vec())
}

fn print_header() {
    println!(
        "{:<8} {:<18} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "payload", "library", "p50_us", "p90_us", "p99_us", "p99.9_us", "max_us", "mean_us"
    );
}

fn print_row(payload_size: usize, name: &str, summary: &LatencySummary) {
    println!(
        "{:<8} {:<18} {:>10.3} {:>10.3} {:>10.3} {:>10.3} {:>10.3} {:>10.3}",
        payload_size,
        name,
        ns_to_us(summary.p50_ns),
        ns_to_us(summary.p90_ns),
        ns_to_us(summary.p99_ns),
        ns_to_us(summary.p999_ns),
        ns_to_us(summary.max_ns),
        ns_to_us(summary.mean_ns),
    );
}

fn ns_to_us(value: u64) -> f64 {
    value as f64 / 1_000.0
}
