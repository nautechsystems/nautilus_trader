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

use std::{collections::VecDeque, sync::Arc};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use futures_util::{Stream, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use nautilus_common::{
    live::sender::EventSender,
    messages::{DataEvent, data::DataResponseEnvelope},
    msgbus::{self, MessagingSwitchboard, TypedHandler},
};
use nautilus_core::{AtomicTime, UUID4};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    enums::AccountType,
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    http::client::OKXHttpClient,
    websocket::{
        client::OKXWebSocketClient,
        dispatch::{WsDispatchState, dispatch_ws_message},
        messages::OKXWsMessage,
        parse::{FeeCache, FilledQtyCache, OrderStateSnapshot},
    },
};

const MAX_PENDING_MESSAGES: usize = 10_000;
const MAX_CONCURRENT_RECOVERIES: usize = 64;

#[derive(Debug)]
pub(super) struct StreamState {
    http: OKXHttpClient,
    private: OKXWebSocketClient,
    business: OKXWebSocketClient,
    registration: Mutex<Registration>,
}

#[derive(Debug, Default)]
struct Registration {
    ready: AHashSet<InstrumentId>,
    pending: AHashMap<InstrumentId, (InstrumentAny, tokio::sync::oneshot::Sender<()>)>,
    completion_id: Option<UUID4>,
    failure: Option<String>,
}

impl StreamState {
    pub(super) fn new(
        http: OKXHttpClient,
        private: OKXWebSocketClient,
        business: OKXWebSocketClient,
        registered: impl IntoIterator<Item = InstrumentId>,
    ) -> Self {
        Self {
            http,
            private,
            business,
            registration: Mutex::new(Registration {
                ready: registered.into_iter().collect(),
                ..Default::default()
            }),
        }
    }

    pub(super) fn observe(&self, published: &InstrumentAny, cached: Option<&InstrumentAny>) {
        let Some(cached) = cached else {
            return;
        };

        if !published.same_definition(cached) {
            return;
        }

        self.accept_registration(cached);
    }

    pub(super) fn register_completion(self: &Arc<Self>, core: ExecutionClientCore) {
        if !msgbus::get_message_bus()
            .borrow()
            .is_registered(MessagingSwitchboard::data_engine_process())
        {
            return;
        }

        let id = UUID4::new();
        self.registration.lock().completion_id = Some(id);
        self.register_completion_handler(core, id);
    }

    pub(super) fn unregister_completion(&self) {
        if let Some(id) = self.registration.lock().completion_id.take() {
            msgbus::get_message_bus()
                .borrow_mut()
                .take_response_handler(&id);
        }
    }

    fn register_completion_handler(self: &Arc<Self>, core: ExecutionClientCore, id: UUID4) {
        let state = Arc::downgrade(self);
        let handler = TypedHandler::from(move |_: &DataResponseEnvelope| {
            let Some(state) = state.upgrade() else {
                return;
            };

            let cache = core.cache();
            let accepted = state
                .registration
                .lock()
                .pending
                .iter()
                .filter_map(|(id, (expected, _))| {
                    cache
                        .instrument(id)
                        .filter(|current| registration_satisfies(current, expected))
                })
                .collect::<Vec<_>>();

            for instrument in accepted {
                state.accept_registration(instrument);
            }

            state.register_completion_handler(core.clone(), id);
        });

        msgbus::register_response_handler(&id, handler);
    }

    fn accept_registration(&self, instrument: &InstrumentAny) {
        let mut registration = self.registration.lock();
        let cached = self.cache_latest_definition(instrument);
        let id = cached.id();
        registration.ready.insert(id);
        if registration
            .pending
            .get(&id)
            .is_some_and(|(expected, _)| registration_satisfies(&cached, expected))
            && let Some((_, sender)) = registration.pending.remove(&id)
        {
            let _ = sender.send(());
        }
    }

    pub(super) fn cache_routed_definition(&self, instrument: &InstrumentAny) {
        let _guard = self.registration.lock();
        self.cache_latest_definition(instrument);
    }

    fn cache_latest_definition(&self, instrument: &InstrumentAny) -> InstrumentAny {
        let symbol = instrument.raw_symbol().inner();
        let latest = [
            self.private.instruments_cache_arc().get_cloned(&symbol),
            self.business.instruments_cache_arc().get_cloned(&symbol),
        ]
        .into_iter()
        .flatten()
        .filter(|cached| cached.ts_init() > instrument.ts_init())
        .max_by_key(Instrument::ts_init)
        .unwrap_or_else(|| instrument.clone());
        let latest = self.http.cache_latest_instrument(&latest);
        let code = self.http.inst_id_code(&symbol);
        let quote_currencies = self.http.trade_quote_ccy_list(&symbol);

        for ws in [&self.private, &self.business] {
            ws.cache_instrument(latest.clone());
            if let Some(code) = code {
                ws.cache_inst_id_code(symbol, code);
            }

            if let Some(currencies) = &quote_currencies {
                ws.cache_trade_quote_ccy_lists([(symbol, currencies.clone())]);
            }
        }

        latest
    }

    pub(super) fn is_healthy(&self) -> bool {
        self.registration.lock().failure.is_none()
    }

    pub(super) fn take_failure(&self) -> Option<String> {
        self.registration.lock().failure.take()
    }

    fn ready(&self, id: InstrumentId) -> bool {
        self.has_registered_instrument(&self.registration.lock(), id)
    }

    fn has_registered_instrument(&self, registration: &Registration, id: InstrumentId) -> bool {
        let symbol = id.symbol.inner();
        registration.ready.contains(&id)
            && self.private.instruments_cache_arc().contains_key(&symbol)
            && (self.http.is_spread(id) || self.private.get_inst_id_code(&symbol).is_some())
    }

    fn fail(&self, reason: String) {
        let mut registration = self.registration.lock();
        if registration.failure.is_none() {
            log::error!("OKX execution stream recovery failed: {reason}");
            registration.failure = Some(reason);
        }

        drop(registration);
        self.private.begin_shutdown();
        self.business.begin_shutdown();
    }

    async fn recover(
        &self,
        id: InstrumentId,
        sender: Option<EventSender<DataEvent>>,
    ) -> anyhow::Result<()> {
        super::prepare_inst_id_code(&self.http, id).await?;
        anyhow::ensure!(
            self.http.is_spread(id) || self.http.inst_id_code(&id.symbol.inner()).is_some(),
            "required numeric instrument code is unavailable for {id}"
        );
        let instrument = self
            .http
            .resolve_instrument(id)
            .await
            .context("stream instrument lookup failed")?;

        let receiver = {
            let mut registration = self.registration.lock();
            if self.has_registered_instrument(&registration, id)
                && self
                    .private
                    .instruments_cache_arc()
                    .get_cloned(&id.symbol.inner())
                    .is_some_and(|current| registration_satisfies(&current, &instrument))
            {
                return Ok(());
            }

            let sender = sender
                .context("data event sender is unavailable for stream instrument registration")?;
            let (completion, receiver) = tokio::sync::oneshot::channel();
            sender
                .send(DataEvent::Data(instrument.clone().into()))
                .map_err(|_| anyhow::anyhow!("stream instrument registration receiver closed"))?;

            // A stale definition is not published, so check the cache after the data event
            if let Some(id) = registration.completion_id {
                sender
                    .send(DataEvent::ResponseEnvelope(DataResponseEnvelope::terminal(
                        id, None,
                    )))
                    .map_err(|_| {
                        anyhow::anyhow!("stream instrument registration receiver closed")
                    })?;
            }

            registration.pending.insert(id, (instrument, completion));
            receiver
        };

        // Startup buffers data until all execution clients release their engine borrow
        receiver
            .await
            .context("stream instrument registration acknowledgement closed")?;
        Ok(())
    }
}

fn registration_satisfies(current: &InstrumentAny, expected: &InstrumentAny) -> bool {
    current.ts_init() > expected.ts_init() || expected.same_definition(current)
}

pub(super) struct ExecutionStream {
    state: Arc<StreamState>,
    dispatch: Arc<WsDispatchState>,
    emitter: ExecutionEventEmitter,
    account_id: AccountId,
    account_type: AccountType,
    clock: &'static AtomicTime,
    data_sender: Option<EventSender<DataEvent>>,
    pending: AHashMap<InstrumentId, VecDeque<OKXWsMessage>>,
    recovery_pending: VecDeque<InstrumentId>,
    recovery: FuturesUnordered<BoxFuture<'static, (InstrumentId, anyhow::Result<()>)>>,
    account_refreshes: FuturesUnordered<BoxFuture<'static, ()>>,
    buffered: usize,
    fee_cache: FeeCache,
    filled_qty_cache: FilledQtyCache,
    order_state_cache: AHashMap<ClientOrderId, OrderStateSnapshot>,
}

impl ExecutionStream {
    pub(super) fn new(
        state: Arc<StreamState>,
        dispatch: Arc<WsDispatchState>,
        emitter: ExecutionEventEmitter,
        account_type: AccountType,
        clock: &'static AtomicTime,
        data_sender: Option<EventSender<DataEvent>>,
    ) -> Self {
        let account_id = emitter.account_id();

        Self {
            state,
            dispatch,
            emitter,
            account_id,
            account_type,
            clock,
            data_sender,
            pending: AHashMap::new(),
            recovery_pending: VecDeque::new(),
            recovery: FuturesUnordered::new(),
            account_refreshes: FuturesUnordered::new(),
            buffered: 0,
            fee_cache: FeeCache::new(),
            filled_qty_cache: FilledQtyCache::new(),
            order_state_cache: AHashMap::new(),
        }
    }

    pub(super) async fn run(
        mut self,
        stream: impl Stream<Item = OKXWsMessage> + Send,
        cancel: CancellationToken,
    ) {
        futures_util::pin_mut!(stream);

        loop {
            tokio::select! {
                biased;
                () = cancel.cancelled() => break,
                Some((id, result)) = self.recovery.next(), if !self.recovery.is_empty() => {
                    if result.is_err() {
                        self.state.fail(format!("required instrument {id} could not be recovered and registered"));
                        break;
                    }
                    let queue = self.pending.remove(&id).expect("recovery owns a pending instrument");
                    for message in queue {
                        self.buffered -= 1;
                        self.dispatch(message);
                    }
                    self.start_recoveries();
                }
                Some(()) = self.account_refreshes.next(), if !self.account_refreshes.is_empty() => {}
                () = self.dispatch.wait_for_linked_child_route() => {
                    self.dispatch(OKXWsMessage::Orders(Vec::new()));
                }
                message = stream.next() => {
                    let Some(message) = message else {
                        self.state.fail("execution streams closed unexpectedly".into());
                        break;
                    };

                    if let Err(e) = self.receive(message) {
                        self.state.fail(e.to_string());
                        break;
                    }
                }
            }
        }
    }

    fn receive(&mut self, message: OKXWsMessage) -> anyhow::Result<()> {
        match message {
            OKXWsMessage::Orders(rows) => {
                for row in rows {
                    let symbol = row.inst_id;
                    self.enqueue(symbol.as_str(), OKXWsMessage::Orders(vec![row]))?;
                }
            }
            OKXWsMessage::SpreadOrders(rows) => {
                for row in rows {
                    let symbol = row.sprd_id;
                    self.enqueue(symbol.as_str(), OKXWsMessage::SpreadOrders(vec![row]))?;
                }
            }
            OKXWsMessage::AlgoOrders(rows) => {
                for row in rows {
                    let symbol = row.inst_id;
                    self.enqueue(symbol.as_str(), OKXWsMessage::AlgoOrders(vec![row]))?;
                }
            }
            OKXWsMessage::Positions(serde_json::Value::Array(rows)) => {
                for row in rows {
                    if let Some(symbol) = row
                        .get("instId")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                    {
                        self.enqueue(
                            &symbol,
                            OKXWsMessage::Positions(serde_json::Value::Array(vec![row])),
                        )?;
                    } else {
                        self.dispatch(OKXWsMessage::Positions(serde_json::Value::Array(vec![row])));
                    }
                }
            }
            message => self.dispatch(message),
        }

        Ok(())
    }

    fn enqueue(&mut self, symbol: &str, message: OKXWsMessage) -> anyhow::Result<()> {
        let id: InstrumentId = format!("{symbol}.OKX")
            .parse()
            .context("invalid execution instrument symbol")?;

        if !self.pending.contains_key(&id) && self.state.ready(id) {
            self.dispatch(message);
            return Ok(());
        }

        anyhow::ensure!(
            self.buffered < MAX_PENDING_MESSAGES,
            "pending execution message limit exceeded"
        );

        if !self.pending.contains_key(&id) {
            self.pending.insert(id, VecDeque::new());
            self.recovery_pending.push_back(id);
            self.start_recoveries();
        }

        self.pending
            .get_mut(&id)
            .expect("pending queue is present")
            .push_back(message);
        self.buffered += 1;
        Ok(())
    }

    fn start_recoveries(&mut self) {
        while self.recovery.len() < MAX_CONCURRENT_RECOVERIES {
            let Some(id) = self.recovery_pending.pop_front() else {
                break;
            };

            let state = Arc::clone(&self.state);
            let sender = self.data_sender.clone();
            self.recovery.push(Box::pin(
                async move { (id, state.recover(id, sender).await) },
            ));
        }
    }

    fn dispatch(&mut self, message: OKXWsMessage) {
        if matches!(message, OKXWsMessage::Reconnected) {
            let http = self.state.http.clone();
            let emitter = self.emitter.clone();
            let account_id = self.account_id;
            let account_type = self.account_type;
            self.account_refreshes.push(Box::pin(async move {
                match http.request_account_state(account_id, account_type).await {
                    Ok(account_state) => emitter.send_account_state(account_state),
                    Err(e) => {
                        log::warn!("Failed to refresh OKX account state after reconnect: {e}");
                    }
                }
            }));
        }

        dispatch_ws_message(
            message,
            &self.emitter,
            &self.dispatch,
            self.account_id,
            self.account_type,
            &self.state.private.instruments_cache_arc(),
            &mut self.fee_cache,
            &mut self.filled_qty_cache,
            &mut self.order_state_cache,
            self.clock,
        );
    }
}

impl Drop for ExecutionStream {
    fn drop(&mut self) {
        self.state.registration.lock().pending.clear();

        if self.buffered > 0 {
            self.state.fail(format!(
                "incomplete stream recovery with {} pending execution messages",
                self.buffered
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        rc::Rc,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use axum::{Json, Router, routing::get};
    use nautilus_common::{
        enums::Environment,
        messages::ExecutionEvent,
        msgbus::{stubs::get_typed_message_saving_handler, switchboard::get_instrument_topic},
        testing::wait_until_async,
    };
    use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_live::{builder::LiveNodeBuilder, runner::AsyncRunner};
    use nautilus_model::{
        data::Data,
        enums::OmsType,
        events::account::state::BALANCES_SNAPSHOT_COMPLETE_INFO_KEY,
        identifiers::{ClientId, TraderId},
        types::Price,
    };
    use nautilus_network::websocket::TransportBackend;
    use rstest::rstest;
    use serde_json::Value;
    use ustr::Ustr;

    use super::*;
    use crate::common::{
        enums::OKXEnvironment, models::OKXInstrument, parse::parse_instrument_any,
        testing::load_test_json,
    };

    #[tokio::test]
    async fn older_routed_definition_keeps_execution_parser_caches_current() {
        let (http, server) = metadata_server(true).await;
        let (_, state, _events) = stream_with_http(http);
        let payload: Value =
            serde_json::from_str(&load_test_json("http_get_instruments_swap.json")).unwrap();
        let mut raw = payload["data"][0].clone();
        raw["instIdCode"] = serde_json::json!(12345);
        raw["tradeQuoteCcyList"] = serde_json::json!(["USD"]);
        let older: OKXInstrument = serde_json::from_value(raw.clone()).unwrap();
        let older = parse_instrument_any(&older, None, None, UnixNanos::from(10))
            .unwrap()
            .unwrap();
        raw["tickSz"] = serde_json::json!("0.05");
        raw["instIdCode"] = serde_json::json!(56789);
        raw["tradeQuoteCcyList"] = serde_json::json!(["USDC"]);
        let newer: OKXInstrument = serde_json::from_value(raw).unwrap();
        let newer = parse_instrument_any(&newer, None, None, UnixNanos::from(20))
            .unwrap()
            .unwrap();
        let symbol = newer.raw_symbol().inner();

        state.cache_routed_definition(&newer);
        state.cache_routed_definition(&older);
        state.observe(&older, Some(&older));

        for instrument in [
            state.http.get_instrument(&symbol).unwrap(),
            state
                .private
                .instruments_cache_arc()
                .get_cloned(&symbol)
                .unwrap(),
            state
                .business
                .instruments_cache_arc()
                .get_cloned(&symbol)
                .unwrap(),
        ] {
            assert_eq!(instrument.ts_init(), newer.ts_init());
            assert_eq!(instrument.price_increment(), Price::from("0.05"));
        }

        assert_eq!(state.http.inst_id_code(&symbol), Some(56789));
        assert_eq!(
            state.http.trade_quote_ccy_list(&symbol),
            Some(vec![Ustr::from("USDC")])
        );
        assert_eq!(state.private.get_inst_id_code(&symbol), Some(56789));
        assert_eq!(state.business.get_inst_id_code(&symbol), Some(56789));

        server.abort();
    }

    #[rstest]
    #[case::cold(false, false)]
    #[case::cached(true, false)]
    #[case::superseded(false, true)]
    #[tokio::test]
    async fn recovery_publishes_complete_websocket_metadata_after_registration(
        #[case] cached: bool,
        #[case] superseded: bool,
    ) {
        let (http, server) = metadata_server(true).await;

        if cached {
            let response: Value =
                serde_json::from_str(&load_test_json("http_get_instruments_swap.json")).unwrap();
            let mut raw: OKXInstrument = serde_json::from_value(
                response["data"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["instId"] == "BTC-USDT-SWAP")
                    .unwrap()
                    .clone(),
            )
            .unwrap();
            raw.inst_id_code = None;
            let instrument = parse_instrument_any(&raw, None, None, UnixNanos::default())
                .unwrap()
                .unwrap();
            http.cache_instrument(instrument);
        }

        let (worker, state, _events) = stream_with_http(http);
        let (sender, mut data) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTC-USDT-SWAP.OKX");

        let trader_id = TraderId::from("STREAM-001");
        let node = superseded.then(|| {
            LiveNodeBuilder::new(trader_id, Environment::Live)
                .unwrap()
                .build()
                .unwrap()
        });

        if let Some(node) = &node {
            state.register_completion(ExecutionClientCore::new(
                trader_id,
                ClientId::from("OKX"),
                id.venue,
                OmsType::Netting,
                AccountId::from("OKX-001"),
                AccountType::Margin,
                None,
                Rc::clone(&node.kernel().cache),
            ));
        }

        if cached {
            let instrument = state.http.get_instrument(&id.symbol.inner()).unwrap();
            state.observe(&instrument, Some(&instrument));
            assert!(
                !state.ready(id),
                "cached definitions without numeric codes are not manageable"
            );
        }

        let recovery = state.recover(id, Some(sender.into()));
        tokio::pin!(recovery);
        let instrument = tokio::select! {
            _ = &mut recovery => panic!("recovery completed before metadata registration"),
            event = data.recv() => {
                let DataEvent::Data(Data::Instrument(instrument)) = event.unwrap() else { panic!("expected metadata"); };
                instrument
            }
        };
        assert!(futures_util::poll!(&mut recovery).is_pending());

        let mut accepted = (*instrument).clone();
        if let Some(node) = &node {
            let InstrumentAny::CryptoPerpetual(definition) = &mut accepted else {
                panic!("expected swap instrument");
            };

            definition.ts_init = UnixNanos::from(definition.ts_init.as_u64() + 1);
            definition.price_precision = 2;
            definition.price_increment = Price::from("0.05");

            node.kernel()
                .cache
                .borrow_mut()
                .add_instrument(accepted.clone())
                .unwrap();
            let (handler, published) = get_typed_message_saving_handler::<InstrumentAny>(None);
            msgbus::subscribe_instruments(get_instrument_topic(id).into(), handler, None);
            AsyncRunner::handle_data_event(DataEvent::Data(Data::Instrument(instrument)));
            assert!(!state.ready(id));

            let completion = data.try_recv().unwrap();

            let DataEvent::ResponseEnvelope(envelope) = &completion else {
                panic!("expected recovery completion");
            };

            let completion_id = *envelope.correlation_id();
            AsyncRunner::handle_data_event(completion);
            assert!(published.get_messages().is_empty());
            assert!(
                msgbus::get_message_bus()
                    .borrow()
                    .get_response_handler(&completion_id)
                    .is_some()
            );

            state.unregister_completion();
            assert!(
                msgbus::get_message_bus()
                    .borrow()
                    .get_response_handler(&completion_id)
                    .is_none()
            );
        } else {
            state.observe(&accepted, Some(&accepted));
        }

        assert!(state.ready(id));
        recovery.await.unwrap();
        state.recover(id, None).await.unwrap();
        for ws in [&state.private, &state.business] {
            assert_eq!(ws.get_inst_id_code(&id.symbol.inner()), Some(10459));
            assert_eq!(
                serde_json::to_value(
                    ws.instruments_cache_arc()
                        .get_cloned(&id.symbol.inner())
                        .unwrap()
                )
                .unwrap(),
                serde_json::to_value(&accepted).unwrap()
            );
        }

        assert!(state.is_healthy());
        drop(worker);
        server.abort();
    }

    #[rstest]
    #[case::orders(false)]
    #[case::positions(true)]
    #[tokio::test]
    async fn recovery_limits_concurrent_instruments(#[case] positions: bool) {
        let (mut worker, state, mut events) = stream_with_http(OKXHttpClient::default());

        let fixture: Value = serde_json::from_str(&load_test_json(if positions {
            "ws_positions.json"
        } else {
            "ws_orders.json"
        }))
        .unwrap();

        for index in 0..66 {
            let mut row = fixture["data"][0].clone();
            row["instId"] = serde_json::json!(format!("UNKNOWN-{index}"));

            let message = if positions {
                OKXWsMessage::Positions(serde_json::json!([row]))
            } else {
                OKXWsMessage::Orders(vec![serde_json::from_value(row).unwrap()])
            };

            worker.receive(message).unwrap();
        }

        assert_eq!(worker.recovery.len(), 64);
        drop(worker);

        assert_eq!(
            state.take_failure().as_deref(),
            Some("incomplete stream recovery with 66 pending execution messages")
        );
        assert!(events.try_recv().is_err());
    }

    #[rstest]
    #[case::messages(
        false,
        MAX_PENDING_MESSAGES,
        "pending execution message limit exceeded"
    )]
    #[case::instruments(true, MAX_PENDING_MESSAGES, "pending execution message limit exceeded")]
    #[tokio::test]
    async fn recovery_bounds_pending_execution_messages(
        #[case] distinct: bool,
        #[case] limit: usize,
        #[case] expected: &str,
    ) {
        let http = OKXHttpClient::new(
            Some("http://127.0.0.1:1".into()),
            1,
            0,
            1,
            1,
            OKXEnvironment::Demo,
            None,
        )
        .unwrap();
        let (mut worker, state, mut events) = stream_with_http(http);
        let fixture: Value = serde_json::from_str(&load_test_json("ws_orders.json")).unwrap();
        for index in 0..=limit {
            let mut row = fixture["data"][0].clone();
            if distinct {
                row["instId"] = serde_json::json!(format!("UNKNOWN-{index}"));
            }

            let message = serde_json::from_value(row).unwrap();
            let result = worker.receive(OKXWsMessage::Orders(vec![message]));

            if index == limit {
                assert_eq!(result.unwrap_err().to_string(), expected);
            } else {
                result.unwrap();
            }
        }

        drop(worker);
        assert_eq!(
            state.take_failure().unwrap(),
            format!("incomplete stream recovery with {limit} pending execution messages")
        );
        assert!(events.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn aborted_stream_reports_pending_registration() {
        let (http, server) = metadata_server(true).await;
        http.request_instrument(InstrumentId::from("BTC-USDT-SWAP.OKX"))
            .await
            .unwrap();
        let (mut worker, state, mut events) = stream_with_http(http);
        let (sender, mut data) = tokio::sync::mpsc::unbounded_channel();
        worker.data_sender = Some(sender.into());
        let fixture: Value = serde_json::from_str(&load_test_json("ws_orders.json")).unwrap();
        let row = serde_json::from_value(fixture["data"][0].clone()).unwrap();
        let input = futures_util::stream::once(async { OKXWsMessage::Orders(vec![row]) })
            .chain(futures_util::stream::pending());

        let task = tokio::spawn(worker.run(input, CancellationToken::new()));
        assert!(matches!(data.recv().await.unwrap(), DataEvent::Data(_)));
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            state.take_failure().as_deref(),
            Some("incomplete stream recovery with 1 pending execution messages")
        );
        assert!(events.try_recv().is_err());
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn closed_stream_reports_failure() {
        let http = OKXHttpClient::new(
            Some("http://127.0.0.1:1".into()),
            1,
            0,
            1,
            1,
            OKXEnvironment::Demo,
            None,
        )
        .unwrap();
        let (worker, state, mut events) = stream_with_http(http);

        worker
            .run(futures_util::stream::empty(), CancellationToken::new())
            .await;

        assert!(!state.is_healthy());
        assert_eq!(
            state.take_failure().as_deref(),
            Some("execution streams closed unexpectedly")
        );
        assert!(events.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn recovery_rejects_missing_numeric_instrument_code() {
        let (http, server) = metadata_server(false).await;
        let (_worker, state, _events) = stream_with_http(http);
        let (sender, mut data) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let error = state.recover(id, Some(sender.into())).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "required numeric instrument code is unavailable for BTC-USDT-SWAP.OKX"
        );
        assert!(data.try_recv().is_err());
        server.abort();
    }

    #[tokio::test]
    async fn reconnect_requests_complete_account_state() {
        let request_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&request_count);
        let router = Router::new().route(
            "/api/v5/account/balance",
            get(move || {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::Relaxed);
                    let response: Value =
                        serde_json::from_str(&load_test_json("http_get_account_balance.json"))
                            .unwrap();
                    Json(response)
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let http = OKXHttpClient::with_credentials(
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some("test_passphrase".to_string()),
            Some(format!("http://{address}")),
            2,
            0,
            1,
            1,
            OKXEnvironment::Demo,
            None,
        )
        .unwrap();
        let (mut worker, _state, mut events) = stream_with_http(http);
        wait_until_async(
            || async { tokio::net::TcpStream::connect(address).await.is_ok() },
            std::time::Duration::from_secs(5),
        )
        .await;

        worker.dispatch(OKXWsMessage::Reconnected);
        let refresh = worker.account_refreshes.next();
        tokio::time::timeout(std::time::Duration::from_secs(5), refresh)
            .await
            .unwrap();
        wait_until_async(
            || async { request_count.load(Ordering::Relaxed) > 0 },
            std::time::Duration::from_secs(5),
        )
        .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();

        let ExecutionEvent::Account(account_state) = event else {
            panic!("expected account state after reconnect");
        };

        assert_eq!(request_count.load(Ordering::Relaxed), 1);
        assert_eq!(
            account_state
                .info
                .as_ref()
                .and_then(|info| info.get_bool(BALANCES_SNAPSHOT_COMPLETE_INFO_KEY)),
            Some(true),
        );
        server.abort();
    }

    async fn metadata_server(code: bool) -> (OKXHttpClient, tokio::task::JoinHandle<()>) {
        let router = Router::new().route(
            "/api/v5/public/instruments",
            get(move || async move {
                let mut response: Value =
                    serde_json::from_str(&load_test_json("http_get_instruments_swap.json"))
                        .unwrap();
                response["data"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|row| row["instId"] == "BTC-USDT-SWAP");

                if !code {
                    response["data"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("instIdCode");
                }

                Json(response)
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let http = OKXHttpClient::new(
            Some(format!("http://{address}")),
            2,
            0,
            1,
            1,
            OKXEnvironment::Demo,
            None,
        )
        .unwrap();
        (http, server)
    }

    fn stream_with_http(
        http: OKXHttpClient,
    ) -> (
        ExecutionStream,
        Arc<StreamState>,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let [private, business] = std::array::from_fn(|_| {
            OKXWebSocketClient::new(
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                TransportBackend::Sockudo,
                None,
            )
            .unwrap()
        });

        let state = Arc::new(StreamState::new(
            http,
            private,
            business,
            std::iter::empty(),
        ));
        let clock = get_atomic_clock_realtime();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("STREAM-TEST"),
            AccountId::from("OKX-001"),
            AccountType::Margin,
            None,
        );
        let (sender, events) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let worker = ExecutionStream::new(
            Arc::clone(&state),
            Arc::new(WsDispatchState::default()),
            emitter,
            AccountType::Margin,
            clock,
            None,
        );
        (worker, state, events)
    }
}
