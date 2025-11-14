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

//! Live execution client implementation for the dYdX adapter.

use std::{
    cell::Ref,
    sync::{Mutex, atomic::AtomicU64},
};

use async_trait::async_trait;
use dashmap::DashMap;
use nautilus_common::{
    clock::Clock,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GeneratePositionReports, ModifyOrder, QueryAccount,
            QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
    runner::get_exec_event_sender,
};
use nautilus_core::UnixNanos;
use nautilus_execution::client::{ExecutionClient, LiveExecutionClient, base::ExecutionClientCore};
use nautilus_live::execution::LiveExecutionClientExt;
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    instruments::InstrumentAny,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::consts::DYDX_VENUE,
    config::DydxAdapterConfig,
    // TODO: Re-enable once proto files are generated
    // grpc::{DydxGrpcClient, OrderBuilder, Wallet},
    http::client::DydxRawHttpClient,
    websocket::client::DydxWebSocketClient,
};

/// Maximum client order ID value for dYdX.
pub const MAX_CLIENT_ID: u32 = u32::MAX;

/// Live execution client for the dYdX adapter.
#[derive(Debug)]
#[allow(dead_code)] // TODO: Remove once implementation is complete
pub struct DydxExecutionClient {
    core: ExecutionClientCore,
    config: DydxAdapterConfig,
    http_client: DydxRawHttpClient,
    ws_client: DydxWebSocketClient,
    // TODO: Re-enable once proto files are generated
    // grpc_client: Arc<tokio::sync::RwLock<DydxGrpcClient>>,
    // wallet: Arc<tokio::sync::RwLock<Option<Wallet>>>,
    // order_builders: DashMap<InstrumentId, OrderBuilder>,
    instruments: DashMap<InstrumentId, InstrumentAny>,
    block_height: AtomicU64,
    oracle_prices: DashMap<InstrumentId, Decimal>,
    client_id_to_int: DashMap<String, u32>,
    int_to_client_id: DashMap<u32, String>,
    wallet_address: String,
    subaccount_number: u32,
    started: bool,
    connected: bool,
    instruments_initialized: bool,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl DydxExecutionClient {
    /// Creates a new [`DydxExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket client fails to construct.
    pub fn new(
        core: ExecutionClientCore,
        config: DydxAdapterConfig,
        wallet_address: String,
        subaccount_number: u32,
    ) -> anyhow::Result<Self> {
        let http_client = DydxRawHttpClient::default();

        let _account_id = core.account_id;
        let ws_client = DydxWebSocketClient::new_public(config.ws_url.clone(), Some(20));

        // TODO: Re-enable once proto files are generated
        // let grpc_urls = config.get_grpc_urls();
        // let grpc_client = Arc::new(tokio::sync::RwLock::new(
        //     get_runtime()
        //         .block_on(async { DydxGrpcClient::new_with_fallback(&grpc_urls).await })
        //         .context("failed to construct dYdX gRPC client")?,
        // ));

        Ok(Self {
            core,
            config,
            http_client,
            ws_client,
            // TODO: Re-enable once proto files are generated
            // grpc_client,
            // wallet: Arc::new(tokio::sync::RwLock::new(None)),
            // order_builders: DashMap::new(),
            instruments: DashMap::new(),
            block_height: AtomicU64::new(0),
            oracle_prices: DashMap::new(),
            client_id_to_int: DashMap::new(),
            int_to_client_id: DashMap::new(),
            wallet_address,
            subaccount_number,
            started: false,
            connected: false,
            instruments_initialized: false,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    /// Generate a unique client order ID integer and store the mapping.
    ///
    /// Attempts to parse the client_order_id as an integer first. If that fails,
    /// generates a random value within the valid range [0, MAX_CLIENT_ID).
    #[allow(dead_code)] // TODO: Remove once used in submit_order
    fn generate_client_order_id_int(&self, client_order_id: &str) -> u32 {
        // Try to parse as integer first
        if let Ok(id) = client_order_id.parse::<u32>() {
            self.client_id_to_int
                .insert(client_order_id.to_string(), id);
            self.int_to_client_id
                .insert(id, client_order_id.to_string());
            return id;
        }

        // Generate random value if parsing fails
        let id = rand::random::<u32>();
        self.client_id_to_int
            .insert(client_order_id.to_string(), id);
        self.int_to_client_id
            .insert(id, client_order_id.to_string());
        id
    }

    /// Retrieve the client order ID integer from the cache.
    ///
    /// Returns `None` if the mapping doesn't exist.
    #[allow(dead_code)] // TODO: Remove once used in cancel_order
    fn get_client_order_id_int(&self, client_order_id: &str) -> Option<u32> {
        // Try parsing first
        if let Ok(id) = client_order_id.parse::<u32>() {
            return Some(id);
        }

        // Look up in cache
        self.client_id_to_int
            .get(client_order_id)
            .map(|entry| *entry.value())
    }

    /// Retrieve the client order ID string from the integer value.
    ///
    /// Returns the integer as a string if no mapping exists.
    #[allow(dead_code)] // TODO: Remove once used in handle_order_message
    fn get_client_order_id(&self, client_order_id_int: u32) -> String {
        self.int_to_client_id.get(&client_order_id_int).map_or_else(
            || client_order_id_int.to_string(),
            |entry| entry.value().clone(),
        )
    }
}

impl ExecutionClient for DydxExecutionClient {
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
        *DYDX_VENUE
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
    ) -> anyhow::Result<()> {
        self.core
            .generate_account_state(balances, margins, reported, ts_event)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.started {
            return Ok(());
        }
        self.started = false;
        self.connected = false;
        Ok(())
    }

    fn submit_order(&self, _cmd: &SubmitOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn submit_order_list(&self, _cmd: &SubmitOrderList) -> anyhow::Result<()> {
        Ok(())
    }

    fn modify_order(&self, _cmd: &ModifyOrder) -> anyhow::Result<()> {
        anyhow::bail!("modify_order not supported by dYdX")
    }

    fn cancel_order(&self, _cmd: &CancelOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn cancel_all_orders(&self, _cmd: &CancelAllOrders) -> anyhow::Result<()> {
        Ok(())
    }

    fn batch_cancel_orders(&self, _cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        Ok(())
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        Ok(())
    }

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait(?Send)]
impl LiveExecutionClient for DydxExecutionClient {
    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected {
            return Ok(());
        }

        // TODO: Implement WebSocket connection and subscriptions
        // - Load instruments
        // - Connect WebSocket
        // - Subscribe to v4_markets, v4_block_height, v4_subaccounts
        // - Initialize wallet from GRPC
        // - Set leverage for all instruments

        self.connected = true;
        tracing::info!("{} connected", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.connected {
            return Ok(());
        }

        // TODO: Implement WebSocket disconnection
        // - Unsubscribe from v4_markets, v4_block_height, v4_subaccounts
        // - Disconnect WebSocket and GRPC clients
        // - Clean up ws_stream_handle and pending_tasks

        self.connected = false;
        tracing::info!("{} disconnected", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        Ok(None)
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        Ok(Vec::new())
    }

    async fn generate_fill_reports(
        &self,
        _cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        Ok(Vec::new())
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        Ok(Vec::new())
    }

    async fn generate_mass_status(
        &self,
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        Ok(None)
    }
}

impl LiveExecutionClientExt for DydxExecutionClient {
    fn get_message_channel(&self) -> tokio::sync::mpsc::UnboundedSender<ExecutionEvent> {
        get_exec_event_sender()
    }

    fn get_clock(&self) -> Ref<'_, dyn Clock> {
        self.core.clock().borrow()
    }
}
