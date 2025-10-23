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

//! Live execution client implementation for the BitMEX adapter.

pub mod canceller;

use std::{any::Any, cell::Ref, future::Future, sync::Mutex};

use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
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
    msgbus,
    runner::get_exec_event_sender,
    runtime::get_runtime,
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_execution::client::{ExecutionClient, LiveExecutionClient, base::ExecutionClientCore};
use nautilus_live::execution::LiveExecutionClientExt;
use nautilus_model::{
    events::{AccountState, OrderEventAny, OrderRejected},
    identifiers::{AccountId, VenueOrderId},
    instruments::Instrument,
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};
use tokio::task::JoinHandle;

use crate::{
    config::BitmexExecClientConfig,
    http::client::BitmexHttpClient,
    websocket::{client::BitmexWebSocketClient, messages::NautilusWsMessage},
};

#[derive(Debug)]
pub struct BitmexExecutionClient {
    core: ExecutionClientCore,
    config: BitmexExecClientConfig,
    http_client: BitmexHttpClient,
    ws_client: BitmexWebSocketClient,
    started: bool,
    connected: bool,
    instruments_initialized: bool,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl BitmexExecutionClient {
    /// Creates a new [`BitmexExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if either the HTTP or WebSocket client fail to construct.
    pub fn new(core: ExecutionClientCore, config: BitmexExecClientConfig) -> anyhow::Result<Self> {
        if !config.has_api_credentials() {
            anyhow::bail!("BitMEX execution client requires API key and secret");
        }

        let http_client = BitmexHttpClient::new(
            Some(config.http_base_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.use_testnet,
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.recv_window_ms,
            config.max_requests_per_second,
            config.max_requests_per_minute,
        )
        .context("failed to construct BitMEX HTTP client")?;

        let account_id = config.account_id.unwrap_or(core.account_id);
        let ws_client = BitmexWebSocketClient::new(
            Some(config.ws_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            Some(account_id),
            config.heartbeat_interval_secs,
        )
        .context("failed to construct BitMEX execution websocket client")?;

        Ok(Self {
            core,
            config,
            http_client,
            ws_client,
            started: false,
            connected: false,
            instruments_initialized: false,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    fn spawn_task<F>(&self, label: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handle = tokio::spawn(async move {
            if let Err(e) = fut.await {
                tracing::error!("{label}: {e:?}");
            }
        });

        self.pending_tasks
            .lock()
            .expect("pending task lock poisoned")
            .push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut guard = self
            .pending_tasks
            .lock()
            .expect("pending task lock poisoned");
        for handle in guard.drain(..) {
            handle.abort();
        }
    }

    async fn ensure_instruments_initialized_async(&mut self) -> anyhow::Result<()> {
        if self.instruments_initialized {
            return Ok(());
        }

        let http = self.http_client.clone();
        let mut instruments = http
            .request_instruments(self.config.active_only)
            .await
            .context("failed to request BitMEX instruments")?;

        instruments.sort_by_key(|instrument| instrument.id());

        for instrument in &instruments {
            self.http_client.add_instrument(instrument.clone());
        }

        self.ws_client.initialize_instruments_cache(instruments);

        self.instruments_initialized = true;
        Ok(())
    }

    fn ensure_instruments_initialized(&mut self) -> anyhow::Result<()> {
        if self.instruments_initialized {
            return Ok(());
        }

        let runtime = get_runtime();
        runtime.block_on(self.ensure_instruments_initialized_async())
    }

    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request BitMEX account state")?;

        dispatch_account_state(account_state);
        Ok(())
    }

    fn update_account_state(&self) -> anyhow::Result<()> {
        let runtime = get_runtime();
        runtime.block_on(self.refresh_account_state())
    }

    fn start_ws_stream(&mut self) -> anyhow::Result<()> {
        if self.ws_stream_handle.is_some() {
            return Ok(());
        }

        let stream = self.ws_client.stream();
        let handle = tokio::spawn(async move {
            pin_mut!(stream);
            while let Some(message) = stream.next().await {
                dispatch_ws_message(message);
            }
        });

        self.ws_stream_handle = Some(handle);
        Ok(())
    }
}

impl ExecutionClient for BitmexExecutionClient {
    fn is_connected(&self) -> bool {
        self.connected
    }

    fn client_id(&self) -> nautilus_model::identifiers::ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> nautilus_model::identifiers::Venue {
        self.core.venue
    }

    fn oms_type(&self) -> nautilus_model::enums::OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<nautilus_model::accounts::AccountAny> {
        self.core.get_account()
    }

    fn generate_account_state(
        &self,
        balances: Vec<nautilus_model::types::AccountBalance>,
        margins: Vec<nautilus_model::types::MarginBalance>,
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

        self.ensure_instruments_initialized()?;
        self.started = true;
        tracing::info!("BitMEX execution client {} started", self.core.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.started {
            return Ok(());
        }

        self.started = false;
        self.connected = false;
        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }
        self.abort_pending_tasks();
        tracing::info!("BitMEX execution client {} stopped", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = cmd.order.clone();

        if order.is_closed() {
            tracing::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        self.core.generate_order_submitted(
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            cmd.ts_init,
        );

        let http_client = self.http_client.clone();
        let trader_id = self.core.trader_id;
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let account_id = self.core.account_id;
        let client_order_id = order.client_order_id();
        let order_side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let time_in_force = order.time_in_force();
        let price = order.price();
        let trigger_price = order.trigger_price();
        let trigger_type = order.trigger_type();
        let display_qty = order.display_qty();
        let post_only = order.is_post_only();
        let reduce_only = order.is_reduce_only();
        let order_list_id = order.order_list_id();
        let contingency_type = order.contingency_type();
        let ts_event = cmd.ts_init;

        self.spawn_task("submit_order", async move {
            match http_client
                .submit_order(
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    trigger_type,
                    display_qty,
                    post_only,
                    reduce_only,
                    order_list_id,
                    contingency_type,
                )
                .await
            {
                Ok(report) => dispatch_order_status_report(report),
                Err(e) => {
                    let event = OrderRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        account_id,
                        format!("submit-order-error: {e}").into(),
                        UUID4::new(),
                        ts_event,
                        get_atomic_clock_realtime().get_time_ns(),
                        false,
                        post_only,
                    );
                    dispatch_order_event(OrderEventAny::Rejected(event));
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        tracing::warn!(
            "submit_order_list not yet implemented for BitMEX execution client ({} orders)",
            cmd.order_list.orders.len()
        );
        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let instrument_id = cmd.instrument_id;
        let client_order_id = Some(cmd.client_order_id);
        let venue_order_id = Some(cmd.venue_order_id);
        let quantity = cmd.quantity;
        let price = cmd.price;
        let trigger_price = cmd.trigger_price;

        self.spawn_task("modify_order", async move {
            match http_client
                .modify_order(
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    quantity,
                    price,
                    trigger_price,
                )
                .await
            {
                Ok(report) => dispatch_order_status_report(report),
                Err(e) => tracing::error!("BitMEX modify order failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let instrument_id = cmd.instrument_id;
        let client_order_id = Some(cmd.client_order_id);
        let venue_order_id = Some(cmd.venue_order_id);

        self.spawn_task("cancel_order", async move {
            match http_client
                .cancel_order(instrument_id, client_order_id, venue_order_id)
                .await
            {
                Ok(report) => dispatch_order_status_report(report),
                Err(e) => tracing::error!("BitMEX cancel order failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let instrument_id = cmd.instrument_id;
        let order_side = Some(cmd.order_side);

        self.spawn_task("cancel_all_orders", async move {
            match http_client
                .cancel_all_orders(instrument_id, order_side)
                .await
            {
                Ok(reports) => {
                    for report in reports {
                        dispatch_order_status_report(report);
                    }
                }
                Err(e) => tracing::error!("BitMEX cancel all failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let instrument_id = cmd.instrument_id;
        let venue_ids: Vec<VenueOrderId> = cmd
            .cancels
            .iter()
            .map(|cancel| cancel.venue_order_id)
            .collect();

        self.spawn_task("batch_cancel_orders", async move {
            match http_client
                .cancel_orders(instrument_id, None, Some(venue_ids))
                .await
            {
                Ok(reports) => {
                    for report in reports {
                        dispatch_order_status_report(report);
                    }
                }
                Err(e) => tracing::error!("BitMEX batch cancel failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        self.update_account_state()
    }

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let instrument_id = cmd.instrument_id;
        let client_order_id = Some(cmd.client_order_id);
        let venue_order_id = Some(cmd.venue_order_id);

        self.spawn_task("query_order", async move {
            match http_client
                .request_order_status_report(instrument_id, client_order_id, venue_order_id)
                .await
            {
                Ok(report) => dispatch_order_status_report(report),
                Err(e) => tracing::error!("BitMEX query order failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }
}

#[async_trait(?Send)]
impl LiveExecutionClient for BitmexExecutionClient {
    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected {
            return Ok(());
        }

        self.ensure_instruments_initialized_async().await?;

        self.ws_client.connect().await?;
        self.ws_client.wait_until_active(10.0).await?;

        self.ws_client.subscribe_orders().await?;
        self.ws_client.subscribe_executions().await?;
        self.ws_client.subscribe_positions().await?;
        self.ws_client.subscribe_wallet().await?;
        if let Err(e) = self.ws_client.subscribe_margin().await {
            tracing::debug!("Margin subscription unavailable: {e:?}");
        }

        self.start_ws_stream()?;
        self.refresh_account_state().await?;

        self.connected = true;
        self.core.set_connected(true);
        tracing::info!("BitMEX execution client {} connected", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.connected {
            return Ok(());
        }

        self.http_client.cancel_all_requests();
        if let Err(e) = self.ws_client.close().await {
            tracing::warn!("Error while closing BitMEX execution websocket: {e:?}");
        }

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }

        self.abort_pending_tasks();
        self.connected = false;
        self.core.set_connected(false);
        tracing::info!(
            "BitMEX execution client {} disconnected",
            self.core.client_id
        );
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let instrument_id = cmd
            .instrument_id
            .context("BitMEX generate_order_status_report requires an instrument identifier")?;

        self.http_client
            .query_order(
                instrument_id,
                cmd.client_order_id,
                cmd.venue_order_id.map(|id| VenueOrderId::from(id.as_str())),
            )
            .await
            .context("failed to query BitMEX order status")
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let reports = self
            .http_client
            .request_order_status_reports(cmd.instrument_id, false, None)
            .await
            .context("failed to request BitMEX order status reports")?;
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let mut reports = self
            .http_client
            .request_fill_reports(cmd.instrument_id, None)
            .await
            .context("failed to request BitMEX fill reports")?;

        if let Some(order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == order_id.as_str());
        }

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let mut reports = self
            .http_client
            .request_position_status_reports()
            .await
            .context("failed to request BitMEX position reports")?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        tracing::warn!("generate_mass_status not yet implemented for BitMEX execution client");
        Ok(None)
    }
}

fn dispatch_ws_message(message: NautilusWsMessage) {
    match message {
        NautilusWsMessage::OrderStatusReports(reports) => {
            for report in reports {
                dispatch_order_status_report(report);
            }
        }
        NautilusWsMessage::FillReports(reports) => {
            for report in reports {
                dispatch_fill_report(report);
            }
        }
        NautilusWsMessage::PositionStatusReport(report) => {
            dispatch_position_status_report(report);
        }
        NautilusWsMessage::AccountState(state) => dispatch_account_state(state),
        NautilusWsMessage::OrderUpdated(event) => {
            dispatch_order_event(OrderEventAny::Updated(event));
        }
        NautilusWsMessage::Data(_) | NautilusWsMessage::FundingRateUpdates(_) => {
            tracing::debug!("Ignoring BitMEX data message on execution stream");
        }
        NautilusWsMessage::Reconnected => {
            tracing::info!("BitMEX execution websocket reconnected");
        }
    }
}

fn dispatch_account_state(state: AccountState) {
    msgbus::send_any("Portfolio.update_account".into(), &state as &dyn Any);
}

fn dispatch_order_status_report(report: OrderStatusReport) {
    let sender = get_exec_event_sender();
    let exec_report = nautilus_common::messages::ExecutionReport::OrderStatus(Box::new(report));
    if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
        tracing::warn!("Failed to send order status report: {e}");
    }
}

fn dispatch_fill_report(report: FillReport) {
    let sender = get_exec_event_sender();
    let exec_report = nautilus_common::messages::ExecutionReport::Fill(Box::new(report));
    if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
        tracing::warn!("Failed to send fill report: {e}");
    }
}

fn dispatch_position_status_report(report: PositionStatusReport) {
    let sender = get_exec_event_sender();
    let exec_report = nautilus_common::messages::ExecutionReport::Position(Box::new(report));
    if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
        tracing::warn!("Failed to send position status report: {e}");
    }
}

fn dispatch_order_event(event: OrderEventAny) {
    let sender = get_exec_event_sender();
    if let Err(e) = sender.send(ExecutionEvent::Order(event)) {
        tracing::warn!("Failed to send order event: {e}");
    }
}

impl LiveExecutionClientExt for BitmexExecutionClient {
    fn get_message_channel(&self) -> tokio::sync::mpsc::UnboundedSender<ExecutionEvent> {
        get_exec_event_sender()
    }

    fn get_clock(&self) -> Ref<'_, dyn Clock> {
        self.core.clock().borrow()
    }
}
