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

use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use ahash::AHashMap;
use anyhow::Context;
use nautilus_common::live::{runner::get_exec_event_sender, runtime::get_runtime};
use nautilus_core::MUTEX_POISONED;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use ustr::Ustr;

use super::PolymarketExecutionClient;
use crate::{
    execution::reports::fetch_and_emit_account_state,
    websocket::{
        dispatch::{WsDispatchContext, WsDispatchState, dispatch_user_message},
        messages::PolymarketWsMessage,
    },
};

impl PolymarketExecutionClient {
    pub(super) fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
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

    pub(super) fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    pub(super) async fn refresh_account_state(&self) -> anyhow::Result<()> {
        fetch_and_emit_account_state(
            &self.http_client,
            &self.emitter,
            self.clock,
            self.config.signature_type,
        )
        .await
    }

    pub(super) async fn await_account_registered(&self, timeout_secs: f64) -> anyhow::Result<()> {
        let account_id = self.core.account_id;

        if self.core.cache().account(&account_id).is_some() {
            log::info!("Account {account_id} registered");
            return Ok(());
        }

        let start = Instant::now();
        let timeout = Duration::from_secs_f64(timeout_secs);
        let interval = Duration::from_millis(10);

        loop {
            tokio::time::sleep(interval).await;

            if self.core.cache().account(&account_id).is_some() {
                log::info!("Account {account_id} registered");
                return Ok(());
            }

            if start.elapsed() >= timeout {
                anyhow::bail!(
                    "Timeout waiting for account {account_id} to be registered after {timeout_secs}s"
                );
            }
        }
    }

    pub(super) async fn start_ws_stream(&mut self) -> anyhow::Result<()> {
        self.ws_client
            .connect()
            .await
            .context("failed to connect user WebSocket")?;

        self.ws_client
            .subscribe_user()
            .await
            .context("failed to subscribe to user channel")?;

        let mut rx = self
            .ws_client
            .take_message_receiver()
            .ok_or_else(|| anyhow::anyhow!("WebSocket message receiver not available"))?;

        let emitter = self.emitter.clone();
        let token_instruments = self.shared_token_instruments.clone();
        let account_id = self.core.account_id;
        let http_client = self.http_client.clone();
        let clock = self.clock;
        let signature_type = self.config.signature_type;
        let user_address = self
            .secrets
            .funder
            .clone()
            .unwrap_or_else(|| self.secrets.address.clone());
        let user_api_key = self.secrets.credential.api_key().to_string();

        let fill_tracker = self.fill_tracker.clone();
        let pending_submits = self.pending_submits.clone();
        let pending_fills = self.pending_fills.clone();
        let pending_order_reports = self.pending_order_reports.clone();

        let handle = get_runtime().spawn(async move {
            let mut state = WsDispatchState::default();
            let ctx = WsDispatchContext {
                token_instruments: &token_instruments,
                fill_tracker: &fill_tracker,
                pending_submits: &pending_submits,
                pending_fills: &pending_fills,
                pending_order_reports: &pending_order_reports,
                emitter: &emitter,
                account_id,
                clock,
                user_address: &user_address,
                user_api_key: &user_api_key,
            };

            loop {
                match rx.recv().await {
                    Some(PolymarketWsMessage::User(user_msg)) => {
                        if let Some(_refresh) =
                            dispatch_user_message(&user_msg, &ctx, &mut state)
                        {
                            let http = http_client.clone();
                            let emit = emitter.clone();

                            get_runtime().spawn(async move {
                                match fetch_and_emit_account_state(
                                    &http, &emit, clock, signature_type,
                                )
                                .await
                                {
                                    Ok(()) => log::info!(
                                        "Account state refreshed after finalized trade for {account_id}"
                                    ),
                                    Err(e) => log::warn!(
                                        "Failed to refresh account after finalized trade: {e}"
                                    ),
                                }
                            });
                        }
                    }
                    Some(PolymarketWsMessage::Market(_)) => {}
                    Some(PolymarketWsMessage::Reconnected) => {
                        log::info!("User WebSocket reconnected");
                    }
                    None => {
                        log::debug!("User WebSocket stream ended");
                        break;
                    }
                }
            }

            log::debug!("User WebSocket handler task completed");
        });

        *self.ws_stream_handle.lock().expect(MUTEX_POISONED) = Some(handle);
        Ok(())
    }

    pub(super) fn get_neg_risk(&self, instrument_id: &InstrumentId) -> bool {
        self.neg_risk_index
            .get_cloned(instrument_id)
            .unwrap_or(false)
    }

    pub(super) fn get_neg_risk_from_snapshot(
        neg_risk_index: &AHashMap<InstrumentId, bool>,
        instrument_id: &InstrumentId,
    ) -> bool {
        neg_risk_index.get(instrument_id).copied().unwrap_or(false)
    }

    pub(super) fn load_instruments_from_cache(&self) {
        let cache = self.core.cache();
        let instruments: Vec<InstrumentAny> = cache
            .instruments(&self.core.venue, None)
            .into_iter()
            .cloned()
            .collect();
        drop(cache);

        for inst in &instruments {
            self.shared_token_instruments
                .insert(Ustr::from(inst.raw_symbol().as_str()), inst.clone());
        }

        for inst in &instruments {
            if let InstrumentAny::BinaryOption(bo) = inst {
                let neg_risk = bo
                    .info
                    .as_ref()
                    .and_then(|i| i.get_bool("neg_risk"))
                    .unwrap_or(false);
                self.neg_risk_index.insert(bo.id, neg_risk);
            }
        }

        log::info!("Loaded {} instruments from cache", instruments.len());
    }

    pub(super) fn start_client(&mut self) {
        if self.core.is_started() {
            return;
        }

        self.stopping.store(false, Ordering::Release);
        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        log::info!(
            "Started: client_id={}, account_id={}",
            self.core.client_id,
            self.core.account_id,
        );
    }

    pub(super) fn stop_client(&mut self) {
        if self.core.is_stopped() {
            return;
        }

        log::info!("Stopping Polymarket execution client");

        self.stopping.store(true, Ordering::Release);

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.abort_pending_tasks();
        self.ws_client.abort();

        self.core.set_disconnected();
        self.core.set_stopped();

        log::info!("Polymarket execution client stopped");
    }

    pub(super) async fn connect_client(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        log::info!("Connecting Polymarket execution client");

        self.stopping.store(false, Ordering::Release);

        self.load_instruments_from_cache();
        self.core.set_instruments_initialized();

        self.start_ws_stream().await?;

        let post_ws = async {
            self.refresh_account_state().await?;
            self.await_account_registered(30.0).await?;
            Ok::<(), anyhow::Error>(())
        };

        if let Err(e) = post_ws.await {
            log::warn!("Connect failed after WS started, tearing down: {e}");
            self.stopping.store(true, Ordering::Release);
            let _ = self.ws_client.disconnect().await;
            self.abort_pending_tasks();
            return Err(e);
        }

        self.core.set_connected();

        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    pub(super) async fn disconnect_client(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        log::info!("Disconnecting Polymarket execution client");

        self.stopping.store(true, Ordering::Release);

        self.ws_client.disconnect().await?;

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.abort_pending_tasks();
        self.core.set_disconnected();

        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    pub(super) fn on_instrument_update(&self, instrument: InstrumentAny) {
        let token_id = Ustr::from(instrument.raw_symbol().as_str());
        if let InstrumentAny::BinaryOption(bo) = &instrument {
            let neg_risk = bo
                .info
                .as_ref()
                .and_then(|i| i.get_bool("neg_risk"))
                .unwrap_or(false);
            self.neg_risk_index.insert(bo.id, neg_risk);
        }
        self.shared_token_instruments.insert(token_id, instrument);
    }
}
