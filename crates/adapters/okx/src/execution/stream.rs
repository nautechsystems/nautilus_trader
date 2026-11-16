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
use nautilus_common::{live::sender::EventSender, messages::DataEvent};
use nautilus_core::AtomicTime;
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::AccountType,
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    common::parse::instrument_definitions_match,
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

        if !instrument_definitions_match(published, cached) {
            return;
        }

        let id = cached.id();
        self.http.cache_instrument(cached.clone());
        for ws in [&self.private, &self.business] {
            ws.cache_instrument(cached.clone());
            if let Some(code) = self.http.inst_id_code(&id.symbol.inner()) {
                ws.cache_inst_id_code(id.symbol.inner(), code);
            }
        }

        let mut registration = self.registration.lock();
        registration.ready.insert(id);
        if registration
            .pending
            .get(&id)
            .is_some_and(|(expected, _)| instrument_definitions_match(expected, cached))
            && let Some((_, sender)) = registration.pending.remove(&id)
        {
            let _ = sender.send(());
        }
    }

    pub(super) fn is_healthy(&self) -> bool {
        self.registration.lock().failure.is_none()
    }

    pub(super) fn take_failure(&self) -> Option<String> {
        self.registration.lock().failure.take()
    }

    fn ready(&self, id: InstrumentId) -> bool {
        self.registration.lock().ready.contains(&id)
            && self
                .private
                .instruments_cache_arc()
                .contains_key(&id.symbol.inner())
            && (self.http.is_spread(id)
                || self.private.get_inst_id_code(&id.symbol.inner()).is_some())
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
        let sender = sender
            .context("data event sender is unavailable for stream instrument registration")?;
        let (completion, receiver) = tokio::sync::oneshot::channel();
        self.registration
            .lock()
            .pending
            .insert(id, (instrument.clone(), completion));
        sender
            .send(DataEvent::Data(instrument.into()))
            .map_err(|_| anyhow::anyhow!("stream instrument registration receiver closed"))?;
        // Startup buffers data until all execution clients release their engine borrow
        receiver
            .await
            .context("stream instrument registration acknowledgement closed")?;
        Ok(())
    }
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::{Json, Router, routing::get};
    use nautilus_common::{messages::ExecutionEvent, testing::wait_until_async};
    use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::{
        data::Data, events::account::state::BALANCES_SNAPSHOT_COMPLETE_INFO_KEY,
        identifiers::TraderId,
    };
    use nautilus_network::websocket::TransportBackend;
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::common::{
        enums::OKXEnvironment, models::OKXInstrument, parse::parse_instrument_any,
        testing::load_test_json,
    };

    #[rstest]
    #[case::cold(false)]
    #[case::cached(true)]
    #[tokio::test]
    async fn recovery_publishes_complete_websocket_metadata_after_registration(
        #[case] cached: bool,
    ) {
        let (http, server) = metadata_server(true).await;

        if cached {
            let response: Value =
                serde_json::from_str(&load_test_json("http_get_instruments_swap.json")).unwrap();
            let raw: OKXInstrument = serde_json::from_value(
                response["data"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["instId"] == "BTC-USDT-SWAP")
                    .unwrap()
                    .clone(),
            )
            .unwrap();
            let instrument = parse_instrument_any(&raw, None, None, UnixNanos::default())
                .unwrap()
                .unwrap();
            http.cache_instrument(instrument);
        }

        let (worker, state, _events) = stream_with_http(http);
        let (sender, mut data) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTC-USDT-SWAP.OKX");
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
        state.observe(&instrument, Some(&instrument));
        recovery.await.unwrap();

        for ws in [&state.private, &state.business] {
            assert_eq!(ws.get_inst_id_code(&id.symbol.inner()), Some(10459));
            assert_eq!(
                serde_json::to_value(
                    ws.instruments_cache_arc()
                        .get_cloned(&id.symbol.inner())
                        .unwrap()
                )
                .unwrap(),
                serde_json::to_value(&*instrument).unwrap()
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
