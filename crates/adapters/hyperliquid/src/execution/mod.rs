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

use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use nautilus_common::{
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
        SubmitOrder, SubmitOrderList,
    },
    runtime::get_runtime,
};
use nautilus_core::UnixNanos;
use nautilus_execution::client::{ExecutionClient, base::ExecutionClientCore};
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    identifiers::{AccountId, ClientId, Venue},
    orders::Order,
    types::{AccountBalance, MarginBalance},
};
use serde_json;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::{
    common::{
        consts::HYPERLIQUID_VENUE, credential::Secrets, parse::order_any_to_hyperliquid_request,
    },
    config::HyperliquidExecClientConfig,
    http::{client::HyperliquidHttpClient, query::ExchangeAction},
    websocket::client::HyperliquidWebSocketClient,
};

#[derive(Debug)]
pub struct HyperliquidExecutionClient {
    core: ExecutionClientCore,
    config: HyperliquidExecClientConfig,
    http_client: HyperliquidHttpClient,
    ws_client: HyperliquidWebSocketClient,
    started: bool,
    connected: bool,
    instruments_initialized: bool,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl HyperliquidExecutionClient {
    /// Returns a reference to the configuration.
    pub fn config(&self) -> &HyperliquidExecClientConfig {
        &self.config
    }

    /// Returns true if the client is configured for testnet.
    pub fn is_testnet(&self) -> bool {
        self.config.is_testnet
    }

    /// Returns the HTTP timeout configuration in seconds.
    pub fn http_timeout_secs(&self) -> u64 {
        self.config.http_timeout_secs
    }

    /// Returns the maximum number of HTTP retry attempts.
    pub fn max_retries(&self) -> u32 {
        self.config.max_retries
    }

    /// Returns the vault address if configured.
    pub fn vault_address(&self) -> Option<&str> {
        self.config.vault_address.as_deref()
    }

    /// Returns the WebSocket URL from config.
    pub fn ws_url(&self) -> String {
        self.config.ws_url()
    }

    /// Returns the HTTP URL from config.
    pub fn http_url(&self) -> String {
        self.config.http_url()
    }

    /// Creates a new [`HyperliquidExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if either the HTTP or WebSocket client fail to construct.
    pub fn new(core: ExecutionClientCore, config: HyperliquidExecClientConfig) -> Result<Self> {
        if !config.has_credentials() {
            bail!("Hyperliquid execution client requires private key");
        }

        let secrets = Secrets::from_json(&format!(
            r#"{{"privateKey": "{}", "isTestnet": {}}}"#,
            config.private_key, config.is_testnet
        ))
        .context("failed to create secrets from private key")?;

        let http_client =
            HyperliquidHttpClient::with_credentials(&secrets, Some(config.http_timeout_secs));

        // Create WebSocket client (will connect when needed)
        let ws_client = HyperliquidWebSocketClient::new(
            crate::common::consts::ws_url(config.is_testnet).to_string(),
        );

        Ok(Self {
            core,
            config,
            http_client,
            ws_client,
            started: false,
            connected: false,
            instruments_initialized: false,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    async fn ensure_instruments_initialized_async(&mut self) -> Result<()> {
        if self.instruments_initialized {
            return Ok(());
        }

        let instruments = self
            .http_client
            .request_instruments()
            .await
            .context("failed to request Hyperliquid instruments")?;

        if instruments.is_empty() {
            warn!("Instrument bootstrap yielded no instruments; WebSocket submissions may fail");
        } else {
            info!("Initialized {} instruments", instruments.len());
        }

        self.instruments_initialized = true;
        Ok(())
    }

    fn ensure_instruments_initialized(&mut self) -> Result<()> {
        if self.instruments_initialized {
            return Ok(());
        }

        let runtime = get_runtime();
        runtime.block_on(self.ensure_instruments_initialized_async())
    }

    async fn refresh_account_state(&self) -> Result<()> {
        // Get account information from Hyperliquid using the user address
        // We need to derive the user address from the private key in the config
        let user_address = self.get_user_address()?;

        // Use vault address if configured, otherwise use user address
        let account_address = self.config.vault_address.as_ref().unwrap_or(&user_address);

        // Query userState endpoint to get balances and margin info
        let user_state_request = crate::http::query::InfoRequest {
            request_type: "clearinghouseState".to_string(),
            params: serde_json::json!({ "user": account_address }),
        };

        match self
            .http_client
            .send_info_request_raw(&user_state_request)
            .await
        {
            Ok(response) => {
                debug!("Received user state: {:?}", response);
                // TODO: Parse the response and convert to Nautilus AccountBalance/MarginBalance
                // For now, just log that we received the data
                Ok(())
            }
            Err(e) => {
                warn!("Failed to refresh account state: {}", e);
                Err(e.into())
            }
        }
    }

    fn get_user_address(&self) -> Result<String> {
        // For now, use a placeholder. In a real implementation, we would
        // derive the Ethereum address from the private key in the config
        // TODO: Implement proper address derivation from private key
        let placeholder_address = if self.config.is_testnet {
            format!("0x{}", "1".repeat(40)) // Testnet placeholder
        } else {
            format!("0x{}", "0".repeat(40)) // Mainnet placeholder
        };
        Ok(placeholder_address)
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(err) = fut.await {
                warn!("{description} failed: {err:?}");
            }
        });

        let mut tasks = self.pending_tasks.lock().unwrap();
        tasks.retain(|handle| !handle.is_finished());
        tasks.push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().unwrap();
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    fn update_account_state(&self) -> Result<()> {
        let runtime = get_runtime();
        runtime.block_on(self.refresh_account_state())
    }
}

impl ExecutionClient for HyperliquidExecutionClient {
    fn is_connected(&self) -> bool {
        self.connected
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *HYPERLIQUID_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.get_account()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> Result<()> {
        self.core
            .generate_account_state(balances, margins, reported, ts_event)
    }

    fn start(&mut self) -> Result<()> {
        if self.started {
            return Ok(());
        }

        info!("Starting Hyperliquid execution client");

        // Ensure instruments are initialized
        self.ensure_instruments_initialized()?;

        // Initialize account state
        if let Err(e) = self.update_account_state() {
            warn!("Failed to initialize account state: {}", e);
        }

        // TODO: Start WebSocket connection for real-time updates
        if !self.ws_client.is_connected() {
            debug!("WebSocket client ready, will connect when needed");
        } else {
            debug!("WebSocket client already connected");
        }

        self.connected = true;
        self.started = true;

        info!("Hyperliquid execution client started");
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        if !self.started {
            return Ok(());
        }

        info!("Stopping Hyperliquid execution client");

        // Abort any pending tasks
        self.abort_pending_tasks();

        // TODO: Disconnect WebSocket

        self.connected = false;
        self.started = false;

        info!("Hyperliquid execution client stopped");
        Ok(())
    }

    fn submit_order(&self, command: &SubmitOrder) -> Result<()> {
        let order = &command.order;

        if order.is_closed() {
            warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        self.core.generate_order_submitted(
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            command.ts_init,
        );

        let http_client = self.http_client.clone();
        let order_clone = order.clone();

        self.spawn_task("submit_order", async move {
            match order_any_to_hyperliquid_request(&order_clone) {
                Ok(hyperliquid_order) => {
                    // Convert single order to JSON array format for the exchange action
                    match serde_json::to_value(vec![hyperliquid_order]) {
                        Ok(orders_json) => {
                            // Create exchange action for order placement
                            let action = ExchangeAction::order(orders_json);

                            match http_client.post_action(&action).await {
                                Ok(response) => {
                                    info!("Order submitted successfully: {:?}", response);
                                    // TODO: Parse response and generate appropriate order events
                                    // For now, we'll generate a basic accepted event
                                    // In a full implementation, you'd parse the response to determine
                                    // if the order was accepted, rejected, or partially filled
                                }
                                Err(err) => {
                                    warn!("Order submission failed: {:?}", err);
                                    // TODO: Generate order rejected event
                                }
                            }
                        }
                        Err(err) => {
                            warn!("Failed to serialize order to JSON: {:?}", err);
                            // TODO: Generate order rejected event
                        }
                    }
                }
                Err(err) => {
                    warn!("Failed to convert order to Hyperliquid format: {:?}", err);
                    // TODO: Generate order rejected event
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, command: &SubmitOrderList) -> Result<()> {
        debug!("Submitting order list: {:?}", command);
        // TODO: Implement batch order submission
        warn!("Order list submission not yet implemented");
        Ok(())
    }

    fn modify_order(&self, command: &ModifyOrder) -> Result<()> {
        debug!("Modifying order: {:?}", command);
        // TODO: Implement order modification
        warn!("Order modification not yet implemented");
        Ok(())
    }

    fn cancel_order(&self, command: &CancelOrder) -> Result<()> {
        debug!("Cancelling order: {:?}", command);
        // TODO: Implement order cancellation
        warn!("Order cancellation not yet implemented");
        Ok(())
    }

    fn cancel_all_orders(&self, command: &CancelAllOrders) -> Result<()> {
        debug!("Cancelling all orders: {:?}", command);
        // TODO: Implement cancel all orders
        warn!("Cancel all orders not yet implemented");
        Ok(())
    }

    fn batch_cancel_orders(&self, command: &BatchCancelOrders) -> Result<()> {
        debug!("Batch cancelling orders: {:?}", command);
        // TODO: Implement batch order cancellation
        warn!("Batch cancel orders not yet implemented");
        Ok(())
    }

    fn query_account(&self, command: &QueryAccount) -> Result<()> {
        debug!("Querying account: {:?}", command);
        // TODO: Implement account query
        warn!("Account query not yet implemented");
        Ok(())
    }

    fn query_order(&self, command: &QueryOrder) -> Result<()> {
        debug!("Querying order: {:?}", command);
        // TODO: Implement order query
        warn!("Order query not yet implemented");
        Ok(())
    }
}

// Re-export execution models from the http module
pub use crate::http::models::{
    AssetId, Cloid, HyperliquidExecAction, HyperliquidExecBuilderFee,
    HyperliquidExecCancelByCloidRequest, HyperliquidExecCancelOrderRequest,
    HyperliquidExecCancelResponseData, HyperliquidExecCancelStatus, HyperliquidExecFilledInfo,
    HyperliquidExecGrouping, HyperliquidExecLimitParams, HyperliquidExecModifyOrderRequest,
    HyperliquidExecModifyResponseData, HyperliquidExecModifyStatus, HyperliquidExecOrderKind,
    HyperliquidExecOrderResponseData, HyperliquidExecOrderStatus, HyperliquidExecPlaceOrderRequest,
    HyperliquidExecRequest, HyperliquidExecResponse, HyperliquidExecResponseData,
    HyperliquidExecRestingInfo, HyperliquidExecTif, HyperliquidExecTpSl,
    HyperliquidExecTriggerParams, HyperliquidExecTwapRequest, OrderId,
};
