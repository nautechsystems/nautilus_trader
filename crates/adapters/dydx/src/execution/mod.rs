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
    instruments::{Instrument, InstrumentAny},
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
    market_to_instrument: DashMap<String, InstrumentId>,
    clob_pair_id_to_instrument: DashMap<u32, InstrumentId>,
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
            market_to_instrument: DashMap::new(),
            clob_pair_id_to_instrument: DashMap::new(),
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

    /// Get an instrument by market ticker (e.g., "BTC-USD").
    fn get_instrument_by_market(&self, market: &str) -> Option<InstrumentAny> {
        self.market_to_instrument
            .get(market)
            .and_then(|id| self.instruments.get(&id).map(|entry| entry.value().clone()))
    }

    /// Get an instrument by clob_pair_id.
    fn get_instrument_by_clob_pair_id(&self, clob_pair_id: u32) -> Option<InstrumentAny> {
        self.clob_pair_id_to_instrument
            .get(&clob_pair_id)
            .and_then(|id| self.instruments.get(&id).map(|entry| entry.value().clone()))
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
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        use anyhow::Context;

        // Query single order from dYdX API
        let response = self
            .http_client
            .get_orders(
                &self.wallet_address,
                self.subaccount_number,
                None,    // market filter
                Some(1), // limit to 1 result
            )
            .await
            .context("failed to fetch order from dYdX API")?;

        if response.orders.is_empty() {
            return Ok(None);
        }

        let order = &response.orders[0];
        let ts_init = UnixNanos::default();

        // Get instrument by clob_pair_id
        let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
            Some(inst) => inst,
            None => {
                tracing::warn!(
                    "Instrument for clob_pair_id {} not found in cache",
                    order.clob_pair_id
                );
                return Ok(None);
            }
        };

        // Parse to OrderStatusReport
        let report = crate::http::parse::parse_order_status_report(
            order,
            &instrument,
            self.core.account_id,
            ts_init,
        )
        .context("failed to parse order status report")?;

        // Filter by client_order_id if specified
        if let Some(client_order_id) = cmd.client_order_id
            && report.client_order_id != Some(client_order_id)
        {
            return Ok(None);
        }

        // Filter by venue_order_id if specified
        if let Some(venue_order_id) = cmd.venue_order_id
            && report.venue_order_id.as_str() != venue_order_id.as_str()
        {
            return Ok(None);
        }

        // Filter by instrument_id if specified
        if let Some(instrument_id) = cmd.instrument_id
            && report.instrument_id != instrument_id
        {
            return Ok(None);
        }

        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        use anyhow::Context;

        // Query orders from dYdX API
        let response = self
            .http_client
            .get_orders(
                &self.wallet_address,
                self.subaccount_number,
                None, // market filter
                None, // limit
            )
            .await
            .context("failed to fetch orders from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        for order in response.orders {
            // Get instrument by clob_pair_id using efficient lookup
            let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
                Some(inst) => inst,
                None => {
                    tracing::warn!(
                        "Instrument for clob_pair_id {} not found in cache, skipping order {}",
                        order.clob_pair_id,
                        order.id
                    );
                    continue;
                }
            };

            // Filter by instrument_id if specified
            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            // Parse to OrderStatusReport
            match crate::http::parse::parse_order_status_report(
                &order,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(report) => {
                    // Filter by client_order_id if specified
                    if let Some(client_order_id) = cmd.client_order_id
                        && report.client_order_id != Some(client_order_id)
                    {
                        continue;
                    }

                    // Filter by venue_order_id if specified
                    if let Some(venue_order_id) = cmd.venue_order_id
                        && report.venue_order_id.as_str() != venue_order_id.as_str()
                    {
                        continue;
                    }

                    reports.push(report);
                }
                Err(e) => tracing::error!("Failed to parse order status report: {e}"),
            }
        }

        tracing::info!("Generated {} order status reports", reports.len());
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        use anyhow::Context;

        // Query fills from dYdX API
        let response = self
            .http_client
            .get_fills(
                &self.wallet_address,
                self.subaccount_number,
                None, // market filter
                None, // limit
            )
            .await
            .context("failed to fetch fills from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        for fill in response.fills {
            // Get instrument by market ticker using efficient lookup
            let instrument = match self.get_instrument_by_market(&fill.market) {
                Some(inst) => inst,
                None => {
                    tracing::warn!(
                        "Instrument for market {} not found in cache, skipping fill {}",
                        fill.market,
                        fill.id
                    );
                    continue;
                }
            };

            // Filter by instrument_id if specified
            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            // Parse to FillReport
            match crate::http::parse::parse_fill_report(
                &fill,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(report) => {
                    // Filter by venue_order_id if specified
                    if let Some(venue_order_id) = cmd.venue_order_id
                        && report.venue_order_id.as_str() != venue_order_id.as_str()
                    {
                        continue;
                    }

                    // Filter by time range if specified
                    if let (Some(start), Some(end)) = (cmd.start, cmd.end) {
                        if report.ts_event >= start && report.ts_event <= end {
                            reports.push(report);
                        }
                    } else if let Some(start) = cmd.start {
                        if report.ts_event >= start {
                            reports.push(report);
                        }
                    } else if let Some(end) = cmd.end {
                        if report.ts_event <= end {
                            reports.push(report);
                        }
                    } else {
                        reports.push(report);
                    }
                }
                Err(e) => tracing::error!("Failed to parse fill report: {e}"),
            }
        }

        tracing::info!("Generated {} fill reports", reports.len());
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        use anyhow::Context;

        // Query subaccount data from dYdX API to get positions
        let response = self
            .http_client
            .get_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .context("failed to fetch subaccount from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        // Iterate through open perpetual positions
        for (market_ticker, position) in response.subaccount.open_perpetual_positions {
            // Get instrument by market ticker using efficient lookup
            let instrument = match self.get_instrument_by_market(&market_ticker) {
                Some(inst) => inst,
                None => {
                    tracing::warn!(
                        "Instrument for market {} not found in cache, skipping position",
                        market_ticker
                    );
                    continue;
                }
            };

            // Filter by instrument_id if specified
            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            // Parse to PositionStatusReport
            match crate::http::parse::parse_position_status_report(
                &position,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    tracing::error!("Failed to parse position status report: {e}");
                }
            }
        }

        tracing::info!("Generated {} position status reports", reports.len());
        Ok(reports)
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
