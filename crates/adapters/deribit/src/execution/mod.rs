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

//! Live execution client implementation for the Deribit adapter.

use std::{
    future::Future,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
};
use nautilus_core::{MUTEX_POISONED, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    events::AccountState,
    identifiers::{AccountId, ClientId, Venue},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use tokio::task::JoinHandle;

use crate::{
    common::consts::DERIBIT_VENUE,
    config::DeribitExecClientConfig,
    http::{client::DeribitHttpClient, models::DeribitCurrency},
};

/// Deribit live execution client.
#[derive(Debug)]
pub struct DeribitExecutionClient {
    core: ExecutionClientCore,
    config: DeribitExecClientConfig,
    http_client: DeribitHttpClient,
    exec_event_sender: Option<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>>,
    started: bool,
    connected: AtomicBool,
    instruments_initialized: AtomicBool,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl DeribitExecutionClient {
    /// Creates a new [`DeribitExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(core: ExecutionClientCore, config: DeribitExecClientConfig) -> anyhow::Result<Self> {
        let http_client = if config.has_api_credentials() {
            DeribitHttpClient::new_with_env(
                config.api_key.clone(),
                config.api_secret.clone(),
                config.use_testnet,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                None, // proxy_url
            )?
        } else {
            DeribitHttpClient::new(
                config.base_url_http.clone(),
                config.use_testnet,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                None, // proxy_url
            )?
        };

        Ok(Self {
            core,
            config,
            http_client,
            exec_event_sender: None,
            started: false,
            connected: AtomicBool::new(false),
            instruments_initialized: AtomicBool::new(false),
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    /// Spawns an async task for execution operations.
    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        });

        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        tasks.retain(|handle| !handle.is_finished());
        tasks.push(handle);
    }

    /// Aborts all pending async tasks.
    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    /// Dispatches an account state event to the execution event sender.
    fn dispatch_account_state(&self, account_state: AccountState) -> anyhow::Result<()> {
        if let Some(sender) = &self.exec_event_sender {
            sender
                .send(ExecutionEvent::Account(account_state))
                .map_err(|e| anyhow::anyhow!("Failed to send account state: {e}"))?;
        }
        Ok(())
    }
}

#[async_trait(?Send)]
impl ExecutionClient for DeribitExecutionClient {
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *DERIBIT_VENUE
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

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, instrument_kinds={:?}, use_testnet={}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.config.instrument_kinds,
            self.config.use_testnet
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.started {
            return Ok(());
        }

        self.started = false;
        self.connected.store(false, Ordering::Release);
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected.load(Ordering::Acquire) {
            return Ok(());
        }

        // Initialize exec event sender (must be done in async context after runner is set up)
        if self.exec_event_sender.is_none() {
            self.exec_event_sender = Some(get_exec_event_sender());
        }

        // Fetch and cache instruments
        if !self.instruments_initialized.load(Ordering::Acquire) {
            for kind in &self.config.instrument_kinds {
                let instruments = self
                    .http_client
                    .request_instruments(DeribitCurrency::ANY, Some(*kind))
                    .await
                    .with_context(|| {
                        format!("failed to request Deribit instruments for {kind:?}")
                    })?;

                if instruments.is_empty() {
                    log::warn!("No instruments returned for {kind:?}");
                    continue;
                }

                self.http_client.cache_instruments(instruments);
            }
            self.instruments_initialized.store(true, Ordering::Release);
        }

        // Check if credentials are available before requesting account state
        if !self.config.has_api_credentials() {
            let (key_env, secret_env) = if self.config.use_testnet {
                ("DERIBIT_TESTNET_API_KEY", "DERIBIT_TESTNET_API_SECRET")
            } else {
                ("DERIBIT_API_KEY", "DERIBIT_API_SECRET")
            };
            anyhow::bail!(
                "Missing Deribit API credentials. Set environment variables: {key_env} and {secret_env}"
            );
        }

        // Fetch initial account state
        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request Deribit account state")?;

        self.dispatch_account_state(account_state)?;

        self.connected.store(true, Ordering::Release);
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.connected.load(Ordering::Acquire) {
            return Ok(());
        }

        self.abort_pending_tasks();

        self.connected.store(false, Ordering::Release);
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        todo!("Implement generate_order_status_report for Deribit execution client");
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        todo!("Implement generate_order_status_reports for Deribit execution client");
    }

    async fn generate_fill_reports(
        &self,
        _cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        todo!("Implement generate_fill_reports for Deribit execution client");
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        todo!("Implement generate_position_status_reports for Deribit execution client");
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::warn!("generate_mass_status not yet implemented (lookback_mins={lookback_mins:?})");
        Ok(None)
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let exec_sender = self.exec_event_sender.clone();

        self.spawn_task("query_account", async move {
            let account_state = http_client
                .request_account_state(account_id)
                .await
                .context(
                    "failed to query Deribit account state (check API credentials are valid)",
                )?;

            if let Some(sender) = exec_sender {
                sender
                    .send(ExecutionEvent::Account(account_state))
                    .map_err(|e| anyhow::anyhow!("Failed to send account state: {e}"))?;
            }
            Ok(())
        });

        Ok(())
    }

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        todo!("Implement query_order for Deribit execution client")
    }

    fn submit_order(&self, _cmd: &SubmitOrder) -> anyhow::Result<()> {
        todo!("Implement submit_order for Deribit execution client");
    }

    fn submit_order_list(&self, _cmd: &SubmitOrderList) -> anyhow::Result<()> {
        todo!("Implement submit_order_list for Deribit execution client");
    }

    fn modify_order(&self, _cmd: &ModifyOrder) -> anyhow::Result<()> {
        todo!("Implement modify_order for Deribit execution client");
    }

    fn cancel_order(&self, _cmd: &CancelOrder) -> anyhow::Result<()> {
        todo!("Implement cancel_order for Deribit execution client");
    }

    fn cancel_all_orders(&self, _cmd: &CancelAllOrders) -> anyhow::Result<()> {
        todo!("Implement cancel_all_orders for Deribit execution client");
    }

    fn batch_cancel_orders(&self, _cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        todo!("Implement batch_cancel_orders for Deribit execution client");
    }
}
