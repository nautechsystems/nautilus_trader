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

//! Live execution client implementation for the OKX adapter.

use std::{cell::Ref, future::Future, sync::Mutex};

use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clock::Clock, messages::ExecutionEvent, msgbus, runner::get_exec_event_sender,
    runtime::get_runtime,
};
use nautilus_core::UnixNanos;
use nautilus_execution::client::{ExecutionClient, LiveExecutionClient, base::ExecutionClientCore};
use nautilus_live::execution::LiveExecutionClientExt;
use nautilus_model::{
    enums::{AccountType, OrderType},
    events::{AccountState, OrderEventAny},
    identifiers::InstrumentId,
    orders::Order,
    reports::ExecutionMassStatus,
};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::{
    common::{
        consts::{OKX_CONDITIONAL_ORDER_TYPES, OKX_VENUE},
        enums::{OKXInstrumentType, OKXTradeMode},
    },
    config::OKXExecClientConfig,
    http::client::OKXHttpClient,
    websocket::{
        client::OKXWebSocketClient,
        messages::{ExecutionReport, NautilusWsMessage},
    },
};

#[derive(Debug)]
pub struct OKXExecutionClient {
    core: ExecutionClientCore,
    config: OKXExecClientConfig,
    http_client: OKXHttpClient,
    ws_client: OKXWebSocketClient,
    trade_mode: OKXTradeMode,
    started: bool,
    connected: bool,
    instruments_initialized: bool,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl OKXExecutionClient {
    /// Creates a new [`OKXExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(core: ExecutionClientCore, config: OKXExecClientConfig) -> anyhow::Result<Self> {
        let http_client = if config.has_api_credentials() {
            OKXHttpClient::with_credentials(
                config.api_key.clone(),
                config.api_secret.clone(),
                config.api_passphrase.clone(),
                config.base_url_http.clone(),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.is_demo,
            )?
        } else {
            OKXHttpClient::new(
                config.base_url_http.clone(),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.is_demo,
            )?
        };

        let account_id = core.account_id;
        let ws_client = OKXWebSocketClient::new(
            Some(config.ws_private_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.api_passphrase.clone(),
            Some(account_id),
            None,
        )
        .context("failed to construct OKX execution websocket client")?;

        let trade_mode = match core.account_type {
            AccountType::Cash => OKXTradeMode::Cash,
            _ => OKXTradeMode::Isolated,
        };

        Ok(Self {
            core,
            config,
            http_client,
            ws_client,
            trade_mode,
            started: false,
            connected: false,
            instruments_initialized: false,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    fn instrument_types(&self) -> Vec<OKXInstrumentType> {
        if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        }
    }

    async fn ensure_instruments_initialized_async(&mut self) -> anyhow::Result<()> {
        if self.instruments_initialized {
            return Ok(());
        }

        let mut all_instruments = Vec::new();
        for instrument_type in self.instrument_types() {
            let instruments = self
                .http_client
                .request_instruments(instrument_type)
                .await
                .with_context(|| {
                    format!("failed to request OKX instruments for {instrument_type:?}")
                })?;

            if instruments.is_empty() {
                warn!("No instruments returned for {instrument_type:?}");
                continue;
            }

            self.http_client.add_instruments(instruments.clone());
            all_instruments.extend(instruments);
        }

        if all_instruments.is_empty() {
            warn!("Instrument bootstrap yielded no instruments; WebSocket submissions may fail");
        } else {
            self.ws_client.initialize_instruments_cache(all_instruments);
        }

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
            .context("failed to request OKX account state")?;

        self.core.generate_account_state(
            account_state.balances.clone(),
            account_state.margins.clone(),
            account_state.is_reported,
            account_state.ts_event,
        )
    }

    fn update_account_state(&self) -> anyhow::Result<()> {
        let runtime = get_runtime();
        runtime.block_on(self.refresh_account_state())
    }

    fn is_conditional_order(&self, order_type: OrderType) -> bool {
        OKX_CONDITIONAL_ORDER_TYPES.contains(&order_type)
    }

    fn submit_regular_order(
        &self,
        cmd: &nautilus_common::messages::execution::SubmitOrder,
    ) -> anyhow::Result<()> {
        let order = cmd.order.clone();
        let ws_client = self.ws_client.clone();
        let trade_mode = self.trade_mode;

        self.spawn_task("submit_order", async move {
            ws_client
                .submit_order(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    trade_mode,
                    order.client_order_id(),
                    order.order_side(),
                    order.order_type(),
                    order.quantity(),
                    Some(order.time_in_force()),
                    order.price(),
                    order.trigger_price(),
                    Some(order.is_post_only()),
                    Some(order.is_reduce_only()),
                    Some(order.is_quote_quantity()),
                    None,
                )
                .await?;
            Ok(())
        });

        Ok(())
    }

    fn submit_conditional_order(
        &self,
        cmd: &nautilus_common::messages::execution::SubmitOrder,
    ) -> anyhow::Result<()> {
        let order = cmd.order.clone();
        let trigger_price = order
            .trigger_price()
            .ok_or_else(|| anyhow::anyhow!("conditional order requires a trigger price"))?;
        let http_client = self.http_client.clone();
        let trade_mode = self.trade_mode;

        self.spawn_task("submit_algo_order", async move {
            http_client
                .place_algo_order_with_domain_types(
                    order.instrument_id(),
                    trade_mode,
                    order.client_order_id(),
                    order.order_side(),
                    order.order_type(),
                    order.quantity(),
                    trigger_price,
                    order.trigger_type(),
                    order.price(),
                    Some(order.is_reduce_only()),
                )
                .await?;
            Ok(())
        });

        Ok(())
    }

    fn cancel_ws_order(
        &self,
        cmd: &nautilus_common::messages::execution::CancelOrder,
    ) -> anyhow::Result<()> {
        let ws_client = self.ws_client.clone();
        let command = cmd.clone();

        self.spawn_task("cancel_order", async move {
            ws_client
                .cancel_order(
                    command.trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    Some(command.client_order_id),
                    Some(command.venue_order_id),
                )
                .await?;
            Ok(())
        });

        Ok(())
    }

    fn mass_cancel_instrument(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let ws_client = self.ws_client.clone();
        self.spawn_task("mass_cancel_orders", async move {
            ws_client.mass_cancel_orders(instrument_id).await?;
            Ok(())
        });
        Ok(())
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
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
}

impl ExecutionClient for OKXExecutionClient {
    fn is_connected(&self) -> bool {
        self.connected
    }

    fn client_id(&self) -> nautilus_model::identifiers::ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> nautilus_model::identifiers::AccountId {
        self.core.account_id
    }

    fn venue(&self) -> nautilus_model::identifiers::Venue {
        *OKX_VENUE
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
        ts_event: nautilus_core::UnixNanos,
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
        info!("OKX execution client {} started", self.core.client_id);
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
        info!("OKX execution client {} stopped", self.core.client_id);
        Ok(())
    }

    fn submit_order(
        &self,
        cmd: &nautilus_common::messages::execution::SubmitOrder,
    ) -> anyhow::Result<()> {
        let order = &cmd.order;

        if order.is_closed() {
            warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        self.core.generate_order_submitted(
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            cmd.ts_init,
        );

        let result = if self.is_conditional_order(order.order_type()) {
            self.submit_conditional_order(cmd)
        } else {
            self.submit_regular_order(cmd)
        };

        if let Err(err) = result {
            self.core.generate_order_rejected(
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                &format!("submit-order-error: {err}"),
                cmd.ts_init,
                false,
            );
            return Err(err);
        }

        Ok(())
    }

    fn submit_order_list(
        &self,
        cmd: &nautilus_common::messages::execution::SubmitOrderList,
    ) -> anyhow::Result<()> {
        warn!(
            "submit_order_list not yet implemented for OKX execution client (got {} orders)",
            cmd.order_list.orders.len()
        );
        Ok(())
    }

    fn modify_order(
        &self,
        cmd: &nautilus_common::messages::execution::ModifyOrder,
    ) -> anyhow::Result<()> {
        let ws_client = self.ws_client.clone();
        let command = cmd.clone();

        self.spawn_task("modify_order", async move {
            ws_client
                .modify_order(
                    command.trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    Some(command.client_order_id),
                    command.price,
                    command.quantity,
                    Some(command.venue_order_id),
                )
                .await?;
            Ok(())
        });

        Ok(())
    }

    fn cancel_order(
        &self,
        cmd: &nautilus_common::messages::execution::CancelOrder,
    ) -> anyhow::Result<()> {
        self.cancel_ws_order(cmd)
    }

    fn cancel_all_orders(
        &self,
        cmd: &nautilus_common::messages::execution::CancelAllOrders,
    ) -> anyhow::Result<()> {
        self.mass_cancel_instrument(cmd.instrument_id)
    }

    fn batch_cancel_orders(
        &self,
        cmd: &nautilus_common::messages::execution::BatchCancelOrders,
    ) -> anyhow::Result<()> {
        let mut payload = Vec::with_capacity(cmd.cancels.len());

        for cancel in &cmd.cancels {
            payload.push((
                OKXInstrumentType::Spot, // TODO: derive real instrument type
                cancel.instrument_id,
                Some(cancel.client_order_id),
                Some(cancel.venue_order_id.as_str().to_string()),
            ));
        }

        let ws_client = self.ws_client.clone();
        self.spawn_task("batch_cancel_orders", async move {
            ws_client.batch_cancel_orders(payload).await?;
            Ok(())
        });

        Ok(())
    }

    fn query_account(
        &self,
        _cmd: &nautilus_common::messages::execution::QueryAccount,
    ) -> anyhow::Result<()> {
        self.update_account_state()
    }

    fn query_order(
        &self,
        cmd: &nautilus_common::messages::execution::QueryOrder,
    ) -> anyhow::Result<()> {
        debug!(
            "query_order not implemented for OKX execution client (client_order_id={})",
            cmd.client_order_id
        );
        Ok(())
    }
}

#[async_trait(?Send)]
impl LiveExecutionClient for OKXExecutionClient {
    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected {
            return Ok(());
        }

        self.ensure_instruments_initialized_async().await?;

        self.ws_client.connect().await?;
        self.ws_client.wait_until_active(10.0).await?;

        for inst_type in self.instrument_types() {
            self.ws_client.subscribe_orders(inst_type).await?;
            self.ws_client.subscribe_orders_algo(inst_type).await?;

            if self.config.use_fills_channel
                && let Err(err) = self.ws_client.subscribe_fills(inst_type).await
            {
                warn!("Failed to subscribe to fills channel ({inst_type:?}): {err}");
            }
        }

        self.ws_client.subscribe_account().await?;

        self.start_ws_stream()?;
        self.refresh_account_state().await?;

        self.connected = true;
        info!("OKX execution client {} connected", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.connected {
            return Ok(());
        }

        self.http_client.cancel_all_requests();
        if let Err(err) = self.ws_client.close().await {
            warn!("Error while closing OKX websocket: {err:?}");
        }

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }

        self.abort_pending_tasks();

        self.connected = false;
        info!("OKX execution client {} disconnected", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &nautilus_common::messages::execution::GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<nautilus_model::reports::OrderStatusReport>> {
        let Some(instrument_id) = cmd.instrument_id else {
            warn!("generate_order_status_report requires instrument_id: {cmd:?}");
            return Ok(None);
        };

        let mut reports = self
            .http_client
            .request_order_status_reports(
                self.core.account_id,
                None,
                Some(instrument_id),
                None,
                None,
                false,
                None,
            )
            .await?;

        if let Some(client_order_id) = cmd.client_order_id {
            reports.retain(|report| report.client_order_id == Some(client_order_id));
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == venue_order_id.as_str());
        }

        Ok(reports.into_iter().next())
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &nautilus_common::messages::execution::GenerateOrderStatusReport,
    ) -> anyhow::Result<Vec<nautilus_model::reports::OrderStatusReport>> {
        let mut reports = Vec::new();

        if let Some(instrument_id) = cmd.instrument_id {
            let mut fetched = self
                .http_client
                .request_order_status_reports(
                    self.core.account_id,
                    None,
                    Some(instrument_id),
                    None,
                    None,
                    false,
                    None,
                )
                .await?;
            reports.append(&mut fetched);
        } else {
            for inst_type in self.instrument_types() {
                let mut fetched = self
                    .http_client
                    .request_order_status_reports(
                        self.core.account_id,
                        Some(inst_type),
                        None,
                        None,
                        None,
                        false,
                        None,
                    )
                    .await?;
                reports.append(&mut fetched);
            }
        }

        if let Some(client_order_id) = cmd.client_order_id {
            reports.retain(|report| report.client_order_id == Some(client_order_id));
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == venue_order_id.as_str());
        }

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: nautilus_common::messages::execution::GenerateFillReports,
    ) -> anyhow::Result<Vec<nautilus_model::reports::FillReport>> {
        let start_dt = nanos_to_datetime(cmd.start);
        let end_dt = nanos_to_datetime(cmd.end);
        let mut reports = Vec::new();

        if let Some(instrument_id) = cmd.instrument_id {
            let mut fetched = self
                .http_client
                .request_fill_reports(
                    self.core.account_id,
                    None,
                    Some(instrument_id),
                    start_dt,
                    end_dt,
                    None,
                )
                .await?;
            reports.append(&mut fetched);
        } else {
            for inst_type in self.instrument_types() {
                let mut fetched = self
                    .http_client
                    .request_fill_reports(
                        self.core.account_id,
                        Some(inst_type),
                        None,
                        start_dt,
                        end_dt,
                        None,
                    )
                    .await?;
                reports.append(&mut fetched);
            }
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == venue_order_id.as_str());
        }

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &nautilus_common::messages::execution::GeneratePositionReports,
    ) -> anyhow::Result<Vec<nautilus_model::reports::PositionStatusReport>> {
        let mut reports = Vec::new();

        if let Some(instrument_id) = cmd.instrument_id {
            let mut fetched = self
                .http_client
                .request_position_status_reports(self.core.account_id, None, Some(instrument_id))
                .await?;
            reports.append(&mut fetched);
        } else {
            for inst_type in self.instrument_types() {
                let mut fetched = self
                    .http_client
                    .request_position_status_reports(self.core.account_id, Some(inst_type), None)
                    .await?;
                reports.append(&mut fetched);
            }
        }

        let _ = nanos_to_datetime(cmd.start);
        let _ = nanos_to_datetime(cmd.end);

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        warn!("generate_mass_status not yet implemented (lookback_mins={lookback_mins:?})");
        Ok(None)
    }
}

impl LiveExecutionClientExt for OKXExecutionClient {
    fn get_message_channel(&self) -> tokio::sync::mpsc::UnboundedSender<ExecutionEvent> {
        get_exec_event_sender()
    }

    fn get_clock(&self) -> Ref<'_, dyn Clock> {
        self.core.clock().borrow()
    }
}

impl OKXExecutionClient {
    fn start_ws_stream(&mut self) -> anyhow::Result<()> {
        if self.ws_stream_handle.is_some() {
            return Ok(());
        }

        let stream = self.ws_client.stream();
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            pin_mut!(stream);
            while let Some(message) = stream.next().await {
                dispatch_ws_message(message);
            }
        });

        self.ws_stream_handle = Some(handle);
        Ok(())
    }
}

fn dispatch_ws_message(message: NautilusWsMessage) {
    match message {
        NautilusWsMessage::AccountUpdate(state) => dispatch_account_state(state),
        NautilusWsMessage::ExecutionReports(reports) => {
            for report in reports {
                dispatch_execution_report(report);
            }
        }
        NautilusWsMessage::OrderRejected(event) => {
            dispatch_order_event(OrderEventAny::Rejected(event));
        }
        NautilusWsMessage::OrderCancelRejected(event) => {
            dispatch_order_event(OrderEventAny::CancelRejected(event));
        }
        NautilusWsMessage::OrderModifyRejected(event) => {
            dispatch_order_event(OrderEventAny::ModifyRejected(event));
        }
        NautilusWsMessage::Error(err) => {
            warn!(
                "OKX websocket error: code={} message={} conn_id={:?}",
                err.code, err.message, err.conn_id
            );
        }
        NautilusWsMessage::Reconnected => {
            info!("OKX websocket reconnected");
        }
        NautilusWsMessage::Deltas(_)
        | NautilusWsMessage::Raw(_)
        | NautilusWsMessage::Data(_)
        | NautilusWsMessage::FundingRates(_)
        | NautilusWsMessage::Instrument(_) => {
            debug!("Ignoring OKX websocket data message");
        }
    }
}

fn dispatch_account_state(state: AccountState) {
    msgbus::send_any(
        "Portfolio.update_account".into(),
        &state as &dyn std::any::Any,
    );
}

fn dispatch_execution_report(report: ExecutionReport) {
    let sender = get_exec_event_sender();
    match report {
        ExecutionReport::Order(order_report) => {
            let exec_report =
                nautilus_common::messages::ExecutionReport::OrderStatus(Box::new(order_report));
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                warn!("Failed to send order status report: {e}");
            }
        }
        ExecutionReport::Fill(fill_report) => {
            let exec_report =
                nautilus_common::messages::ExecutionReport::Fill(Box::new(fill_report));
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                warn!("Failed to send fill report: {e}");
            }
        }
    }
}

fn dispatch_order_event(event: OrderEventAny) {
    let sender = get_exec_event_sender();
    if let Err(e) = sender.send(ExecutionEvent::Order(event)) {
        warn!("Failed to send order event: {e}");
    }
}

fn nanos_to_datetime(value: Option<UnixNanos>) -> Option<DateTime<Utc>> {
    value.map(|nanos| nanos.to_datetime_utc())
}
