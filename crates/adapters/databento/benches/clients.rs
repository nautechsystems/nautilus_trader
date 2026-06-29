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

//! End-to-end Databento client benches.
//!
//! These benches keep the network local and deterministic while exercising the
//! public historical client and live feed handler paths.

mod common;

#[allow(dead_code)]
#[path = "../tests/common/mod.rs"]
mod feed_test_common;

use std::{hint::black_box, mem::size_of, sync::Arc, time::Duration};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use databento::{dbn, live::Subscription};
use feed_test_common::{
    TEST_DATASET, TEST_KEY, create_test_handler, mbo_msg_with_ts, mock_server::MockLsgServer,
    symbol_mapping_msg, trade_msg,
};
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_databento::{
    common::Credential,
    historical::{DatabentoHistoricalClient, RangeQueryParams},
    live::{DatabentoMessage, HandlerCommand},
};
use nautilus_model::{data::Data, identifiers::Symbol};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    runtime::{Builder, Runtime},
    task::JoinHandle,
};

const HISTORICAL_RECORDS: u64 = 2;
const LIVE_RECORDS: u64 = 100;
const LIVE_MBO_RECORDS: u64 = 10_000;
const INSTRUMENT_ID: u32 = 1;
const RAW_SYMBOL: &str = "ESM4";
const RECV_TIMEOUT: Duration = Duration::from_secs(5);

fn bench_historical_client(c: &mut Criterion) {
    let runtime = runtime();
    let trades_body = std::fs::read(common::data_path("test_data.trades.dbn.zst")).unwrap();
    let quotes_body = std::fs::read(common::data_path("test_data.mbp-1.dbn.zst")).unwrap();
    let trades_server = HistoricalFixtureServer::start(&runtime, trades_body);
    let quotes_server = HistoricalFixtureServer::start(&runtime, quotes_body);
    let trades_client = historical_client(trades_server.base_url());
    let quotes_client = historical_client(quotes_server.base_url());

    let mut group = c.benchmark_group("historical_client");

    group.throughput(Throughput::Elements(HISTORICAL_RECORDS));
    group.bench_function("trades_http", |b| {
        b.iter(|| {
            let items = runtime
                .block_on(trades_client.get_range_trades(range_params(), None))
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(HISTORICAL_RECORDS));
    group.bench_function("mbp1_quotes_http", |b| {
        b.iter(|| {
            let items = runtime
                .block_on(quotes_client.get_range_quotes(range_params(), Some("mbp-1".to_string())))
                .unwrap();
            black_box(items);
        });
    });

    group.finish();
    trades_server.stop(&runtime);
    quotes_server.stop(&runtime);
}

fn bench_live_client(c: &mut Criterion) {
    let runtime = runtime();
    let server = runtime.block_on(MockLsgServer::new(TEST_DATASET));
    let addr = server.addr();

    let mut group = c.benchmark_group("live_client");
    group.throughput(Throughput::Elements(LIVE_RECORDS));
    group.bench_function("trades_mock_lsg", |b| {
        b.iter(|| {
            let received = runtime.block_on(run_live_trade_burst(&server, &addr, LIVE_RECORDS));
            black_box(received);
        });
    });

    let payload = live_mbo_payload(LIVE_MBO_RECORDS);
    let (cmd_tx, mut msg_rx, handle) = runtime.block_on(start_live_mbo_session(&server, &addr));

    group.throughput(Throughput::Elements(LIVE_MBO_RECORDS));
    group.bench_function("mbo_stream_mock_lsg", |b| {
        b.iter(|| {
            let received = runtime.block_on(run_live_mbo_burst(
                &server,
                Arc::clone(&payload),
                &mut msg_rx,
                LIVE_MBO_RECORDS,
            ));
            black_box(received);
        });
    });

    runtime.block_on(close_live_session(cmd_tx, handle));
    group.finish();
    runtime.block_on(server.stop());
}

fn runtime() -> Runtime {
    Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

fn historical_client(base_url: &str) -> DatabentoHistoricalClient {
    let client = DatabentoHistoricalClient::new_with_base_url(
        Credential::new(TEST_KEY),
        common::publishers_path(),
        get_atomic_clock_realtime(),
        false,
        base_url,
    )
    .unwrap();
    client.set_price_precision(Symbol::from(RAW_SYMBOL), common::PRICE_PRECISION);
    client
}

fn range_params() -> RangeQueryParams {
    RangeQueryParams {
        dataset: TEST_DATASET.to_string(),
        symbols: vec![RAW_SYMBOL.to_string()],
        start: 1_000_000_000.into(),
        end: Some(2_000_000_000.into()),
        limit: None,
        price_precision: Some(common::PRICE_PRECISION),
    }
}

fn subscription() -> Subscription {
    Subscription::builder()
        .symbols(RAW_SYMBOL)
        .schema(dbn::Schema::Trades)
        .stype_in(dbn::SType::RawSymbol)
        .build()
}

fn mbo_subscription() -> Subscription {
    Subscription::builder()
        .symbols(RAW_SYMBOL)
        .schema(dbn::Schema::Mbo)
        .stype_in(dbn::SType::RawSymbol)
        .build()
}

async fn run_live_trade_burst(server: &MockLsgServer, addr: &str, records: u64) -> u64 {
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(addr, TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    for _ in 0..records {
        server.send_record(trade_msg(INSTRUMENT_ID, 100_000_000_000, 50));
    }

    let handle = tokio::runtime::Handle::current().spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::SetPricePrecision(
            Symbol::from(RAW_SYMBOL),
            common::PRICE_PRECISION,
        ))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription()))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let mut received = 0;
    while received < records {
        let msg = tokio::time::timeout(RECV_TIMEOUT, msg_rx.recv())
            .await
            .expect("timed out waiting for live benchmark message")
            .expect("live benchmark message channel closed");

        if matches!(msg, DatabentoMessage::Data(Data::Trade(_))) {
            received += 1;
        }
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    tokio::time::timeout(RECV_TIMEOUT, handle)
        .await
        .expect("timed out waiting for live benchmark handler")
        .expect("live benchmark handler panicked")
        .unwrap();
    received
}

async fn start_live_mbo_session(
    server: &MockLsgServer,
    addr: &str,
) -> (
    tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    tokio::sync::mpsc::UnboundedReceiver<DatabentoMessage>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(addr, TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(live_mbo_msg(0, 128 | 32));
    server.send_record(live_mbo_msg(1, 128));

    let handle = tokio::runtime::Handle::current().spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::SetPricePrecision(
            Symbol::from(RAW_SYMBOL),
            common::PRICE_PRECISION,
        ))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(mbo_subscription()))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    receive_live_mbo_deltas(&mut msg_rx, 2).await;

    (cmd_tx, msg_rx, handle)
}

async fn run_live_mbo_burst(
    server: &MockLsgServer,
    payload: Arc<[u8]>,
    msg_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DatabentoMessage>,
    records: u64,
) -> u64 {
    server.send_bytes(payload);
    receive_live_mbo_deltas(msg_rx, records).await
}

async fn receive_live_mbo_deltas(
    msg_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DatabentoMessage>,
    records: u64,
) -> u64 {
    let mut received = 0;
    while received < records {
        let msg = tokio::time::timeout(RECV_TIMEOUT, msg_rx.recv())
            .await
            .expect("timed out waiting for live MBO benchmark message")
            .expect("live MBO benchmark message channel closed");

        if let DatabentoMessage::Data(Data::Deltas(deltas)) = msg {
            received += deltas.deltas.len() as u64;
        }
    }
    received
}

async fn close_live_session(
    cmd_tx: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    handle: tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    cmd_tx.send(HandlerCommand::Close).unwrap();
    tokio::time::timeout(RECV_TIMEOUT, handle)
        .await
        .expect("timed out waiting for live benchmark handler")
        .expect("live benchmark handler panicked")
        .unwrap();
}

fn live_mbo_payload(records: u64) -> Arc<[u8]> {
    let mut bytes = Vec::with_capacity(records as usize * size_of::<dbn::MboMsg>());
    for sequence in 2..records + 2 {
        bytes.extend_from_slice(live_mbo_msg(sequence, 128).as_ref());
    }
    Arc::from(bytes)
}

fn live_mbo_msg(sequence: u64, flags: u8) -> dbn::MboMsg {
    let price = 100_000_000_000 + (sequence as i64 % 20) * 250_000_000;
    let side = if sequence % 2 == 0 { b'B' } else { b'A' };
    let mut msg = mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'A',
        side,
        flags,
        price,
        1_000_000_000 + sequence,
    );
    msg.order_id = sequence;
    msg.sequence = sequence as u32;
    msg.size = 10 + (sequence as u32 % 20);
    msg
}

struct HistoricalFixtureServer {
    base_url: String,
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl HistoricalFixtureServer {
    fn start(runtime: &Runtime, body: Vec<u8>) -> Self {
        runtime.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base_url = format!("http://{}/", listener.local_addr().unwrap());
            let (shutdown, shutdown_rx) = tokio::sync::oneshot::channel();
            let body = Arc::new(body);

            let task = tokio::runtime::Handle::current().spawn(serve_historical_fixture(
                listener,
                body,
                shutdown_rx,
            ));

            Self {
                base_url,
                shutdown,
                task,
            }
        })
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn stop(self, runtime: &Runtime) {
        let _ = self.shutdown.send(());
        runtime.block_on(self.task).unwrap();
    }
}

async fn serve_historical_fixture(
    listener: TcpListener,
    body: Arc<Vec<u8>>,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let Ok((mut stream, _)) = accepted else {
                    break;
                };
                let body = Arc::clone(&body);

                tokio::runtime::Handle::current().spawn(async move {
                    read_http_request(&mut stream).await;
                    write_http_response(&mut stream, &body).await;
                });
            }
        }
    }
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    let header_end;

    loop {
        let read = stream.read(&mut buffer).await.unwrap_or(0);
        if read == 0 {
            return;
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            header_end = index + 4;
            break;
        }
    }

    let content_length = content_length(&request[..header_end]);
    let mut body_read = request.len().saturating_sub(header_end);
    while body_read < content_length {
        let read = stream.read(&mut buffer).await.unwrap_or(0);
        if read == 0 {
            break;
        }
        body_read += read;
    }
}

fn content_length(headers: &[u8]) -> usize {
    let headers = String::from_utf8_lossy(headers);
    headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0)
}

async fn write_http_response(stream: &mut tokio::net::TcpStream, body: &[u8]) {
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
    let _ = stream.shutdown().await;
}

criterion_group!(benches, bench_historical_client, bench_live_client);
criterion_main!(benches);
