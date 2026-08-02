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

use std::{collections::HashMap, fmt::Debug, sync::Arc};

use alloy::primitives::Address;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
#[cfg(feature = "hypersync")]
use nautilus_model::defi::DexType;
use nautilus_model::defi::{
    Block, Chain,
    rpc::{RpcLog, RpcNodeWssResponse},
};
use nautilus_network::{
    RECONNECTED,
    http::USER_AGENT,
    websocket::{TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler},
};
use tokio_tungstenite::tungstenite::Message;

#[cfg(feature = "hypersync")]
use crate::exchanges::get_dex_extended;
use crate::rpc::{
    error::BlockchainRpcClientError,
    types::{BlockchainMessage, RpcEventType},
    utils::{
        extract_rpc_subscription_id, is_subscription_confirmation_response, is_subscription_event,
        is_unsubscribe_confirmation_response,
    },
};

/// Core implementation of a blockchain RPC client that serves as the base for all chain-specific clients.
///
/// It provides a shared implementation of common blockchain RPC functionality, handling:
/// - WebSocket connection management with blockchain RPC node.
/// - Subscription lifecycle (creation, tracking, and termination).
/// - Message serialization and deserialization of RPC messages.
/// - Event type mapping and dispatching.
/// - Automatic subscription re-establishment on reconnection.
pub struct CoreBlockchainRpcClient {
    /// The blockchain network type this client connects to.
    chain: Chain,
    /// WebSocket secure URL for the blockchain node's RPC endpoint.
    wss_rpc_url: String,
    /// Auto-incrementing counter for generating unique RPC request IDs.
    request_id: u64,
    /// Tracks in-flight subscription requests by mapping request IDs to their event types.
    pending_subscription_request: HashMap<u64, RpcEventType>,
    /// Maps active subscription IDs to their corresponding event types for message
    /// deserialization.
    subscription_event_types: HashMap<String, RpcEventType>,
    /// The active WebSocket client connection.
    wss_client: Option<Arc<WebSocketClient>>,
    /// Channel receiver for consuming WebSocket messages.
    wss_consumer_rx: Option<tokio::sync::mpsc::UnboundedReceiver<Message>>,
    /// Tracks desired subscriptions that need to be re-established on reconnection.
    subscriptions: Arc<tokio::sync::RwLock<HashMap<RpcEventType, RpcSubscription>>>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    transport_backend: TransportBackend,
    /// Optional proxy URL for the WebSocket connection.
    proxy_url: Option<String>,
}

impl Debug for CoreBlockchainRpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(CoreBlockchainRpcClient))
            .field("chain", &self.chain)
            .field("wss_rpc_url", &self.wss_rpc_url)
            .field("request_id", &self.request_id)
            .field(
                "pending_subscription_request",
                &self.pending_subscription_request,
            )
            .field("subscription_event_types", &self.subscription_event_types)
            .field(
                "wss_client",
                &self.wss_client.as_ref().map(|_| "<WebSocketClient>"),
            )
            .field(
                "wss_consumer_rx",
                &self.wss_consumer_rx.as_ref().map(|_| "<Receiver>"),
            )
            .field("confirmed_subscriptions", &"<RwLock<HashMap>>")
            .finish()
    }
}

#[derive(Debug, Clone)]
struct RpcSubscription {
    name: String,
    filter: Option<serde_json::Value>,
}

impl RpcSubscription {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            filter: None,
        }
    }

    fn pool_logs(addresses: &[Address], event_signature: String) -> Self {
        let mut addresses: Vec<String> = addresses
            .iter()
            .map(|address| format!("{address:?}"))
            .collect();
        addresses.sort();

        Self {
            name: "logs".to_string(),
            filter: Some(serde_json::json!({
                "address": addresses,
                "topics": [event_signature],
            })),
        }
    }

    fn params(&self) -> Vec<serde_json::Value> {
        let mut params = vec![serde_json::json!(self.name)];
        if let Some(filter) = &self.filter {
            params.push(filter.clone());
        }
        params
    }
}

impl CoreBlockchainRpcClient {
    #[must_use]
    pub fn new(chain: Chain, wss_rpc_url: String, proxy_url: Option<String>) -> Self {
        Self {
            chain,
            wss_rpc_url,
            request_id: 1,
            wss_client: None,
            pending_subscription_request: HashMap::new(),
            subscription_event_types: HashMap::new(),
            wss_consumer_rx: None,
            subscriptions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            transport_backend: TransportBackend::default(),
            proxy_url,
        }
    }

    /// Sets the transport backend for the next [`Self::connect`].
    #[must_use]
    pub fn with_transport_backend(mut self, backend: TransportBackend) -> Self {
        self.transport_backend = backend;
        self
    }

    /// Updates the transport backend in place.
    pub fn set_transport_backend(&mut self, backend: TransportBackend) {
        self.transport_backend = backend;
    }

    /// Establishes a WebSocket connection to the blockchain node and sets up the message channel.
    ///
    /// Configures automatic reconnection with exponential backoff and subscription re-establishment.
    /// Reconnection is handled via the `RECONNECTED` message in the message stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let (handler, rx) = channel_message_handler();

        // Most blockchain RPC nodes require a heartbeat to keep the connection alive
        let heartbeat_interval = 30;

        let config = WebSocketConfig {
            url: self.wss_rpc_url.clone(),
            headers: vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())],
            heartbeat: Some(heartbeat_interval),
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(10_000),
            reconnect_delay_initial_ms: Some(1_000),
            reconnect_delay_max_ms: Some(30_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(1_000),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };

        let client =
            WebSocketClient::connect(config, Some(handler), None, None, vec![], None).await?;

        self.wss_client = Some(Arc::new(client));
        self.wss_consumer_rx = Some(rx);

        Ok(())
    }

    /// Registers a subscription for the specified event type.
    async fn subscribe_events(
        &mut self,
        event_type: RpcEventType,
        subscription: RpcSubscription,
    ) -> Result<(), BlockchainRpcClientError> {
        if self.subscriptions.read().await.contains_key(&event_type) {
            return Ok(());
        }

        self.send_subscription_request(event_type, subscription)
            .await
    }

    async fn replace_subscription(
        &mut self,
        event_type: RpcEventType,
        subscription: RpcSubscription,
    ) -> Result<(), BlockchainRpcClientError> {
        self.unsubscribe_event_type(event_type).await?;
        self.send_subscription_request(event_type, subscription)
            .await
    }

    async fn send_subscription_request(
        &mut self,
        event_type: RpcEventType,
        subscription: RpcSubscription,
    ) -> Result<(), BlockchainRpcClientError> {
        if let Some(client) = &self.wss_client {
            log::debug!(
                "Subscribing to '{}' on chain '{}'",
                subscription.name,
                self.chain.name
            );
            let msg = serde_json::json!({
                "method": "eth_subscribe",
                "id": self.request_id,
                "jsonrpc": "2.0",
                "params": subscription.params()
            });
            self.pending_subscription_request
                .insert(self.request_id, event_type);
            self.request_id += 1;

            if let Err(e) = client.send_text(msg.to_string(), None).await {
                log::error!("Error sending subscribe message: {e:?}");
            }

            // Track subscription for re-establishment on reconnect
            let mut confirmed = self.subscriptions.write().await;
            confirmed.insert(event_type, subscription);

            Ok(())
        } else {
            Err(BlockchainRpcClientError::ClientError(String::from(
                "Client not connected",
            )))
        }
    }

    /// Re-establishes all confirmed subscriptions after reconnection.
    async fn resubscribe_all(&mut self) -> Result<(), BlockchainRpcClientError> {
        let subscriptions = self.subscriptions.read().await;

        if subscriptions.is_empty() {
            log::debug!(
                "No subscriptions to re-establish for chain '{}'",
                self.chain.name
            );
            return Ok(());
        }

        log::info!(
            "Re-establishing {} subscription(s) for chain '{}'",
            subscriptions.len(),
            self.chain.name
        );

        let subs_to_restore: Vec<(RpcEventType, RpcSubscription)> = subscriptions
            .iter()
            .map(|(event_type, subscription)| (*event_type, subscription.clone()))
            .collect();

        drop(subscriptions);

        for (event_type, subscription) in subs_to_restore {
            self.send_subscription_request(event_type, subscription)
                .await?;
        }

        Ok(())
    }

    /// Terminates a subscription with the blockchain node using the provided subscription ID.
    async fn unsubscribe_events(
        &self,
        subscription_id: String,
    ) -> Result<(), BlockchainRpcClientError> {
        if let Some(client) = &self.wss_client {
            log::debug!(
                "Unsubscribing from '{}' on chain {}",
                subscription_id,
                self.chain.name
            );
            let msg = serde_json::json!({
                "method": "eth_unsubscribe",
                "id": 1,
                "jsonrpc": "2.0",
                "params": [subscription_id]
            });

            if let Err(e) = client.send_text(msg.to_string(), None).await {
                log::error!("Error sending unsubscribe message: {e:?}");
            }
            Ok(())
        } else {
            Err(BlockchainRpcClientError::ClientError(String::from(
                "Client not connected",
            )))
        }
    }

    async fn unsubscribe_event_type(
        &mut self,
        event_type: RpcEventType,
    ) -> Result<(), BlockchainRpcClientError> {
        let subscription_ids_to_remove: Vec<String> = self
            .subscription_event_types
            .iter()
            .filter(|(_, active_event_type)| **active_event_type == event_type)
            .map(|(id, _)| id.clone())
            .collect();

        for id in subscription_ids_to_remove {
            self.unsubscribe_events(id.clone()).await?;
            self.subscription_event_types.remove(&id);
        }

        self.pending_subscription_request
            .retain(|_, pending_event_type| *pending_event_type != event_type);
        self.subscriptions.write().await.remove(&event_type);

        Ok(())
    }

    /// Waits for and returns the next available message from the WebSocket channel.
    pub async fn wait_on_rpc_channel(&mut self) -> Option<Message> {
        match &mut self.wss_consumer_rx {
            Some(rx) => rx.recv().await,
            None => None,
        }
    }

    /// Retrieves, parses, and returns the next blockchain RPC message as a structured `BlockchainRpcMessage` type.
    ///
    /// Handles subscription confirmations, events, and reconnection signals automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC channel encounters an error or if deserialization of the message fails.
    pub async fn next_rpc_message(
        &mut self,
    ) -> Result<BlockchainMessage, BlockchainRpcClientError> {
        while let Some(msg) = self.wait_on_rpc_channel().await {
            match msg {
                Message::Text(text) => {
                    if text == RECONNECTED {
                        log::info!("Detected reconnection for chain '{}'", self.chain.name);

                        if let Err(e) = self.resubscribe_all().await {
                            log::error!("Failed to re-establish subscriptions: {e:?}");
                        }
                        continue;
                    }

                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(json) => {
                            if is_unsubscribe_confirmation_response(&json) {
                                log::debug!(
                                    "Received unsubscribe confirmation on chain '{}'",
                                    self.chain.name
                                );
                                continue;
                            } else if is_subscription_confirmation_response(&json) {
                                let subscription_request_id = json
                                    .get("id")
                                    .and_then(serde_json::Value::as_u64)
                                    .ok_or_else(|| {
                                        BlockchainRpcClientError::InternalRpcClientError(
                                            "Missing subscription request id".to_string(),
                                        )
                                    })?;
                                let result = json
                                    .get("result")
                                    .and_then(serde_json::Value::as_str)
                                    .ok_or_else(|| {
                                        BlockchainRpcClientError::InternalRpcClientError(
                                            "Missing subscription id".to_string(),
                                        )
                                    })?;
                                let Some(event_type) = self
                                    .pending_subscription_request
                                    .remove(&subscription_request_id)
                                else {
                                    log::debug!(
                                        "Unsubscribing from stale subscription confirmation '{}' on chain '{}'",
                                        result,
                                        self.chain.name
                                    );
                                    self.unsubscribe_events(result.to_string()).await?;
                                    continue;
                                };

                                if self.subscriptions.read().await.contains_key(&event_type) {
                                    self.subscription_event_types
                                        .insert(result.to_string(), event_type);
                                } else {
                                    self.unsubscribe_events(result.to_string()).await?;
                                }
                                continue;
                            } else if is_subscription_event(&json) {
                                let subscription_id = match extract_rpc_subscription_id(&json) {
                                    Some(id) => id,
                                    None => {
                                        return Err(BlockchainRpcClientError::InternalRpcClientError(
                                        "Error parsing subscription id from valid rpc response"
                                            .to_string(),
                                    ));
                                    }
                                };

                                if let Some(event_type) =
                                    self.subscription_event_types.get(subscription_id).copied()
                                {
                                    match event_type {
                                        RpcEventType::NewBlock => {
                                            return match serde_json::from_value::<
                                                RpcNodeWssResponse<Block>,
                                            >(
                                                json
                                            ) {
                                                Ok(block_response) => {
                                                    let block = block_response.params.result;
                                                    Ok(BlockchainMessage::Block(block))
                                                }
                                                Err(e) => Err(
                                                    BlockchainRpcClientError::MessageParsingError(
                                                        format!(
                                                            "Error parsing rpc response to block with error {e}"
                                                        ),
                                                    ),
                                                ),
                                            };
                                        }
                                        RpcEventType::PoolSwap(_)
                                        | RpcEventType::PoolMint(_)
                                        | RpcEventType::PoolBurn(_)
                                        | RpcEventType::PoolCollect(_)
                                        | RpcEventType::PoolFlash(_)
                                        | RpcEventType::PoolFeeProtocolUpdate(_)
                                        | RpcEventType::PoolFeeProtocolCollect(_) => {
                                            let log = Self::parse_rpc_log_response(json)?;

                                            if let Some(message) = self
                                                .blockchain_message_from_pool_log(
                                                    event_type, &log,
                                                )?
                                            {
                                                return Ok(message);
                                            }
                                            continue;
                                        }
                                    }
                                }
                                return Err(BlockchainRpcClientError::InternalRpcClientError(
                                    format!(
                                        "Event type not found for defined subscription id {subscription_id}"
                                    ),
                                ));
                            }
                            return Err(BlockchainRpcClientError::UnsupportedRpcResponseType(
                                json.to_string(),
                            ));
                        }
                        Err(e) => {
                            return Err(BlockchainRpcClientError::MessageParsingError(
                                e.to_string(),
                            ));
                        }
                    }
                }
                Message::Pong(_) => {}
                _ => {
                    return Err(BlockchainRpcClientError::UnsupportedRpcResponseType(
                        msg.to_string(),
                    ));
                }
            }
        }

        Err(BlockchainRpcClientError::NoMessageReceived)
    }

    fn parse_rpc_log_response(json: serde_json::Value) -> Result<RpcLog, BlockchainRpcClientError> {
        serde_json::from_value::<RpcNodeWssResponse<RpcLog>>(json)
            .map(|response| response.params.result)
            .map_err(|e| {
                BlockchainRpcClientError::MessageParsingError(format!(
                    "Error parsing rpc response to log with error {e}"
                ))
            })
    }

    #[cfg(feature = "hypersync")]
    fn blockchain_message_from_pool_log(
        &self,
        event_type: RpcEventType,
        log: &RpcLog,
    ) -> Result<Option<BlockchainMessage>, BlockchainRpcClientError> {
        if log.removed {
            log::debug!(
                "Skipping removed pool log on chain '{}' for event {:?}",
                self.chain.name,
                event_type
            );
            return Ok(None);
        }

        let dex = Self::pool_event_dex(event_type)?;
        let dex_extended = get_dex_extended(self.chain.name, &dex).ok_or_else(|| {
            BlockchainRpcClientError::InternalRpcClientError(format!(
                "DEX {dex} is not registered for chain {}",
                self.chain.name
            ))
        })?;

        match event_type {
            RpcEventType::PoolSwap(_) => dex_extended
                .parse_swap_event_rpc(log)
                .map(BlockchainMessage::SwapEvent),
            RpcEventType::PoolMint(_) => dex_extended
                .parse_mint_event_rpc(log)
                .map(BlockchainMessage::MintEvent),
            RpcEventType::PoolBurn(_) => dex_extended
                .parse_burn_event_rpc(log)
                .map(BlockchainMessage::BurnEvent),
            RpcEventType::PoolCollect(_) => dex_extended
                .parse_collect_event_rpc(log)
                .map(BlockchainMessage::CollectEvent),
            RpcEventType::PoolFlash(_) => dex_extended
                .parse_flash_event_rpc(log)
                .map(BlockchainMessage::FlashEvent),
            RpcEventType::PoolFeeProtocolUpdate(_) => dex_extended
                .parse_fee_protocol_update_event_rpc(log)
                .map(BlockchainMessage::FeeProtocolUpdateEvent),
            RpcEventType::PoolFeeProtocolCollect(_) => dex_extended
                .parse_fee_protocol_collect_event_rpc(log)
                .map(BlockchainMessage::FeeProtocolCollectEvent),
            RpcEventType::NewBlock => Err(anyhow::anyhow!(
                "NewBlock event type cannot parse pool logs"
            )),
        }
        .map(Some)
        .map_err(|e| BlockchainRpcClientError::MessageParsingError(e.to_string()))
    }

    #[cfg(not(feature = "hypersync"))]
    fn blockchain_message_from_pool_log(
        &self,
        event_type: RpcEventType,
        log: &RpcLog,
    ) -> Result<Option<BlockchainMessage>, BlockchainRpcClientError> {
        if log.removed {
            log::debug!(
                "Skipping removed pool log on chain '{}' for event {:?}",
                self.chain.name,
                event_type
            );
            return Ok(None);
        }

        Err(BlockchainRpcClientError::UnsupportedRpcResponseType(
            format!("RPC pool log parsing for {event_type:?} requires the hypersync feature"),
        ))
    }

    #[cfg(feature = "hypersync")]
    fn pool_event_dex(event_type: RpcEventType) -> Result<DexType, BlockchainRpcClientError> {
        match event_type {
            RpcEventType::PoolSwap(dex)
            | RpcEventType::PoolMint(dex)
            | RpcEventType::PoolBurn(dex)
            | RpcEventType::PoolCollect(dex)
            | RpcEventType::PoolFlash(dex)
            | RpcEventType::PoolFeeProtocolUpdate(dex)
            | RpcEventType::PoolFeeProtocolCollect(dex) => Ok(dex),
            RpcEventType::NewBlock => Err(BlockchainRpcClientError::InternalRpcClientError(
                "NewBlock event type has no DEX".to_string(),
            )),
        }
    }

    /// Subscribes to real-time block updates from the blockchain node.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails or if the client is not connected.
    pub async fn subscribe_blocks(&mut self) -> Result<(), BlockchainRpcClientError> {
        self.subscribe_events(RpcEventType::NewBlock, RpcSubscription::new("newHeads"))
            .await
    }

    /// Subscribes to real-time pool logs for one event type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails or if the client is not connected.
    pub async fn subscribe_pool_events(
        &mut self,
        event_type: RpcEventType,
        addresses: &[Address],
        event_signature: String,
    ) -> Result<(), BlockchainRpcClientError> {
        if matches!(event_type, RpcEventType::NewBlock) {
            return Err(BlockchainRpcClientError::InvalidParameters(
                "NewBlock is not a pool event subscription".to_string(),
            ));
        }

        if addresses.is_empty() {
            return self.unsubscribe_event_type(event_type).await;
        }

        self.replace_subscription(
            event_type,
            RpcSubscription::pool_logs(addresses, event_signature),
        )
        .await
    }

    /// Cancels the subscription to real-time block updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails or if the client is not connected.
    pub async fn unsubscribe_blocks(&mut self) -> Result<(), BlockchainRpcClientError> {
        self.unsubscribe_event_type(RpcEventType::NewBlock).await
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::address;
    use nautilus_model::defi::{Chain, DexType};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn pool_logs_subscription_params_use_logs_filter_with_sorted_addresses() {
        let event_signature =
            "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67".to_string();
        let subscription = RpcSubscription::pool_logs(
            &[
                address!("2222222222222222222222222222222222222222"),
                address!("1111111111111111111111111111111111111111"),
            ],
            event_signature.clone(),
        );

        let params = subscription.params();
        let filter = &params[1];

        assert_eq!(params[0], serde_json::json!("logs"));
        assert_eq!(
            filter["address"],
            serde_json::json!([
                "0x1111111111111111111111111111111111111111",
                "0x2222222222222222222222222222222222222222",
            ])
        );
        assert_eq!(filter["topics"], serde_json::json!([event_signature]));
    }

    #[cfg(feature = "hypersync")]
    #[rstest]
    fn pool_event_dex_rejects_block_event_type() {
        assert!(CoreBlockchainRpcClient::pool_event_dex(RpcEventType::NewBlock).is_err());
        assert_eq!(
            CoreBlockchainRpcClient::pool_event_dex(RpcEventType::PoolSwap(DexType::UniswapV3))
                .unwrap(),
            DexType::UniswapV3
        );
    }

    #[rstest]
    fn removed_pool_log_returns_no_message() {
        let client = CoreBlockchainRpcClient::new(
            Chain::from_chain_id(1)
                .expect("Ethereum chain should exist")
                .clone(),
            "ws://127.0.0.1:9".to_string(),
            None,
        );
        let log = RpcLog {
            removed: true,
            log_index: Some("0x0".to_string()),
            transaction_index: Some("0x0".to_string()),
            transaction_hash: Some("0x1".to_string()),
            block_hash: Some("0x1".to_string()),
            block_number: Some("0x1".to_string()),
            address: "0x1111111111111111111111111111111111111111".to_string(),
            data: "0x".to_string(),
            topics: vec![],
        };

        let message = client
            .blockchain_message_from_pool_log(RpcEventType::PoolSwap(DexType::UniswapV3), &log)
            .expect("removed logs should not fail conversion");

        assert!(message.is_none());
    }

    #[tokio::test]
    async fn next_rpc_message_skips_unsubscribe_confirmation() {
        let mut client = CoreBlockchainRpcClient::new(
            Chain::from_chain_id(1)
                .expect("Ethereum chain should exist")
                .clone(),
            "ws://127.0.0.1:9".to_string(),
            None,
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        client.wss_consumer_rx = Some(rx);
        tx.send(Message::Text(
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": true})
                .to_string()
                .into(),
        ))
        .expect("unsubscribe ack should enqueue");
        drop(tx);

        let error = client
            .next_rpc_message()
            .await
            .expect_err("unsubscribe ack should be skipped");

        assert!(matches!(error, BlockchainRpcClientError::NoMessageReceived));
    }
}
