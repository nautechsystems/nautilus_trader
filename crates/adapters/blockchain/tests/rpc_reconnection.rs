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

//! Integration tests for blockchain RPC reconnection logic.
//!
//! These tests verify that subscriptions are properly re-established after
//! WebSocket reconnection events.

#![cfg(feature = "turmoil")]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nautilus_blockchain::rpc::core::CoreBlockchainRpcClient;
use nautilus_model::defi::{Blockchain, Chain};
use rstest::rstest;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use turmoil::{Builder, net};

/// Simulates a blockchain RPC node that handles eth_subscribe and sends block notifications.
async fn blockchain_rpc_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8545").await?;

    loop {
        let (stream, _) = listener.accept().await?;

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
                                        subscription_id += 1;
                                        let response = serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "id": json.get("id").unwrap(),
                                            "result": format!("0x{subscription_id:x}")
                                        });

                                        let _ = ws_stream
                                            .send(Message::Text(response.to_string().into()))
                                            .await;

                                        // Send a test block notification
                                        tokio::time::sleep(Duration::from_millis(50)).await;
                                        let notification = serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "method": "eth_subscription",
                                            "params": {
                                                "subscription": format!("0x{subscription_id:x}"),
                                                "result": {
                                                    "hash": "0x1234567890abcdef",
                                                    "number": "0x1",
                                                    "timestamp": "0x64d5e3c0",
                                                    "parentHash": "0x0000000000000000",
                                                    "nonce": "0x0",
                                                    "sha3Uncles": "0x0",
                                                    "logsBloom": "0x0",
                                                    "transactionsRoot": "0x0",
                                                    "stateRoot": "0x0",
                                                    "receiptsRoot": "0x0",
                                                    "miner": "0x0000000000000000000000000000000000000000",
                                                    "difficulty": "0x0",
                                                    "totalDifficulty": "0x0",
                                                    "extraData": "0x0",
                                                    "size": "0x0",
                                                    "gasLimit": "0x0",
                                                    "gasUsed": "0x0",
                                                    "transactions": [],
                                                    "uncles": [],
                                                    "baseFeePerGas": "0x0"
                                                }
                                            }
                                        });

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

#[rstest]
fn test_rpc_basic_subscription() {
    let mut sim = Builder::new().build();

    sim.host("rpc-node", blockchain_rpc_server);

    sim.client("client", async move {
        let chain = Chain::new(Blockchain::Ethereum, 1);
        let mut rpc_client = CoreBlockchainRpcClient::new(chain, "ws://rpc-node:8545".to_string());

        // Connect to RPC node
        rpc_client.connect().await.expect("Should connect");

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

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_rpc_reconnection_resubscribes() {
    let mut sim = Builder::new().build();

    // Server that accepts connection, sends block, drops it, then accepts reconnection
    sim.host("rpc-node", || async {
        let listener = net::TcpListener::bind("0.0.0.0:8545").await?;

        // First connection
        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut ws) = accept_async(stream).await
        {
            while let Some(msg) = ws.next().await {
                if let Ok(Message::Text(text)) = msg
                    && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                    && json.get("method").and_then(|m| m.as_str()) == Some("eth_subscribe")
                {
                    // Send subscription confirmation
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": json.get("id").unwrap(),
                        "result": "0x1"
                    });
                    let _ = ws.send(Message::Text(response.to_string().into())).await;

                    // Send a block notification
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    let notification = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "eth_subscription",
                        "params": {
                            "subscription": "0x1",
                            "result": {
                                "hash": "0xfirstblock",
                                "number": "0x1",
                                "timestamp": "0x64d5e3c0",
                                "parentHash": "0x0",
                                "nonce": "0x0",
                                "sha3Uncles": "0x0",
                                "logsBloom": "0x0",
                                "transactionsRoot": "0x0",
                                "stateRoot": "0x0",
                                "receiptsRoot": "0x0",
                                "miner": "0x0",
                                "difficulty": "0x0",
                                "totalDifficulty": "0x0",
                                "extraData": "0x0",
                                "size": "0x0",
                                "gasLimit": "0x0",
                                "gasUsed": "0x0",
                                "transactions": [],
                                "uncles": [],
                                "baseFeePerGas": "0x0"
                            }
                        }
                    });
                    let _ = ws
                        .send(Message::Text(notification.to_string().into()))
                        .await;

                    // Drop connection to trigger reconnect
                    drop(ws);
                    break;
                }
            }
        }

        // Wait before accepting reconnection
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Second connection - handle resubscription
        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut ws) = accept_async(stream).await
        {
            while let Some(msg) = ws.next().await {
                if let Ok(Message::Text(text)) = msg
                    && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                    && json.get("method").and_then(|m| m.as_str()) == Some("eth_subscribe")
                {
                    // Send confirmation for resubscription
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": json.get("id").unwrap(),
                        "result": "0x2"
                    });
                    let _ = ws.send(Message::Text(response.to_string().into())).await;

                    // Send block on reconnected subscription
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    let notification = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "eth_subscription",
                        "params": {
                            "subscription": "0x2",
                            "result": {
                                "hash": "0xsecondblock",
                                "number": "0x2",
                                "timestamp": "0x64d5e3c1",
                                "parentHash": "0xfirstblock",
                                "nonce": "0x0",
                                "sha3Uncles": "0x0",
                                "logsBloom": "0x0",
                                "transactionsRoot": "0x0",
                                "stateRoot": "0x0",
                                "receiptsRoot": "0x0",
                                "miner": "0x0",
                                "difficulty": "0x0",
                                "totalDifficulty": "0x0",
                                "extraData": "0x0",
                                "size": "0x0",
                                "gasLimit": "0x0",
                                "gasUsed": "0x0",
                                "transactions": [],
                                "uncles": [],
                                "baseFeePerGas": "0x0"
                            }
                        }
                    });
                    let _ = ws
                        .send(Message::Text(notification.to_string().into()))
                        .await;
                    break;
                }
            }
        }

        Ok::<(), Box<dyn std::error::Error>>(())
    });

    sim.client("client", async move {
        let chain = Chain::new(Blockchain::Ethereum, 1);
        let mut rpc_client = CoreBlockchainRpcClient::new(chain, "ws://rpc-node:8545".to_string());

        // Connect and subscribe
        rpc_client.connect().await.expect("Should connect");
        rpc_client
            .subscribe_blocks()
            .await
            .expect("Should subscribe");

        // Receive first block
        tokio::time::sleep(Duration::from_millis(150)).await;
        let msg1 = rpc_client.next_rpc_message().await;
        assert!(msg1.is_ok(), "Should receive first block");

        // Wait for reconnection to complete
        tokio::time::sleep(Duration::from_millis(600)).await;

        // Should receive block from resubscription after reconnect
        let msg2 = rpc_client.next_rpc_message().await;
        assert!(msg2.is_ok(), "Should receive block after reconnection");

        Ok(())
    });

    sim.run().unwrap();
}
