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

//! Redis message bus hot-path benchmarks.

use std::{
    fs,
    hint::black_box,
    time::{Duration, Instant},
};

use bytes::Bytes;
use criterion::{Criterion, Throughput, criterion_group};
use nautilus_common::{
    enums::SerializationEncoding,
    msgbus::{BusMessage, BusPayloadType, MessageBusBacking, MessageBusConfig},
};
use nautilus_core::UUID4;
use nautilus_infrastructure::redis::{
    get_stream_key,
    msgbus::{RedisMessageBusBacking, RedisMessageBusConfig},
};
use nautilus_model::identifiers::TraderId;
use redis::AsyncCommands;
use ustr::Ustr;

#[path = "../src/redis/stream_fields.rs"]
mod stream_fields;

const MESSAGE_BATCH_SIZE: u64 = 16;

fn bench_bus_message_fields(c: &mut Criterion) {
    let custom = message(BusPayloadType::Custom(Ustr::from("CustomData")));
    let typed = message(BusPayloadType::TradingCommand);
    let mut group = c.benchmark_group("Redis message bus/fields");
    group.throughput(Throughput::Elements(MESSAGE_BATCH_SIZE));

    group.bench_function("custom", |b| {
        b.iter(|| {
            for _ in 0..MESSAGE_BATCH_SIZE {
                let fields =
                    stream_fields::bus_message_fields(black_box(&custom), black_box("json"));
                black_box(fields.as_slice());
            }
        });
    });
    group.bench_function("typed", |b| {
        b.iter(|| {
            for _ in 0..MESSAGE_BATCH_SIZE {
                let fields =
                    stream_fields::bus_message_fields(black_box(&typed), black_box("json"));
                black_box(fields.as_slice());
            }
        });
    });

    group.finish();
}

fn message(payload_type: BusPayloadType) -> BusMessage {
    BusMessage::with_str_topic(
        "commands.execution.EXTERNAL",
        payload_type,
        Bytes::from(vec![0xAB; 256]),
        SerializationEncoding::Json,
    )
}

criterion_group!(benches, bench_bus_message_fields);

fn main() {
    if let Ok(port) = std::env::var("NAUTILUS_BENCH_REDIS_PORT") {
        nautilus_common::live::get_runtime().block_on(measure_transport(port.parse().unwrap()));
    } else {
        benches();
        Criterion::default().configure_from_args().final_summary();
    }
}

async fn measure_transport(port: u16) {
    let directory = std::env::var("NAUTILUS_BENCH_REPLAY_DIR").unwrap();
    let samples = std::env::var("NAUTILUS_BENCH_SAMPLES")
        .map_or(1_000, |value| value.parse::<usize>().unwrap());
    let trials =
        std::env::var("NAUTILUS_BENCH_TRIALS").map_or(3, |value| value.parse::<usize>().unwrap());
    let warmup =
        std::env::var("NAUTILUS_BENCH_WARMUP").map_or(100, |value| value.parse::<usize>().unwrap());
    let selected = std::env::var("NAUTILUS_BENCH_CASE").ok();
    assert!(samples > 0);
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    paths.sort();

    let backing = RedisMessageBusConfig {
        host: Some("127.0.0.1".into()),
        port: Some(port),
        ..Default::default()
    };

    let config = MessageBusConfig {
        use_instance_id: true,
        stream_per_topic: false,
        autotrim_maxlen: Some(64),
        ..Default::default()
    };

    println!(
        "{}",
        serde_json::json!({"environment": {"samples": samples, "trials": trials, "warmup": warmup, "redis_port": port, "autotrim_maxlen": config.autotrim_maxlen, "inflight": 1}})
    );
    let trader_id = TraderId::from("BENCH-001");
    let instance_id = UUID4::new();
    let key = get_stream_key(trader_id, instance_id, &config);
    let mut publisher =
        RedisMessageBusBacking::new(trader_id, instance_id, config, backing.clone()).unwrap();

    let mut receiver = RedisMessageBusBacking::new(
        TraderId::from("BENCH-002"),
        UUID4::new(),
        MessageBusConfig {
            external_streams: Some(vec![key.clone()]),
            stream_per_topic: false,
            ..Default::default()
        },
        backing,
    )
    .unwrap();

    let mut incoming = receiver.take_receiver().unwrap();
    wait_for_reader(&publisher, &mut incoming).await;

    for trial in 0..trials {
        for index in 0..paths.len() {
            let fixture: serde_json::Value =
                serde_json::from_slice(&fs::read(&paths[(index + trial) % paths.len()]).unwrap())
                    .unwrap();
            let name = fixture["case"].as_str().unwrap();
            if fixture["diagnostic"].as_bool().unwrap()
                || selected.as_ref().is_some_and(|selected| selected != name)
            {
                continue;
            }

            let message: BusMessage = serde_json::from_value(fixture["message"].clone()).unwrap();
            let mut times =
                measure_delivery(&publisher, &mut incoming, &message, samples, warmup).await;

            let mean = times.iter().map(|value| *value as f64).sum::<f64>() / samples as f64;
            times.sort_unstable();
            let percentile = |n| times[(samples - 1) * n / 1_000];
            println!(
                "{}",
                serde_json::json!({"case": name, "operation": "redis_transport_preencoded", "format": message.encoding,
                "trial": trial, "samples": samples, "bytes": message.payload.len(), "mean_ns": mean,
                "p50_ns": percentile(500), "p95_ns": percentile(950), "p99_ns": percentile(990),
                "p999_ns": percentile(999), "max_ns": times.last().unwrap()})
            );
        }
    }

    publisher.close();
    receiver.close();
    let mut connection = redis::Client::open(format!("redis://127.0.0.1:{port}/"))
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let removed: usize = connection.del(&key).await.unwrap();
    assert_eq!(removed, 1);
}

async fn wait_for_reader(
    publisher: &RedisMessageBusBacking,
    incoming: &mut tokio::sync::mpsc::Receiver<BusMessage>,
) {
    let ready = message(BusPayloadType::QuoteTick);
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        publisher.publish(ready.clone());

        if tokio::time::timeout(Duration::from_millis(100), incoming.recv())
            .await
            .is_ok()
        {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "Redis stream did not become ready"
        );
    }

    let barrier = BusMessage::with_str_topic(
        "benchmark.barrier",
        BusPayloadType::QuoteTick,
        Bytes::new(),
        SerializationEncoding::Json,
    );
    publisher.publish(barrier.clone());

    while tokio::time::timeout(Duration::from_secs(5), incoming.recv())
        .await
        .unwrap()
        .unwrap()
        .topic
        != barrier.topic
    {}
}

async fn measure_delivery(
    publisher: &RedisMessageBusBacking,
    incoming: &mut tokio::sync::mpsc::Receiver<BusMessage>,
    message: &BusMessage,
    samples: usize,
    warmup: usize,
) -> Vec<u64> {
    let mut times = Vec::with_capacity(samples);
    for index in 0..warmup + samples {
        let started = Instant::now();
        publisher.publish(message.clone());
        let received = tokio::time::timeout(Duration::from_secs(5), incoming.recv())
            .await
            .unwrap()
            .unwrap();
        let elapsed = started.elapsed().as_nanos() as u64;
        assert_eq!(received.topic, message.topic);
        assert_eq!(received.payload_type, message.payload_type);
        assert_eq!(received.encoding, message.encoding);
        assert_eq!(received.payload, message.payload);

        if index >= warmup {
            times.push(elapsed);
        }
    }

    times
}
