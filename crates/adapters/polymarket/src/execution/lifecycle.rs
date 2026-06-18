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

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use nautilus_common::{
    cache::Cache,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    msgbus::{self, TypedHandler},
};
use nautilus_core::{MUTEX_POISONED, UnixNanos, collections::AtomicMap, time::AtomicTime};
use nautilus_model::{
    events::PositionEvent,
    identifiers::{AccountId, InstrumentId},
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
    fn ensure_position_event_subscription(&mut self) {
        if self.position_event_handler.is_some() {
            return;
        }

        let core = self.core.clone();
        let clock = self.clock;
        let shared_token_instruments = self.shared_token_instruments.clone();
        let neg_risk_index = self.neg_risk_index.clone();
        let handler = TypedHandler::from(move |event: &PositionEvent| {
            if !matches!(event, PositionEvent::PositionClosed(_)) {
                return;
            }

            if event.instrument_id().venue != core.venue {
                return;
            }

            sync_execution_lookup_for_instrument(
                &core,
                clock,
                &shared_token_instruments,
                &neg_risk_index,
                event.instrument_id(),
            );
        });

        msgbus::subscribe_position_events("events.position.*".into(), handler.clone(), Some(10));
        self.position_event_handler = Some(handler);
    }

    fn clear_position_event_subscription(&mut self) {
        if let Some(handler) = self.position_event_handler.take() {
            msgbus::unsubscribe_position_events("events.position.*".into(), &handler);
        }
    }

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

    fn should_retain_execution_lookup(
        &self,
        cache: &Cache,
        instrument: &InstrumentAny,
        now_ns: UnixNanos,
        account_id: AccountId,
    ) -> bool {
        if !crate::filters::is_expired(instrument, now_ns) {
            return true;
        }

        cache.has_positions_open(
            Some(&self.core.venue),
            Some(&instrument.id()),
            None,
            Some(&account_id),
            None,
        )
    }

    fn upsert_execution_lookup(&self, instrument: &InstrumentAny) {
        upsert_execution_lookup(
            &self.shared_token_instruments,
            &self.neg_risk_index,
            instrument,
        );
    }

    fn remove_execution_lookup(&self, instrument_id: InstrumentId) {
        remove_execution_lookup(
            &self.shared_token_instruments,
            &self.neg_risk_index,
            instrument_id,
        );
    }

    pub(super) fn load_instruments_from_cache(&self) {
        let cache = self.core.cache();
        let now_ns = self.clock.get_time_ns();
        let account_id = self.core.account_id;
        let instruments: Vec<InstrumentAny> = cache
            .instruments(&self.core.venue, None)
            .into_iter()
            .cloned()
            .collect();
        let mut retained_instrument_ids = AHashSet::new();

        for inst in instruments
            .iter()
            .filter(|inst| self.should_retain_execution_lookup(&cache, inst, now_ns, account_id))
        {
            retained_instrument_ids.insert(inst.id());
            self.upsert_execution_lookup(inst);
        }

        drop(cache);

        self.prune_execution_lookup_not_in_retained(&retained_instrument_ids);

        log::info!(
            "Loaded {} retained instruments from cache",
            retained_instrument_ids.len()
        );
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
        self.clear_position_event_subscription();

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
        self.ensure_position_event_subscription();

        let post_ws = async {
            self.refresh_account_state().await?;
            self.await_account_registered(30.0).await?;
            Ok::<(), anyhow::Error>(())
        };

        if let Err(e) = post_ws.await {
            log::warn!("Connect failed after WS started, tearing down: {e}");
            self.stopping.store(true, Ordering::Release);
            self.clear_position_event_subscription();
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
        self.clear_position_event_subscription();

        self.ws_client.disconnect().await?;

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.abort_pending_tasks();
        self.core.set_disconnected();

        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    pub(super) fn on_instrument_update(&self, instrument: &InstrumentAny) {
        let cache = self.core.cache();
        let now_ns = self.clock.get_time_ns();
        let account_id = self.core.account_id;
        let instrument_id = instrument.id();

        if self.should_retain_execution_lookup(&cache, instrument, now_ns, account_id) {
            self.upsert_execution_lookup(instrument);
        } else {
            self.remove_execution_lookup(instrument_id);
        }
    }

    fn prune_execution_lookup_not_in_retained(
        &self,
        retained_instrument_ids: &AHashSet<InstrumentId>,
    ) {
        let token_ids: Vec<Ustr> = self
            .shared_token_instruments
            .load()
            .keys()
            .copied()
            .collect();

        for token_id in token_ids {
            if let Some(instrument) = self.shared_token_instruments.get_cloned(&token_id)
                && !retained_instrument_ids.contains(&instrument.id())
            {
                self.remove_execution_lookup(instrument.id());
            }
        }

        let instrument_ids: Vec<InstrumentId> =
            self.neg_risk_index.load().keys().copied().collect();

        for instrument_id in instrument_ids {
            if !retained_instrument_ids.contains(&instrument_id) {
                self.neg_risk_index.remove(&instrument_id);
            }
        }
    }
}

fn upsert_execution_lookup(
    shared_token_instruments: &AtomicMap<Ustr, InstrumentAny>,
    neg_risk_index: &AtomicMap<InstrumentId, bool>,
    instrument: &InstrumentAny,
) {
    let token_id = Ustr::from(instrument.raw_symbol().as_str());
    shared_token_instruments.insert(token_id, instrument.clone());

    if let InstrumentAny::BinaryOption(bo) = instrument {
        let neg_risk = bo
            .info
            .as_ref()
            .and_then(|i| i.get_bool("neg_risk"))
            .unwrap_or(false);
        neg_risk_index.insert(bo.id, neg_risk);
    }
}

fn remove_execution_lookup(
    shared_token_instruments: &AtomicMap<Ustr, InstrumentAny>,
    neg_risk_index: &AtomicMap<InstrumentId, bool>,
    instrument_id: InstrumentId,
) {
    let token_ids = shared_token_instruments
        .load()
        .iter()
        .filter_map(|(token_id, instrument)| {
            (instrument.id() == instrument_id).then_some(*token_id)
        })
        .collect::<Vec<_>>();

    for token_id in token_ids {
        shared_token_instruments.remove(&token_id);
    }

    neg_risk_index.remove(&instrument_id);
}

fn sync_execution_lookup_for_instrument(
    core: &nautilus_live::ExecutionClientCore,
    clock: &'static AtomicTime,
    shared_token_instruments: &AtomicMap<Ustr, InstrumentAny>,
    neg_risk_index: &AtomicMap<InstrumentId, bool>,
    instrument_id: InstrumentId,
) {
    let now_ns = clock.get_time_ns();
    let account_id = core.account_id;
    let cache = core.cache();

    let retained = cache
        .instrument(&instrument_id)
        .cloned()
        .filter(|instrument| {
            if !crate::filters::is_expired(instrument, now_ns) {
                return true;
            }

            cache.has_positions_open(
                Some(&core.venue),
                Some(&instrument_id),
                None,
                Some(&account_id),
                None,
            )
        });

    drop(cache);

    if let Some(instrument) = retained {
        upsert_execution_lookup(shared_token_instruments, neg_risk_index, &instrument);
    } else {
        remove_execution_lookup(shared_token_instruments, neg_risk_index, instrument_id);
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, live::runner::set_exec_event_sender};
    use nautilus_core::{UUID4, nanos::DurationNanos};
    use nautilus_live::ExecutionClientCore;
    use nautilus_model::{
        enums::{AccountType, OmsType, OrderSide, PositionSide, TimeInForce},
        events::{OrderEventAny, PositionClosed, PositionEvent},
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId,
        },
        instruments::stubs::binary_option,
        orders::{LimitOrder, OrderAny, stubs::TestOrderEventStubs},
        position::Position,
        types::{Currency, Money, Price, Price as ModelPrice, Quantity, Quantity as ModelQuantity},
    };
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    const TEST_PRIVATE_KEY: &str =
        "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
    const TEST_API_SECRET_B64: &str = "dGVzdF9zZWNyZXRfa2V5XzMyYnl0ZXNfcGFkMTIzNDU=";

    fn test_client() -> (PolymarketExecutionClient, Rc<RefCell<Cache>>) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            ClientId::from("POLYMARKET"),
            *crate::common::consts::POLYMARKET_VENUE,
            OmsType::Netting,
            AccountId::from("POLYMARKET-001"),
            AccountType::Cash,
            None,
            cache.clone(),
        );
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        set_exec_event_sender(tx);
        let client = PolymarketExecutionClient::new(
            core,
            crate::config::PolymarketExecClientConfig {
                private_key: Some(TEST_PRIVATE_KEY.to_string()),
                api_key: Some("test_api_key".to_string()),
                api_secret: Some(TEST_API_SECRET_B64.to_string()),
                passphrase: Some("test_pass".to_string()),
                funder: None,
                base_url_http: Some("http://127.0.0.1:3000".to_string()),
                base_url_ws: Some("ws://127.0.0.1:3000/ws".to_string()),
                base_url_data_api: Some("http://127.0.0.1:3000".to_string()),
                ..crate::config::PolymarketExecClientConfig::default()
            },
        )
        .expect("test client should construct");

        (client, cache)
    }

    fn test_binary_option(raw_symbol: &str, expired: bool, neg_risk: bool) -> InstrumentAny {
        let clock = nautilus_core::time::get_atomic_clock_realtime();
        let mut binary = binary_option();
        binary.id = InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str());
        binary.raw_symbol = Symbol::new(raw_symbol);
        binary.currency = Currency::pUSD();
        binary.expiration_ns = if expired {
            UnixNanos::from(clock.get_time_ns().as_u64().saturating_sub(1_000_000_000))
        } else {
            UnixNanos::from(
                clock
                    .get_time_ns()
                    .as_u64()
                    .saturating_add(86_400_000_000_000),
            )
        };

        let mut info = nautilus_core::Params::new();
        info.insert("neg_risk".to_string(), Value::Bool(neg_risk));
        binary.info = Some(info);

        InstrumentAny::BinaryOption(binary)
    }

    fn open_limit_order(instrument_id: InstrumentId) -> OrderAny {
        OrderAny::Limit(LimitOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            ClientOrderId::from("O-RETAIN"),
            OrderSide::Buy,
            ModelQuantity::new(10.0, 0),
            ModelPrice::from("0.5000"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            nautilus_core::UUID4::new(),
            UnixNanos::default(),
        ))
    }

    fn open_position(instrument: &InstrumentAny) -> Position {
        let order = open_limit_order(instrument.id());
        let filled = match TestOrderEventStubs::filled(
            &order,
            instrument,
            None,
            None,
            Some(ModelPrice::from("0.5000")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("POLYMARKET-001")),
        ) {
            OrderEventAny::Filled(filled) => filled,
            other => panic!("expected filled event, was {other:?}"),
        };

        Position::new(instrument, filled)
    }

    fn closed_position(position: &Position) -> Position {
        let mut closed = position.clone();
        closed.side = PositionSide::Flat;
        closed.signed_qty = 0.0;
        closed.quantity = Quantity::zero(position.size_precision);
        closed.ts_closed = Some(position.ts_last);
        closed.duration_ns = 1;
        closed
    }

    fn position_closed_event(position: &Position) -> PositionEvent {
        PositionEvent::PositionClosed(PositionClosed {
            trader_id: position.trader_id,
            strategy_id: position.strategy_id,
            instrument_id: position.instrument_id,
            position_id: position.id,
            account_id: position.account_id,
            opening_order_id: position.opening_order_id,
            closing_order_id: position.closing_order_id,
            entry: position.entry,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::zero(position.size_precision),
            peak_quantity: position.peak_qty,
            last_qty: Quantity::zero(position.size_precision),
            last_px: Price::zero(position.price_precision),
            currency: position.quote_currency,
            avg_px_open: position.avg_px_open,
            avg_px_close: position.avg_px_close,
            realized_return: position.realized_return,
            realized_pnl: position.realized_pnl,
            unrealized_pnl: Money::zero(position.quote_currency),
            duration: DurationNanos::from(1_u64),
            event_id: UUID4::new(),
            ts_opened: position.ts_opened,
            ts_closed: position.ts_closed.or(Some(position.ts_last)),
            ts_event: position.ts_last,
            ts_init: position.ts_last,
        })
    }

    #[rstest]
    fn load_instruments_from_cache_prunes_expired_execution_lookup_state() {
        let (client, cache) = test_client();
        let active = test_binary_option("0xACTIVE", false, true);
        let expired = test_binary_option("0xEXPIRED", true, true);

        {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(active.clone()).unwrap();
            cache.add_instrument(expired.clone()).unwrap();
        }

        client
            .shared_token_instruments
            .insert(Ustr::from(expired.raw_symbol().as_str()), expired.clone());
        client.neg_risk_index.insert(expired.id(), true);

        client.load_instruments_from_cache();

        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(active.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&active.id()));
        assert!(
            !client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(!client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn on_instrument_update_skips_expired_execution_lookup_state() {
        let (client, _cache) = test_client();
        let expired = test_binary_option("0xEXPIRED_ONLY", true, true);

        client.on_instrument_update(&expired);

        assert!(
            !client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(!client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn load_instruments_from_cache_keeps_expired_lookup_state_with_open_position() {
        let (client, cache) = test_client();
        let expired = test_binary_option("0xEXPIRED_POSITION", true, true);
        let position = open_position(&expired);

        {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(expired.clone()).unwrap();
            cache.add_position(&position, OmsType::Netting).unwrap();
        }

        client.load_instruments_from_cache();

        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn sync_execution_lookup_prunes_expired_lookup_after_position_closes() {
        let (client, cache) = test_client();
        let expired = test_binary_option("0xEXPIRED_CLOSED", true, true);
        let position = open_position(&expired);
        let closed = closed_position(&position);

        {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(expired.clone()).unwrap();
            cache.add_position(&position, OmsType::Netting).unwrap();
        }

        client.load_instruments_from_cache();
        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&expired.id()));

        {
            let mut cache = cache.borrow_mut();
            cache.update_position(&closed).unwrap();
        }

        let event = position_closed_event(&closed);
        assert!(matches!(event, PositionEvent::PositionClosed(_)));
        sync_execution_lookup_for_instrument(
            &client.core,
            client.clock,
            &client.shared_token_instruments,
            &client.neg_risk_index,
            event.instrument_id(),
        );

        assert!(
            !client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(!client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn position_event_subscription_can_be_reinstalled_after_disconnect_cleanup() {
        let (mut client, _cache) = test_client();

        client.start_client();
        assert!(client.position_event_handler.is_none());

        client.ensure_position_event_subscription();
        assert!(client.position_event_handler.is_some());

        client.clear_position_event_subscription();
        assert!(client.position_event_handler.is_none());

        client.ensure_position_event_subscription();
        assert!(client.position_event_handler.is_some());
    }
}
