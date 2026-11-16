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

//! Integration tests for blockchain RPC reconnection logic.
//!
//! These tests verify that subscriptions are properly re-established after
//! WebSocket reconnection events.

#![cfg(feature = "turmoil")]

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use alloy::primitives::address;
use futures_util::{SinkExt, StreamExt};
use nautilus_blockchain::rpc::{core::CoreBlockchainRpcClient, types::RpcEventType};
use nautilus_common::{
    live::runner::replace_system_event_sender,
    messages::{SystemEvent, system::SocketState},
};
use nautilus_live::{SocketControl, SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::{
    defi::{Blockchain, Chain, DexType},
    identifiers::{ClientId, Venue},
};
use rstest::rstest;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use turmoil::{Builder, net};
use ustr::Ustr;

/// Simulates a blockchain RPC node that handles eth_subscribe and sends block notifications.
async fn blockchain_rpc_server(
    counter: Arc<AtomicUsize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8545").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let counter = Arc::clone(&counter);

        tokio::spawn(async move {
            if let Ok(mut ws_stream) = accept_async(stream).await {
                let mut subscription_id = 0u64;

                while let Some(msg) = ws_stream.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            // Parse RPC request
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                let method = json.get("method").and_then(|m| m.as_str());

                                match method {
                                    Some("eth_subscribe") => {
                                        // Send subscription confirmation
                                        counter.fetch_add(1, Ordering::SeqCst);
                                        subscription_id += 1;
                                        let sid = format!("0x{subscription_id:x}");
                                        let response = subscribe_confirmation_json(
                                            json.get("id").unwrap(),
                                            &sid,
                                        );

                                        let _ = ws_stream
                                            .send(Message::Text(response.to_string().into()))
                                            .await;

                                        // Send a test block notification
                                        tokio::time::sleep(Duration::from_millis(50)).await;
                                        let notification = block_notification_json(&sid);

                                        let _ = ws_stream
                                            .send(Message::Text(notification.to_string().into()))
                                            .await;
                                    }
                                    Some("eth_unsubscribe") => {
                                        let response = serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "id": json.get("id").unwrap(),
                                            "result": true
                                        });

                                        let _ = ws_stream
                                            .send(Message::Text(response.to_string().into()))
                                            .await;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Ok(Message::Ping(data)) => {
                            let _ = ws_stream.send(Message::Pong(data)).await;
                        }
                        Ok(Message::Close(_)) => {
                            let _ = ws_stream.close(None).await;
                            break;
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
        });
    }
}

/// Test client with its system event channel, reconnect registry, and endpoint.
struct TestRpcClient {
    rpc_client: CoreBlockchainRpcClient,
    system_rx: tokio::sync::mpsc::UnboundedReceiver<SystemEvent>,
    registry: SocketReconnectRegistry,
    endpoint: Ustr,
}

/// Builds a client wired to the sim `rpc-node` with socket state reporting.
async fn setup_test_client() -> TestRpcClient {
    let chain = Chain::new(Blockchain::Ethereum, 1);
    let mut rpc_client =
        CoreBlockchainRpcClient::new(chain, "ws://rpc-node:8545".to_string(), None);
    let (system_tx, system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);
    let registry = SocketReconnectRegistry::default();
    let endpoint = Ustr::from("ethereum-data-streams");
    rpc_client.set_socket_control(SocketControl::with_registry(
        ClientId::from("ETHEREUM"),
        Some(Venue::from("ETHEREUM")),
        endpoint,
        &registry,
    ));

    TestRpcClient {
        rpc_client,
        system_rx,
        registry,
        endpoint,
    }
}

/// Waits for a socket state on the expected endpoint.
async fn wait_for_socket_state(
    system_rx: &mut tokio::sync::mpsc::UnboundedReceiver<SystemEvent>,
    endpoint: Ustr,
    state: SocketState,
    what: &str,
) {
    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what} socket state"))
        .expect("system event channel closed");
    let SystemEvent::SocketState(change) = event;
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, state);
}

/// Drives one reconnect and waits for the disconnect/reconnect states.
async fn drive_reconnect(
    registry: &SocketReconnectRegistry,
    endpoint: Ustr,
    system_rx: &mut tokio::sync::mpsc::UnboundedReceiver<SystemEvent>,
) {
    let handle = registry
        .handle(ClientId::from("ETHEREUM"), endpoint)
        .expect("reconnect handle should be registered");
    assert_eq!(
        handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    wait_for_socket_state(
        system_rx,
        endpoint,
        SocketState::Disconnected,
        "disconnected",
    )
    .await;
    wait_for_socket_state(system_rx, endpoint, SocketState::Connected, "reconnected").await;
}

/// Builds an `eth_subscribe` confirmation for a request id and subscription id.
fn subscribe_confirmation_json(id: &serde_json::Value, sid: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": sid
    })
}

fn block_notification_json(sid: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_subscription",
        "params": {
            "subscription": sid,
            "result": {
                "hash": "0xabc",
                "number": "0x1",
                "parentHash": "0xdef",
                "miner": "0x0000000000000000000000000000000000000001",
                "gasLimit": "0x1c9c380",
                "gasUsed": "0x5208",
                "timestamp": "0x6553f100"
            }
        }
    })
}

#[rstest]
fn test_rpc_basic_subscription() {
    let subscribes_seen = Arc::new(AtomicUsize::new(0));
    let server_count = Arc::clone(&subscribes_seen);
    let mut sim = Builder::new().build();

    sim.host("rpc-node", move || {
        let server_count = Arc::clone(&server_count);
        blockchain_rpc_server(server_count)
    });

    sim.client("client", async move {
        let TestRpcClient {
            mut rpc_client,
            mut system_rx,
            registry,
            endpoint,
        } = setup_test_client().await;

        // Connect to RPC node
        rpc_client.connect().await.expect("Should connect");
        let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
            .await
            .expect("timed out waiting for connected socket state")
            .expect("system event channel closed");
        let SystemEvent::SocketState(change) = event;
        let handle = registry
            .handle(ClientId::from("ETHEREUM"), endpoint)
            .expect("reconnect handle should be registered");

        assert_eq!(change.client_id, ClientId::from("ETHEREUM"));
        assert_eq!(change.venue, Some(Venue::from("ETHEREUM")));
        assert_eq!(change.endpoint, endpoint);
        assert_eq!(change.state, SocketState::Connected);
        assert_eq!(
            handle.request_reconnect(),
            SocketReconnectRequestOutcome::Accepted
        );
        let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
            .await
            .expect("timed out waiting for disconnected socket state")
            .expect("system event channel closed");
        let SystemEvent::SocketState(change) = event;
        assert_eq!(change.endpoint, endpoint);
        assert_eq!(change.state, SocketState::Disconnected);
        let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
            .await
            .expect("timed out waiting for reconnected socket state")
            .expect("system event channel closed");
        let SystemEvent::SocketState(change) = event;
        assert_eq!(change.endpoint, endpoint);
        assert_eq!(change.state, SocketState::Connected);

        // Subscribe to new blocks
        rpc_client
            .subscribe_blocks()
            .await
            .expect("Should subscribe");

        // Wait for subscription confirmation and block notification
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Get the block message
        let msg = rpc_client.next_rpc_message().await;
        assert!(msg.is_ok(), "Should receive block message");

        // Recovery must not duplicate the fresh subscription
        let duplicate =
            tokio::time::timeout(Duration::from_millis(300), rpc_client.next_rpc_message()).await;
        assert!(
            duplicate.is_err(),
            "Duplicate block proves duplicate server subscriptions"
        );

        drop(rpc_client);
        assert!(
            registry
                .handle(ClientId::from("ETHEREUM"), endpoint)
                .is_none()
        );

        Ok(())
    });

    sim.run().unwrap();
    assert_eq!(
        subscribes_seen.load(Ordering::SeqCst),
        1,
        "Recovery should not duplicate the fresh subscription"
    );
}

#[rstest]
fn test_rpc_reconnect_with_unconfirmed_subscription_creates_single_subscription() {
    let subscribes_seen = Arc::new(AtomicUsize::new(0));
    let server_count = Arc::clone(&subscribes_seen);
    let mut sim = Builder::new().build();

    sim.host("rpc-node", move || {
        let server_count = Arc::clone(&server_count);
        async move {
            let listener = net::TcpListener::bind("0.0.0.0:8545").await?;

            // First connection: accept then drop to trigger client reconnect
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                tokio::time::sleep(Duration::from_millis(100)).await;
                drop(ws);
            }

            // Second connection: confirm every subscribe and notify per id
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                let mut sub_id = 0u64;

                loop {
                    let next = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;
                    let Ok(Some(msg)) = next else { break };
                    if let Ok(Message::Text(text)) = msg
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                        && json.get("method").and_then(|m| m.as_str()) == Some("eth_subscribe")
                    {
                        server_count.fetch_add(1, Ordering::SeqCst);
                        sub_id += 1;
                        let sid = format!("0x{sub_id:x}");
                        let response = subscribe_confirmation_json(json.get("id").unwrap(), &sid);
                        let _ = ws.send(Message::Text(response.to_string().into())).await;

                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let notification = block_notification_json(&sid);
                        let _ = ws
                            .send(Message::Text(notification.to_string().into()))
                            .await;
                    }
                }
            }

            Ok::<(), Box<dyn std::error::Error>>(())
        }
    });

    sim.client("client", async move {
        let TestRpcClient {
            mut rpc_client,
            mut system_rx,
            registry: _registry,
            endpoint,
        } = setup_test_client().await;

        rpc_client.connect().await.expect("Should connect");
        wait_for_socket_state(
            &mut system_rx,
            endpoint,
            SocketState::Connected,
            "connected",
        )
        .await;

        // Server drops the first connection; subscribe while reconnecting
        wait_for_socket_state(
            &mut system_rx,
            endpoint,
            SocketState::Disconnected,
            "disconnected",
        )
        .await;

        rpc_client
            .subscribe_blocks()
            .await
            .expect("Should subscribe");

        // First block arrives after recovery; a second proves duplication
        let msg = rpc_client.next_rpc_message().await;
        assert!(msg.is_ok(), "Should receive block after reconnection");

        let duplicate =
            tokio::time::timeout(Duration::from_millis(300), rpc_client.next_rpc_message()).await;
        assert!(
            duplicate.is_err(),
            "Duplicate block proves duplicate server subscriptions"
        );

        Ok(())
    });

    sim.run().unwrap();
    assert_eq!(
        subscribes_seen.load(Ordering::SeqCst),
        1,
        "Reconnect recovery should create a single server subscription"
    );
}

#[rstest]
fn test_rpc_reconnect_resends_sent_but_unconfirmed_subscription() {
    let subscribes_seen = Arc::new(AtomicUsize::new(0));
    let server_count = Arc::clone(&subscribes_seen);
    let mut sim = Builder::new().build();

    sim.host("rpc-node", move || {
        let server_count = Arc::clone(&server_count);
        async move {
            let listener = net::TcpListener::bind("0.0.0.0:8545").await?;

            // First connection: read the subscribe, hold its confirmation, then drop
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                let _ = ws.next().await;
                tokio::time::sleep(Duration::from_millis(300)).await;
            }

            // Second connection: confirm every subscribe and notify per id
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                let mut sub_id = 0u64;

                loop {
                    let next = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;
                    let Ok(Some(msg)) = next else { break };
                    if let Ok(Message::Text(text)) = msg
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                        && json.get("method").and_then(|m| m.as_str()) == Some("eth_subscribe")
                    {
                        server_count.fetch_add(1, Ordering::SeqCst);
                        sub_id += 1;
                        let sid = format!("0x{sub_id:x}");
                        let response = subscribe_confirmation_json(json.get("id").unwrap(), &sid);
                        let _ = ws.send(Message::Text(response.to_string().into())).await;

                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let notification = block_notification_json(&sid);
                        let _ = ws
                            .send(Message::Text(notification.to_string().into()))
                            .await;
                    }
                }
            }

            Ok::<(), Box<dyn std::error::Error>>(())
        }
    });

    sim.client("client", async move {
        let chain = Chain::new(Blockchain::Ethereum, 1);
        let mut rpc_client =
            CoreBlockchainRpcClient::new(chain, "ws://rpc-node:8545".to_string(), None);

        rpc_client.connect().await.expect("Should connect");
        rpc_client
            .subscribe_blocks()
            .await
            .expect("Should subscribe");

        // Recovery must resend the stale pending request exactly once
        let msg = tokio::time::timeout(Duration::from_secs(8), rpc_client.next_rpc_message())
            .await
            .expect("timed out waiting for block after recovery");
        assert!(msg.is_ok(), "Should receive block after recovery resends");

        Ok(())
    });

    sim.run().unwrap();
    assert_eq!(
        subscribes_seen.load(Ordering::SeqCst),
        1,
        "Recovery should resend the stale pending request exactly once"
    );
}

#[rstest]
fn test_rpc_reconnect_mid_restore_cancel_recovers_both_types() {
    const SWAP_SIGNATURE: &str =
        "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";

    let seen: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let server_seen = Arc::clone(&seen);
    let mut sim = Builder::new().build();

    sim.host("rpc-node", move || {
        let server_seen = Arc::clone(&server_seen);
        async move {
            let listener = net::TcpListener::bind("0.0.0.0:8545").await?;
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };

                let Ok(ws) = accept_async(stream).await else {
                    continue;
                };

                let mut ws = ws;

                let conn_idx = {
                    let mut seen = server_seen.lock().expect("test lock poisoned");
                    seen.push(Vec::new());
                    seen.len() - 1
                };

                let mut sub_id = 0u64;
                let mut head_sid: Option<String> = None;

                loop {
                    let next = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;
                    let Ok(Some(msg)) = next else { break };
                    let Ok(Message::Text(text)) = msg else { break };

                    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };

                    if json.get("method").and_then(|m| m.as_str()) != Some("eth_subscribe") {
                        continue;
                    }

                    let name = json
                        .get("params")
                        .and_then(|p| p.get(0))
                        .and_then(|n| n.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    server_seen
                        .lock()
                        .expect("test lock poisoned")
                        .get_mut(conn_idx)
                        .expect("connection slot exists")
                        .push(name.clone());
                    sub_id += 1;
                    let sid = format!("0x{sub_id:x}");
                    let response = subscribe_confirmation_json(json.get("id").unwrap(), &sid);
                    let _ = ws.send(Message::Text(response.to_string().into())).await;

                    if name == "newHeads" {
                        head_sid = Some(sid.clone());
                    }

                    tokio::time::sleep(Duration::from_millis(50)).await;

                    // Restore order across types is not deterministic: only notify
                    // once newHeads has subscribed, regardless of arrival order.
                    if let Some(head_sid) = head_sid.clone() {
                        let notification = block_notification_json(&head_sid);
                        let _ = ws
                            .send(Message::Text(notification.to_string().into()))
                            .await;
                    }
                }
            }

            Ok::<(), Box<dyn std::error::Error>>(())
        }
    });

    sim.client("client", async move {
        let TestRpcClient {
            mut rpc_client,
            mut system_rx,
            registry,
            endpoint,
        } = setup_test_client().await;

        rpc_client.connect().await.expect("Should connect");
        wait_for_socket_state(
            &mut system_rx,
            endpoint,
            SocketState::Connected,
            "connected",
        )
        .await;

        rpc_client
            .subscribe_blocks()
            .await
            .expect("Should subscribe blocks");
        rpc_client
            .subscribe_pool_events(
                RpcEventType::PoolSwap(DexType::UniswapV3),
                &[address!("0x1111111111111111111111111111111111111111")],
                SWAP_SIGNATURE.to_string(),
            )
            .await
            .expect("Should subscribe pool logs");

        // Drain both confirmations so both ids are live
        let first = rpc_client.next_rpc_message().await;
        assert!(first.is_ok(), "Should receive first block");
        let second = rpc_client.next_rpc_message().await;
        assert!(second.is_ok(), "Should receive second block");

        drive_reconnect(&registry, endpoint, &mut system_rx).await;

        // Drop the restore after exactly two polls: the first parks before the
        // first send and the second parks with one frame queued and recorded
        let mut restore = Box::pin(rpc_client.next_rpc_message());
        assert!(futures_util::poll!(&mut restore).is_pending());
        assert!(futures_util::poll!(&mut restore).is_pending());
        drop(restore);

        // Drive normally: both types must be live exactly once
        let msg = tokio::time::timeout(Duration::from_secs(5), rpc_client.next_rpc_message())
            .await
            .expect("timed out waiting for block after cancelled restore");
        assert!(msg.is_ok(), "Should receive block after recovery completes");
        tokio::time::sleep(Duration::from_millis(200)).await;

        Ok(())
    });

    sim.run().unwrap();
    let seen = seen.lock().expect("test lock poisoned");
    assert_eq!(seen.len(), 2, "Should see exactly two connections");

    for (idx, names) in seen.iter().enumerate() {
        assert_eq!(
            names.len(),
            2,
            "Connection {idx} should see exactly two subscribes"
        );
        assert!(
            names.contains(&"newHeads".to_string()),
            "Connection {idx} should subscribe blocks once"
        );
        assert!(
            names.contains(&"logs".to_string()),
            "Connection {idx} should subscribe pool logs once"
        );
    }
}

#[rstest]
fn test_rpc_reconnect_replaced_request_ignores_stale_confirmation() {
    let subscribes_seen = Arc::new(AtomicUsize::new(0));
    let unsubscribes_seen = Arc::new(AtomicUsize::new(0));
    let server_subs = Arc::clone(&subscribes_seen);
    let server_unsubs = Arc::clone(&unsubscribes_seen);
    let mut sim = Builder::new().build();

    sim.host("rpc-node", move || {
        let server_subs = Arc::clone(&server_subs);
        let server_unsubs = Arc::clone(&server_unsubs);
        async move {
            let listener = net::TcpListener::bind("0.0.0.0:8545").await?;

            // First connection: confirm the subscribe, then the client replaces it
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                let mut sub_id = 0u64;

                loop {
                    let next = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;
                    let Ok(Some(msg)) = next else { break };
                    let Ok(Message::Text(text)) = msg else { break };

                    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };

                    if json.get("method").and_then(|m| m.as_str()) == Some("eth_subscribe") {
                        sub_id += 1;
                        let sid = format!("0x{sub_id:x}");
                        let response = subscribe_confirmation_json(json.get("id").unwrap(), &sid);
                        let _ = ws.send(Message::Text(response.to_string().into())).await;
                    }
                }
            }

            // Second connection: the stale confirmation must not unsubscribe here
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                let mut sub_id = 0u64;
                let mut live: Vec<String> = Vec::new();

                loop {
                    let next = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;

                    let Ok(Some(msg)) = next else {
                        // Quiet: notify each live subscription once more
                        for sid in live.clone() {
                            let notification = block_notification_json(&sid);
                            let _ = ws
                                .send(Message::Text(notification.to_string().into()))
                                .await;
                        }

                        break;
                    };

                    let Ok(Message::Text(text)) = msg else { break };

                    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };

                    if json.get("method").and_then(|m| m.as_str()) == Some("eth_subscribe") {
                        server_subs.fetch_add(1, Ordering::SeqCst);
                        sub_id += 1;
                        let sid = format!("0x{sub_id:x}");
                        live.push(sid.clone());
                        let response = subscribe_confirmation_json(json.get("id").unwrap(), &sid);
                        let _ = ws.send(Message::Text(response.to_string().into())).await;
                        let notification = block_notification_json(&sid);
                        let _ = ws
                            .send(Message::Text(notification.to_string().into()))
                            .await;
                    } else if json.get("method").and_then(|m| m.as_str()) == Some("eth_unsubscribe")
                    {
                        server_unsubs.fetch_add(1, Ordering::SeqCst);

                        if let Some(sid) = json
                            .get("params")
                            .and_then(|p| p.get(0))
                            .and_then(|s| s.as_str())
                        {
                            live.retain(|live_sid| live_sid != sid);
                        }
                    }
                }
            }

            Ok::<(), Box<dyn std::error::Error>>(())
        }
    });

    sim.client("client", async move {
        let TestRpcClient {
            mut rpc_client,
            mut system_rx,
            registry,
            endpoint,
        } = setup_test_client().await;

        rpc_client.connect().await.expect("Should connect");
        wait_for_socket_state(
            &mut system_rx,
            endpoint,
            SocketState::Connected,
            "connected",
        )
        .await;

        rpc_client
            .subscribe_blocks()
            .await
            .expect("Should subscribe");
        // Let the confirmation arrive without consuming it
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Replace the subscription on the new connection before draining
        drive_reconnect(&registry, endpoint, &mut system_rx).await;

        rpc_client
            .unsubscribe_blocks()
            .await
            .expect("Should unsubscribe");
        rpc_client
            .subscribe_blocks()
            .await
            .expect("Should resubscribe");

        // The replacement stays live: no stale unsubscribe may cancel it
        let first = tokio::time::timeout(Duration::from_secs(5), rpc_client.next_rpc_message())
            .await
            .expect("timed out waiting for first block");
        assert!(first.is_ok(), "Should receive first block");
        let second = tokio::time::timeout(Duration::from_secs(5), rpc_client.next_rpc_message())
            .await
            .expect("timed out waiting for second block");
        assert!(second.is_ok(), "Should receive second block");

        Ok(())
    });

    sim.run().unwrap();
    assert_eq!(
        subscribes_seen.load(Ordering::SeqCst),
        1,
        "Replacement connection should see exactly one subscribe"
    );
    assert_eq!(
        unsubscribes_seen.load(Ordering::SeqCst),
        0,
        "Stale confirmation must not unsubscribe on the new connection"
    );
}

#[rstest]
fn test_rpc_reconnect_pruned_request_ignores_stale_confirmation() {
    let subscribes_seen = Arc::new(AtomicUsize::new(0));
    let unsubscribes_seen = Arc::new(AtomicUsize::new(0));
    let server_subs = Arc::clone(&subscribes_seen);
    let server_unsubs = Arc::clone(&unsubscribes_seen);
    let mut sim = Builder::new().build();

    sim.host("rpc-node", move || {
        let server_subs = Arc::clone(&server_subs);
        let server_unsubs = Arc::clone(&server_unsubs);
        async move {
            let listener = net::TcpListener::bind("0.0.0.0:8545").await?;

            // First connection: the client only passes through it
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                loop {
                    let next = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;
                    let Ok(Some(msg)) = next else { break };
                    let Ok(Message::Text(_)) = msg else { break };
                }
            }

            // Second connection: confirm the subscribe, then the client moves on
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                let mut sub_id = 0u64;

                loop {
                    let next = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;
                    let Ok(Some(msg)) = next else { break };
                    let Ok(Message::Text(text)) = msg else { break };

                    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };

                    if json.get("method").and_then(|m| m.as_str()) == Some("eth_subscribe") {
                        sub_id += 1;
                        let sid = format!("0x{sub_id:x}");
                        let response = subscribe_confirmation_json(json.get("id").unwrap(), &sid);
                        let _ = ws.send(Message::Text(response.to_string().into())).await;
                    }
                }
            }

            // Third connection: the pruned confirmation must not unsubscribe here
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                let mut sub_id = 0u64;
                let mut live: Vec<String> = Vec::new();

                loop {
                    let next = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;

                    let Ok(Some(msg)) = next else {
                        for sid in live.clone() {
                            let notification = block_notification_json(&sid);
                            let _ = ws
                                .send(Message::Text(notification.to_string().into()))
                                .await;
                        }

                        break;
                    };

                    let Ok(Message::Text(text)) = msg else { break };

                    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };

                    if json.get("method").and_then(|m| m.as_str()) == Some("eth_subscribe") {
                        server_subs.fetch_add(1, Ordering::SeqCst);
                        sub_id += 1;
                        let sid = format!("0x{sub_id:x}");
                        live.push(sid.clone());
                        let response = subscribe_confirmation_json(json.get("id").unwrap(), &sid);
                        let _ = ws.send(Message::Text(response.to_string().into())).await;
                        let notification = block_notification_json(&sid);
                        let _ = ws
                            .send(Message::Text(notification.to_string().into()))
                            .await;
                    } else if json.get("method").and_then(|m| m.as_str()) == Some("eth_unsubscribe")
                    {
                        server_unsubs.fetch_add(1, Ordering::SeqCst);

                        if let Some(sid) = json
                            .get("params")
                            .and_then(|p| p.get(0))
                            .and_then(|s| s.as_str())
                        {
                            live.retain(|live_sid| live_sid != sid);
                        }
                    }
                }
            }

            Ok::<(), Box<dyn std::error::Error>>(())
        }
    });

    sim.client("client", async move {
        let TestRpcClient {
            mut rpc_client,
            mut system_rx,
            registry,
            endpoint,
        } = setup_test_client().await;

        rpc_client.connect().await.expect("Should connect");
        wait_for_socket_state(
            &mut system_rx,
            endpoint,
            SocketState::Connected,
            "connected",
        )
        .await;

        // Move to the second connection and subscribe there without draining
        drive_reconnect(&registry, endpoint, &mut system_rx).await;

        rpc_client
            .subscribe_blocks()
            .await
            .expect("Should subscribe");
        // Let the confirmation arrive without consuming it
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Move to the third connection, then drain the backlog
        drive_reconnect(&registry, endpoint, &mut system_rx).await;

        let first = tokio::time::timeout(Duration::from_secs(5), rpc_client.next_rpc_message())
            .await
            .expect("timed out waiting for first block");
        assert!(first.is_ok(), "Should receive first block");
        let second = tokio::time::timeout(Duration::from_secs(5), rpc_client.next_rpc_message())
            .await
            .expect("timed out waiting for second block");
        assert!(second.is_ok(), "Should receive second block");

        Ok(())
    });

    sim.run().unwrap();
    assert_eq!(
        subscribes_seen.load(Ordering::SeqCst),
        1,
        "Replacement connection should see exactly one subscribe"
    );
    assert_eq!(
        unsubscribes_seen.load(Ordering::SeqCst),
        0,
        "Pruned confirmation must not unsubscribe on the new connection"
    );
}
