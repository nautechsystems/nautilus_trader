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

use std::sync::Arc;

use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_model::defi::chain::Chain;
use nautilus_network::websocket::{Consumer, WebSocketClient, WebSocketConfig};
use reqwest::header::USER_AGENT;
use tokio_tungstenite::tungstenite::Message;

use crate::rpc::error::BlockchainRpcClientError;

pub struct CoreBlockchainRpcClient {
    chain: Chain,
    wss_rpc_url: String,
    wss_client: Option<Arc<WebSocketClient>>,
    wss_consumer_rx: Option<tokio::sync::mpsc::Receiver<Message>>,
}

impl CoreBlockchainRpcClient {
    pub fn new(chain: Chain, wss_rpc_url: String) -> Self {
        Self {
            chain,
            wss_rpc_url,
            wss_client: None,
            wss_consumer_rx: None,
        }
    }

    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let user_agent = (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string());
        // Most of the blockchain rpc nodes require a heartbeat to keep the connection alive
        let heartbeat_interval = 30;
        let config = WebSocketConfig {
            url: self.wss_rpc_url.clone(),
            headers: vec![user_agent],
            heartbeat: Some(heartbeat_interval),
            heartbeat_msg: None,
            handler: Consumer::Rust(tx),
            ping_handler: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None,
            reconnect_jitter_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
        };
        let client = WebSocketClient::connect(config, None, None, None, vec![], None).await?;

        self.wss_client = Some(Arc::new(client));
        self.wss_consumer_rx = Some(rx);

        Ok(())
    }

    async fn subscribe_events(
        &self,
        subscription_id: String,
    ) -> Result<(), BlockchainRpcClientError> {
        if let Some(client) = &self.wss_client {
            log::info!("Subscribing to new blocks on chain {}", self.chain.name);
            let msg = serde_json::json!({
                "method": "eth_subscribe",
                "id": 1,
                "jsonrpc": "2.0",
                "params": [subscription_id]
            });
            client.send_text(msg.to_string(), None).await;
            Ok(())
        } else {
            Err(BlockchainRpcClientError::ClientError(String::from(
                "Client not connected",
            )))
        }
    }

    async fn unsubscribe_events(
        &self,
        subscription_id: String,
    ) -> Result<(), BlockchainRpcClientError> {
        if let Some(client) = &self.wss_client {
            log::info!("Unsubscribing to new blocks on chain {}", self.chain.name);
            let msg = serde_json::json!({
                "method": "eth_unsubscribe",
                "id": 1,
                "jsonrpc": "2.0",
                "params": [subscription_id]
            });
            client.send_text(msg.to_string(), None).await;
            Ok(())
        } else {
            Err(BlockchainRpcClientError::ClientError(String::from(
                "Client not connected",
            )))
        }
    }

    pub async fn next_rpc_message(&mut self) -> Option<Message> {
        match &mut self.wss_consumer_rx {
            Some(rx) => rx.recv().await,
            None => None,
        }
    }

    pub async fn process_rpc_messages(&mut self) {
        while let Some(msg) = self.next_rpc_message().await {
            match msg {
                Message::Text(text) => {
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(json) => {
                            // check if json serde Value contains both the id field and result field
                            if json.get("id").is_some() && json.get("result").is_some() {
                                let subscription_request_id =
                                    json.get("id").unwrap().as_u64().unwrap();
                                let result = json.get("result").unwrap().to_string();
                                println!(
                                    "SUBSCRIPTION ----->>> {subscription_request_id} : {result}"
                                )
                            } else {
                                println!("DATA ----->>> {text}")
                            }
                        }
                        Err(e) => {
                            log::error!("Error parsing RPC response to json value: {}", e);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub async fn subscribe_live_blocks(&self) -> Result<(), BlockchainRpcClientError> {
        self.subscribe_events(String::from("newHeads")).await
    }

    pub async fn unsubscribe_live_blocks(&self) -> Result<(), BlockchainRpcClientError> {
        self.unsubscribe_events(String::from("newHeads")).await
    }
}
