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

//! Measures the public NT client with independently driven reads and writes over loopback TCP.

use std::{
    env,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use nautilus_network::{
    transport::Message,
    websocket::{TransportBackend, WebSocketClient, WebSocketConfig},
};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as PeerMessage;

const BLOCKING_PAYLOAD_SIZE: usize = 8 * 1024 * 1024;

type BenchResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> BenchResult {
    pin_thread("NAUTILUS_WS_CLIENT_CPU", "client")?;
    let selected_mode = env::var("NAUTILUS_WS_CLIENT_MODE").ok();
    let count = env::var("NAUTILUS_WS_CLIENT_MESSAGES").map_or(Ok(10_000), |s| s.parse())?;
    let sizes = env::var("NAUTILUS_WS_CLIENT_PAYLOADS")
        .unwrap_or_else(|_| "64,256,512,1024,4096,8192,16384".to_owned());

    for backend in [TransportBackend::Sockudo, TransportBackend::Tungstenite] {
        if env::var("NAUTILUS_WS_CLIENT_BACKEND").is_ok_and(|value| value == "sockudo")
            && backend == TransportBackend::Tungstenite
        {
            continue;
        }
        for size in sizes.split(',').map(str::parse::<usize>) {
            let size = size?;
            if size < 16 || count == 0 {
                return Err(
                    "payloads must be at least 16 bytes and message count must be nonzero".into(),
                );
            }

            for mode in ["mixed", "duplex", "ingress-batched", "pingpong"] {
                if selected_mode
                    .as_deref()
                    .is_some_and(|selected| selected != mode)
                {
                    continue;
                }
                tokio::time::timeout(Duration::from_secs(30), run(size, count, mode, backend))
                    .await??;
            }
        }
        if selected_mode
            .as_deref()
            .is_none_or(|selected| selected == "paused")
        {
            tokio::time::timeout(
                Duration::from_secs(30),
                run(16_384, 1024, "paused", backend),
            )
            .await??;
        }
    }
    Ok(())
}

async fn run(
    size: usize,
    count: usize,
    mode: &'static str,
    backend: TransportBackend,
) -> BenchResult {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let origin = Instant::now();

    // Bound offered load independently of the kernel's socket buffering
    let window = Arc::new(tokio::sync::Semaphore::new(64));
    let peer_window = Arc::clone(&window);
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let sends = if mode == "ingress-batched" {
        1
    } else if mode == "mixed" {
        count.div_ceil(64)
    } else {
        count
    };

    // Pingpong keeps one message in flight: the peer answers each client text with its send time
    let (echo_tx, mut echo_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();

    let (peer_done_tx, peer_done_rx) = tokio::sync::oneshot::channel();
    // Keep peer protocol work off the client runtime so CPU samples have an unambiguous owner
    let peer = std::thread::Builder::new()
        .name("nt-ws-peer".into())
        .spawn(move || {
            let result = (|| -> BenchResult {
                pin_thread("NAUTILUS_WS_PEER_CPU", "peer")?;
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(async move {
                        let listener = TcpListener::from_std(listener)?;
                        let (socket, _) = listener.accept().await?;
                        socket.set_nodelay(true)?;
                        if mode == "paused" {
                            socket2::SockRef::from(&socket).set_recv_buffer_size(64 * 1024)?;
                        }
                        let ws = tokio_tungstenite::accept_async(socket).await?;
                        let (mut write, mut read) = ws.split();
                        start_rx.await?;

                        let inbound = tokio::spawn(async move {
                            if mode == "paused" {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                            let mut texts = 0;
                            let mut pong = false;
                            while texts < sends || !pong {
                                let message = read.next().await.transpose()?.ok_or("peer EOF")?;
                                match message {
                                    PeerMessage::Text(data) => {
                                        assert_eq!(
                                            data.len(),
                                            if mode == "paused" && texts == 0 {
                                                BLOCKING_PAYLOAD_SIZE
                                            } else {
                                                size
                                            }
                                        );
                                        texts += 1;
                                        if mode == "pingpong" {
                                            let sent =
                                                u64::from_str_radix(&data.as_str()[..16], 16)?;
                                            echo_tx.send(sent)?;
                                        }
                                    }
                                    PeerMessage::Pong(data) => {
                                        assert_eq!(&data[..], b"probe");
                                        pong = true;
                                    }
                                    other => panic!("unexpected peer message: {other:?}"),
                                }
                            }
                            BenchResult::Ok(())
                        });

                        // Start incoming traffic after the initial large application write has filled the socket
                        if mode == "paused" {
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                        write
                            .send(PeerMessage::Ping(bytes::Bytes::from_static(b"probe")))
                            .await?;

                        for sequence in 0..count {
                            let sent = if mode == "pingpong" {
                                echo_rx.recv().await.ok_or("client text channel closed")?
                            } else {
                                peer_window.acquire().await?.forget();
                                origin.elapsed().as_nanos() as u64
                            };
                            let mut payload = vec![b'x'; size];
                            payload[..8].copy_from_slice(&(sequence as u64).to_le_bytes());
                            payload[8..16].copy_from_slice(&sent.to_le_bytes());
                            if mode == "ingress-batched" {
                                write.feed(PeerMessage::Binary(payload.into())).await?;
                                // Match the 64-message credit window so unflushed frames never exhaust it
                                if (sequence + 1) % 64 == 0 || sequence + 1 == count {
                                    write.flush().await?;
                                }
                            } else {
                                write.send(PeerMessage::Binary(payload.into())).await?;
                            }
                        }
                        inbound.await??;

                        // Hold the transport open until the last client receive has been observed
                        finish_rx.await?;
                        BenchResult::Ok(())
                    })
            })();
            let _ = peer_done_tx.send(result);
        })?;
    let completed_sends = Arc::new(AtomicUsize::new(0));
    let read_completed_sends = Arc::clone(&completed_sends);
    let send_slots = Arc::new(tokio::sync::Semaphore::new(0));
    let read_slots = Arc::clone(&send_slots);
    let started = Arc::new(OnceLock::<Instant>::new());
    let read_started = Arc::clone(&started);
    let received = Arc::new(Mutex::new((
        Vec::with_capacity(count),
        Duration::ZERO,
        None,
        0,
    )));
    let read_samples = Arc::clone(&received);
    let done = Arc::new(tokio::sync::Notify::new());
    let read_done = Arc::clone(&done);
    let handler = Arc::new(move |message: Message| {
        let data = match message {
            Message::Binary(data) => data,
            Message::Ping(_) | Message::Pong(_) => return,
            other => panic!("unexpected message: {other:?}"),
        };
        let now = origin.elapsed().as_nanos() as u64;
        let mut samples = read_samples.lock().unwrap();
        assert_eq!(data.len(), size);
        assert_eq!(
            u64::from_le_bytes(data[..8].try_into().unwrap()),
            samples.0.len() as u64
        );

        if samples.0.is_empty() {
            samples.2 = Some(read_started.get().unwrap().elapsed());
        }
        let sent = u64::from_le_bytes(data[8..16].try_into().unwrap());
        samples.0.push(now - sent);
        window.add_permits(1);
        if (mode == "mixed" && (samples.0.len() - 1) % 64 == 0) || mode == "pingpong" {
            read_slots.add_permits(1);
        }

        if samples.0.len() == count {
            samples.3 = read_completed_sends.load(Ordering::Relaxed);
            samples.1 = read_started.get().unwrap().elapsed();
            read_done.notify_one();
        }
    });
    let config = WebSocketConfig::builder()
        .url(format!("ws://{address}"))
        .backend(backend)
        .build()?;
    let client = WebSocketClient::builder()
        .config(config)
        .message_handler(handler)
        .connect()
        .await?;
    started.set(Instant::now()).unwrap();
    start_tx.send(()).unwrap();
    let mut send_samples = Vec::with_capacity(sends);
    let mut first_send_start = Duration::ZERO;
    let mut first_send_end = Duration::ZERO;

    for index in 0..sends {
        if mode == "mixed" || (mode == "pingpong" && index > 0) {
            send_slots.acquire().await?.forget();
        }
        let mut payload = "x".repeat(if mode == "paused" && index == 0 {
            BLOCKING_PAYLOAD_SIZE
        } else {
            size
        });
        let sent = Instant::now();
        if mode == "pingpong" {
            // The peer echoes this stamp, so the receive sample spans the full client round trip
            let stamp = format!("{:016x}", origin.elapsed().as_nanos() as u64);
            payload.replace_range(..16, &stamp);
        }

        if index == 0 {
            first_send_start = started.get().unwrap().elapsed();
        }

        // This API acknowledges transport completion; send_text only measures queue admission
        client.send_text_on_connection(payload, None, 0).await?;
        send_samples.push(sent.elapsed().as_nanos() as u64);
        completed_sends.store(index + 1, Ordering::Relaxed);
        if index == 0 {
            first_send_end = started.get().unwrap().elapsed();
        }
    }
    let last_send_end = started.get().unwrap().elapsed();
    done.notified().await;
    let (samples, duration, receive_first, sends_at_last_receive) = {
        let mut received = received.lock().unwrap();
        (
            std::mem::take(&mut received.0),
            received.1,
            received.2.unwrap(),
            received.3,
        )
    };

    if mode == "paused" {
        assert!(
            first_send_end
                .checked_sub(first_send_start)
                .expect("send completion follows send start")
                >= Duration::from_millis(50),
            "fixture did not block the first write"
        );
        assert!(
            receive_first > first_send_start,
            "incoming traffic preceded the first write"
        );
    }
    let output = serde_json::json!({
        "backend": backend, "mode": mode, "payload": size, "messages": count,
        "receive_messages_per_second": count as f64 / duration.as_secs_f64(),
        "receive": summary(samples), "send_completion": summary(send_samples),
        "first_send_started_ns": first_send_start.as_nanos(), "first_send_finished_ns": first_send_end.as_nanos(),
        "last_send_finished_ns": last_send_end.as_nanos(),
        "sends_completed_at_last_receive": sends_at_last_receive,
        "first_receive_ns": receive_first.as_nanos(), "last_receive_ns": duration.as_nanos(),
    });
    finish_tx.send(()).unwrap();
    peer_done_rx.await??;
    peer.join().expect("peer thread panicked");
    client.disconnect().await;
    drop(client);
    tokio::task::yield_now().await;
    println!("{output}");
    Ok(())
}

fn summary(mut samples: Vec<u64>) -> serde_json::Value {
    samples.sort_unstable();
    let percentile =
        |fraction: f64| samples[((samples.len() - 1) as f64 * fraction).round() as usize];
    serde_json::json!({
        "samples": samples.len(), "p50_ns": percentile(0.50), "p99_ns": percentile(0.99),
        "p999_ns": percentile(0.999), "max_ns": samples.last().unwrap(),
        "mean_ns": samples.iter().map(|&n| u128::from(n)).sum::<u128>() / samples.len() as u128,
    })
}

fn pin_thread(variable: &str, role: &str) -> BenchResult {
    let Ok(cpu) = env::var(variable) else {
        return Ok(());
    };
    let cpu: usize = cpu.parse()?;
    let thread_path = std::fs::read_link("/proc/thread-self")?;
    let tid = thread_path.file_name().ok_or("missing Linux thread ID")?;
    let output = std::process::Command::new("taskset")
        .args(["-pc", &cpu.to_string()])
        .arg(tid)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    eprintln!("role={role} tid={} cpu={cpu}", tid.to_string_lossy());
    Ok(())
}
