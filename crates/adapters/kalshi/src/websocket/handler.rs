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

//! Command correlation and subscription restoration for one multiplexed connection.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use futures_util::{StreamExt, stream::FuturesUnordered};
use nautilus_common::live::dst::time;
use nautilus_network::{RECONNECTED, transport::Message, websocket::WebSocketClient};
use tokio_util::sync::CancellationToken;

use super::{
    client::{KalshiWsEvent, Shared},
    error::KalshiWsError,
    messages::{
        KalshiCommand, KalshiOperation, KalshiSubscription, KalshiUpdateAction, KalshiWsMessage,
    },
    parse::decode_frame,
};
use crate::common::enums::KalshiWsErrorCode;

pub(super) struct KalshiWsHandler {
    shared: Arc<Shared>,
    ws_client: Arc<WebSocketClient>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    cancellation: CancellationToken,
    epoch: u64,
    generation: u64,
    next_id: u64,
    active: BTreeMap<String, ActiveSubscription>,
    pending: BTreeMap<u64, PendingCommand>,
    blocked: BTreeMap<String, Option<KalshiSubscription>>,
    sequences: BTreeMap<u64, u64>,
}

impl KalshiWsHandler {
    pub(super) fn new(
        shared: Arc<Shared>,
        ws_client: Arc<WebSocketClient>,
        receiver: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        cancellation: CancellationToken,
    ) -> Self {
        let generation = shared.generation.fetch_add(1, Ordering::AcqRel);

        Self {
            shared,
            ws_client,
            receiver,
            cancellation,
            epoch: 0,
            generation,
            next_id: 1,
            active: BTreeMap::new(),
            pending: BTreeMap::new(),
            blocked: BTreeMap::new(),
            sequences: BTreeMap::new(),
        }
    }

    pub(super) async fn run(mut self) {
        loop {
            if self.cancellation.is_cancelled() || self.shared.events.is_closed() {
                break;
            }

            let pending = self.pending.len();
            self.pending.retain(|_, command| {
                command
                    .book_cancel
                    .as_ref()
                    .is_none_or(|cancel| !cancel.is_cancelled())
            });

            if self.pending.len() != pending {
                self.reconcile().await;
            }

            let timeout = Duration::from_secs(self.shared.config.ws_timeout_secs);
            let deadline = self
                .pending
                .values()
                .filter(|pending| pending.book_cancel.is_none())
                .map(|pending| pending.deadline)
                .min()
                .unwrap_or_else(|| time::Instant::now() + timeout);
            let mut snapshots: FuturesUnordered<_> = self
                .pending
                .values()
                .filter_map(|pending| pending.book_cancel.clone())
                .map(CancellationToken::cancelled_owned)
                .collect();
            tokio::select! {
                biased;
                () = self.cancellation.cancelled() => break,
                () = self.shared.events.closed() => break,
                Some(()) = snapshots.next() => {},
                () = time::sleep_until(deadline) => {
                    self.expire().await;

                    if self.ws_client.is_closed() { break; }
                }
                command = self.receiver.recv() => {
                    let Some(command) = command else { break; };
                    self.command(command).await;
                }
            }
        }

        self.shared.ready.store(false, Ordering::Release);
        self.shared.auth.fail("Kalshi WebSocket handler stopped");
        self.ws_client.disconnect().await;
    }

    async fn expire(&mut self) {
        let queued = self.receiver.len();
        for _ in 0..queued {
            if self.cancellation.is_cancelled() {
                return;
            }

            let Ok(command) = self.receiver.try_recv() else {
                break;
            };

            self.command(command).await;
        }

        self.expire_pending();
    }

    fn expire_pending(&mut self) {
        if self.pending.values().any(|pending| {
            pending.book_cancel.is_none() && pending.deadline <= time::Instant::now()
        }) {
            self.error("Kalshi WebSocket command acknowledgement timed out");
            self.recover();
        }
    }

    async fn command(&mut self, command: HandlerCommand) {
        if let HandlerCommand::Frame { received, .. } = &command
            && self
                .pending
                .values()
                .any(|pending| pending.book_cancel.is_none() && pending.deadline < *received)
        {
            self.expire_pending();
        }

        match command {
            HandlerCommand::Reconcile => self.reconcile().await,
            HandlerCommand::Snapshot { topic, markets } => self.snapshot(topic, markets).await,
            HandlerCommand::BookSnapshot {
                topic,
                markets,
                attempt,
                cancel,
                completed,
            } => {
                let result = self.book_snapshot(topic, markets, attempt, cancel).await;
                let _ = completed.send(result);
            }
            HandlerCommand::Frame { epoch, message, .. } => self.frame(epoch, message).await,
        }
    }

    async fn reconcile(&mut self) {
        if !self.ws_client.is_active()
            || !self.shared.auth.is_authenticated()
            || !self.owns_connection()
        {
            return;
        }

        let desired = self
            .shared
            .state
            .lock()
            .expect("client state lock is not poisoned")
            .desired
            .clone();
        self.blocked
            .retain(|topic, failed| desired.get(topic) == failed.as_ref());
        let mut topics: BTreeSet<String> = desired.keys().cloned().collect();
        topics.extend(self.active.keys().cloned());
        for topic in topics {
            if self.blocked.contains_key(&topic)
                || self.pending.values().any(|pending| pending.topic == topic)
            {
                continue;
            }

            let wanted = self
                .shared
                .state
                .lock()
                .expect("client state lock is not poisoned")
                .desired
                .get(&topic)
                .cloned();
            let wanted = wanted.as_ref();
            let active = self.active.get(&topic);

            let operation = match (wanted, active) {
                (Some(args), None) => KalshiOperation::Subscribe {
                    channels: [args.channel],
                    market_tickers: args.market_tickers.clone(),
                    use_yes_price: (args.channel == super::messages::KalshiChannel::OrderbookDelta)
                        .then_some(true),
                    send_initial_snapshot: (args.channel == super::messages::KalshiChannel::Ticker)
                        .then_some(true),
                },
                (None, Some(active)) => KalshiOperation::Unsubscribe { sids: [active.sid] },
                (Some(args), Some(active)) if args != &active.args => {
                    let additions: Vec<_> = args
                        .market_tickers
                        .iter()
                        .filter(|market| !active.args.market_tickers.contains(market))
                        .cloned()
                        .collect();

                    let (action, markets) = if additions.is_empty() {
                        (
                            KalshiUpdateAction::DeleteMarkets,
                            active
                                .args
                                .market_tickers
                                .iter()
                                .filter(|market| !args.market_tickers.contains(market))
                                .cloned()
                                .collect(),
                        )
                    } else {
                        (KalshiUpdateAction::AddMarkets, additions)
                    };

                    KalshiOperation::UpdateSubscription {
                        sid: active.sid,
                        market_tickers: markets,
                        action,
                        send_initial_snapshot: (args.channel
                            == super::messages::KalshiChannel::Ticker
                            && action == KalshiUpdateAction::AddMarkets)
                            .then_some(true),
                    }
                }
                (Some(_), Some(_)) => {
                    self.shared.subscriptions.confirm_subscribe(&topic);
                    continue;
                }
                _ => continue,
            };

            if !self.send(topic, operation, wanted.cloned()).await {
                break;
            }
        }

        self.publish_ready();
    }

    async fn send(
        &mut self,
        topic: String,
        operation: KalshiOperation,
        desired: Option<KalshiSubscription>,
    ) -> bool {
        let id = self.next_id;
        self.next_id += 1;
        let command = KalshiCommand { id, operation };
        let wire = serde_json::to_string(&command).expect("command serialization is infallible");
        let timeout = Duration::from_secs(self.shared.config.ws_timeout_secs);
        self.pending.insert(
            id,
            PendingCommand {
                topic,
                operation: command.operation,
                desired,
                deadline: time::Instant::now() + timeout,
                book_cancel: None,
            },
        );

        if !matches!(
            time::timeout(
                timeout,
                self.ws_client
                    .send_text_on_connection(wire, None, self.epoch)
            )
            .await,
            Ok(Ok(()))
        ) {
            if self.owns_connection() {
                self.error("Kalshi WebSocket command send failed");
            }

            self.recover();
            return false;
        }

        true
    }

    async fn snapshot(&mut self, topic: String, markets: Vec<String>) {
        if !self.ws_client.is_active()
            || self.pending.values().any(|pending| pending.topic == topic)
        {
            self.error("Kalshi WebSocket snapshot requires a confirmed subscription");
            return;
        }

        let Some(active) = self.active.get(&topic) else {
            return;
        };

        let operation = KalshiOperation::UpdateSubscription {
            sid: active.sid,
            market_tickers: markets,
            action: KalshiUpdateAction::GetSnapshot,
            send_initial_snapshot: None,
        };

        let desired = Some(active.args.clone());
        self.send(topic, operation, desired).await;
    }

    async fn frame(&mut self, epoch: u64, message: Message) {
        if let Message::Text(text) = &message
            && text.as_ref() == RECONNECTED.as_bytes()
        {
            if epoch <= self.epoch
                || epoch
                    != self
                        .ws_client
                        .connection_epoch_atomic()
                        .load(Ordering::Acquire)
            {
                return;
            }

            self.epoch = epoch;
            self.generation = self.shared.generation.fetch_add(1, Ordering::AcqRel);
            self.active.clear();
            self.pending.clear();
            self.blocked.clear();
            self.sequences.clear();
            self.shared.ready.store(false, Ordering::Release);
            self.shared.subscriptions.reset_after_reconnect();
            self.shared.auth.succeed();
            self.reconcile().await;
            return;
        }

        if epoch != self.epoch
            || !self.ws_client.is_active()
            || !self.shared.auth.is_authenticated()
        {
            return;
        }

        let bytes = match &message {
            Message::Text(text) => text.as_ref(),
            Message::Binary(bytes) => bytes.as_ref(),
            _ => return,
        };

        let decoded = match decode_frame(bytes) {
            Ok(decoded) => decoded,
            Err(_) => {
                self.error("Malformed Kalshi WebSocket frame");
                return;
            }
        };

        if !self.sequence(&decoded) {
            return;
        }

        if decoded.data().is_some() {
            self.dispatch_data(decoded).await;
            return;
        }

        if self.acknowledge(&decoded) {
            let _ = self.shared.events.send(KalshiWsEvent::Message {
                generation: self.generation,
                message: decoded,
            });

            self.reconcile().await;
        }
    }

    async fn dispatch_data(&mut self, decoded: KalshiWsMessage) {
        let snapshot_completed = match &decoded {
            KalshiWsMessage::OrderbookSnapshot(data) if data.id.is_some() => {
                let Some(completed) = self.acknowledge_snapshot(data) else {
                    return;
                };

                completed
            }
            _ => false,
        };

        if let Some((channel, data)) = decoded.data() {
            let topic = self.active.iter().find_map(|(topic, active)| {
                (active.sid == data.sid && active.args.channel == channel).then_some(topic)
            });

            if let Some(topic) = topic
                && self
                    .shared
                    .state
                    .lock()
                    .expect("client state lock is not poisoned")
                    .desired
                    .contains_key(topic)
            {
                let _ = self.shared.events.send(KalshiWsEvent::Message {
                    generation: self.generation,
                    message: decoded,
                });
            }

            if snapshot_completed {
                self.reconcile().await;
            }
        }
    }

    async fn book_snapshot(
        &mut self,
        topic: String,
        markets: Vec<String>,
        attempt: u64,
        cancel: CancellationToken,
    ) -> Result<(), KalshiWsError> {
        if cancel.is_cancelled()
            || !self.ws_client.is_active()
            || !self.shared.auth.is_authenticated()
            || !self.owns_connection()
        {
            return Err(KalshiWsError::Client(
                "snapshot connection is unavailable".into(),
            ));
        }

        let Some(active) = self.active.get(&topic) else {
            if self.blocked.contains_key(&topic) {
                self.recover();
            }

            return Err(KalshiWsError::Client(
                "subscription is not confirmed".into(),
            ));
        };

        if self.pending.values().any(|pending| {
            pending.topic == topic
                && !matches!(
                    pending.operation,
                    KalshiOperation::UpdateSubscription {
                        action: KalshiUpdateAction::GetSnapshot,
                        ..
                    }
                )
        }) {
            return Err(KalshiWsError::Client(
                "subscription has a pending command".into(),
            ));
        }

        let sid = active.sid;
        let desired = Some(active.args.clone());
        let id = self.next_id;

        let operation = KalshiOperation::UpdateSubscription {
            sid,
            market_tickers: markets,
            action: KalshiUpdateAction::GetSnapshot,
            send_initial_snapshot: None,
        };

        let sent = tokio::select! {
            biased;
            () = cancel.cancelled() => false,
            sent = self.send(topic, operation, desired) => sent,
        };

        if !sent || cancel.is_cancelled() || !self.owns_connection() {
            self.pending.remove(&id);
            return Err(KalshiWsError::Client(
                "snapshot write did not complete on the intended connection".into(),
            ));
        }

        self.pending
            .get_mut(&id)
            .expect("snapshot write is pending")
            .book_cancel = Some(cancel);

        self.shared
            .events
            .send(KalshiWsEvent::BookSnapshotRequested {
                generation: self.generation,
                sid,
                id,
                attempt,
            })
            .map_err(|_| KalshiWsError::Client("book event receiver closed".into()))?;

        Ok(())
    }

    fn sequence(&mut self, message: &KalshiWsMessage) -> bool {
        let position = match message {
            KalshiWsMessage::Ok {
                sid: Some(sid),
                seq: Some(seq),
                ..
            }
            | KalshiWsMessage::Unsubscribed { sid, seq, .. } => Some((*sid, *seq)),
            _ => message
                .data()
                .and_then(|(_, data)| data.seq.map(|seq| (data.sid, seq))),
        };

        let Some((sid, seq)) = position else {
            return true;
        };

        let Some(previous) = self.sequences.get_mut(&sid) else {
            return true;
        };

        let Some(gap) = advance_sid_sequence(*previous, seq) else {
            return false;
        };

        if gap {
            log::warn!("Kalshi book sequence gap on SID {sid}: after {previous}, received {seq}");

            let _ = self.shared.events.send(KalshiWsEvent::BookSequenceGap {
                generation: self.generation,
                sid,
            });
        }

        *previous = seq;
        true
    }

    fn acknowledge_snapshot(&mut self, data: &super::messages::KalshiDataMessage) -> Option<bool> {
        let id = data.id?;
        let pending = self.pending.get_mut(&id)?;

        let KalshiOperation::UpdateSubscription {
            sid,
            market_tickers,
            action: KalshiUpdateAction::GetSnapshot,
            ..
        } = &mut pending.operation
        else {
            return None;
        };

        if *sid != data.sid {
            return None;
        }

        let market = data
            .msg
            .get("market_ticker")
            .and_then(serde_json::Value::as_str)?;

        if !market_tickers.iter().any(|ticker| ticker == market) {
            return None;
        }

        market_tickers.retain(|ticker| ticker != market);
        let completed = market_tickers.is_empty();
        if completed {
            self.pending.remove(&id);
        }

        Some(completed)
    }

    fn acknowledge(&mut self, message: &KalshiWsMessage) -> bool {
        match message {
            KalshiWsMessage::Subscribed { id, msg } => self.acknowledge_subscribe(id, msg),
            KalshiWsMessage::Unsubscribed { id, sid, .. } => self.acknowledge_unsubscribe(id, sid),
            KalshiWsMessage::Ok {
                id: Some(id),
                sid: Some(sid),
                msg,
                ..
            } => self.acknowledge_update(id, sid, msg),
            KalshiWsMessage::Error { id, sid, msg } => self.acknowledge_error(id, sid, msg),
            _ => false,
        }
    }

    fn acknowledge_subscribe(&mut self, id: &u64, msg: &super::messages::KalshiSubscribed) -> bool {
        let Some(pending) = self.pending.get(id) else {
            return false;
        };

        let KalshiOperation::Subscribe { channels, .. } = &pending.operation else {
            return false;
        };

        if channels[0] != msg.channel
            || msg.sid == 0
            || self.active.values().any(|active| active.sid == msg.sid)
        {
            return false;
        }

        let pending = self.pending.remove(id).expect("pending command exists");
        let args = pending.desired.expect("subscribe retains arguments");
        if self
            .shared
            .state
            .lock()
            .expect("client state lock is not poisoned")
            .desired
            .get(&pending.topic)
            == Some(&args)
        {
            self.shared.subscriptions.confirm_subscribe(&pending.topic);
        }

        self.active
            .insert(pending.topic, ActiveSubscription { sid: msg.sid, args });
        if msg.channel == super::messages::KalshiChannel::OrderbookDelta {
            self.sequences.insert(msg.sid, 0);
        }

        true
    }

    fn acknowledge_unsubscribe(&mut self, id: &Option<u64>, sid: &u64) -> bool {
        let id = id.or_else(|| self.pending.iter().find_map(|(id, pending)| matches!(pending.operation, KalshiOperation::Unsubscribe { sids } if sids[0] == *sid).then_some(*id)));

        let Some(id) = id else {
            return false;
        };

        let Some(pending) = self.pending.get(&id) else {
            return false;
        };

        if !matches!(pending.operation, KalshiOperation::Unsubscribe { sids } if sids[0] == *sid) {
            return false;
        }

        let pending = self.pending.remove(&id).expect("pending command exists");
        self.active.remove(&pending.topic);
        self.sequences.remove(sid);
        self.shared
            .subscriptions
            .confirm_unsubscribe(&pending.topic);
        true
    }

    fn acknowledge_update(
        &mut self,
        id: &u64,
        sid: &u64,
        msg: &Option<super::messages::KalshiUpdated>,
    ) -> bool {
        let Some(pending) = self.pending.get(id) else {
            return false;
        };

        let KalshiOperation::UpdateSubscription {
            sid: expected,
            market_tickers,
            action,
            ..
        } = &pending.operation
        else {
            return false;
        };

        if sid != expected || *action == KalshiUpdateAction::GetSnapshot {
            return false;
        }

        let Some(active) = self.active.get_mut(&pending.topic) else {
            return false;
        };

        if *action != KalshiUpdateAction::GetSnapshot {
            let mut updated = active.args.market_tickers.clone();
            match action {
                KalshiUpdateAction::AddMarkets => updated.extend(market_tickers.iter().cloned()),
                KalshiUpdateAction::DeleteMarkets => {
                    updated.retain(|market| !market_tickers.contains(market));
                }
                KalshiUpdateAction::GetSnapshot => unreachable!(),
            }

            updated.sort();
            updated.dedup();

            let Some(msg) = msg else {
                return false;
            };

            let mut confirmed = msg.market_tickers.clone();
            confirmed.sort();
            if confirmed != updated {
                return false;
            }

            active.args.market_tickers = updated;
        }

        let pending = self.pending.remove(id).expect("pending command exists");
        if self
            .shared
            .state
            .lock()
            .expect("client state lock is not poisoned")
            .desired
            .get(&pending.topic)
            == Some(&active.args)
        {
            self.shared.subscriptions.confirm_subscribe(&pending.topic);
        }

        true
    }

    fn acknowledge_error(
        &mut self,
        id: &Option<u64>,
        sid: &Option<u64>,
        msg: &super::messages::KalshiServerError,
    ) -> bool {
        let pending = id.and_then(|id| self.pending.get(&id));
        if id.is_some() && pending.is_none() {
            return false;
        }

        let expected_sid = pending.and_then(|pending| match pending.operation {
            KalshiOperation::Subscribe { .. } => None,
            KalshiOperation::Unsubscribe { sids } => Some(sids[0]),
            KalshiOperation::UpdateSubscription { sid, .. } => Some(sid),
        });

        if let Some(sid) = sid {
            if id.is_some() {
                if expected_sid != Some(*sid) {
                    return false;
                }
            } else if !self.active.values().any(|active| active.sid == *sid) {
                return false;
            }
        }

        let terminal = matches!(
            KalshiWsErrorCode::from_repr(msg.code),
            Some(
                KalshiWsErrorCode::AuthenticationRequired
                    | KalshiWsErrorCode::ChannelError
                    | KalshiWsErrorCode::SubscriptionBufferOverflow
            )
        );

        if !terminal && pending.is_some_and(|pending| pending.book_cancel.is_some()) {
            self.pending
                .remove(&id.expect("snapshot command has an ID"));
            return true;
        }

        self.error(&format!("Kalshi WebSocket protocol error {}", msg.code));

        if terminal {
            self.recover();
            return true;
        }

        let Some(id) = id else {
            return true;
        };

        let pending = self.pending.remove(id).expect("pending command exists");
        self.shared.subscriptions.mark_failure(&pending.topic);
        self.blocked.insert(pending.topic, pending.desired);
        true
    }

    fn recover(&mut self) {
        self.shared.ready.store(false, Ordering::Release);
        self.shared.auth.invalidate();
        self.pending.clear();

        if self.owns_connection() {
            let _ = self.ws_client.request_reconnect();
        }
    }

    fn owns_connection(&self) -> bool {
        self.epoch
            == self
                .ws_client
                .connection_epoch_atomic()
                .load(Ordering::Acquire)
    }

    fn publish_ready(&self) {
        let state = self
            .shared
            .state
            .lock()
            .expect("client state lock is not poisoned");

        let ready = self.pending.is_empty()
            && self.owns_connection()
            && self.ws_client.is_active()
            && self.shared.auth.is_authenticated()
            && self.active.len() == state.desired.len()
            && state.desired.iter().all(|(topic, args)| {
                self.active
                    .get(topic)
                    .is_some_and(|active| &active.args == args)
            });

        if ready && !self.shared.ready.swap(true, Ordering::AcqRel) {
            let _ = self.shared.events.send(KalshiWsEvent::Ready {
                generation: self.generation,
            });

            self.shared.ready_notify.notify_waiters();
        }
    }

    fn error(&self, message: &str) {
        log::warn!("{message}");

        let _ = self.shared.events.send(KalshiWsEvent::Error {
            generation: self.generation,
            message: message.to_owned(),
        });
    }
}

pub(super) enum HandlerCommand {
    Reconcile,
    Snapshot {
        topic: String,
        markets: Vec<String>,
    },
    BookSnapshot {
        topic: String,
        markets: Vec<String>,
        attempt: u64,
        cancel: CancellationToken,
        completed: tokio::sync::oneshot::Sender<Result<(), KalshiWsError>>,
    },
    Frame {
        epoch: u64,
        received: time::Instant,
        message: Message,
    },
}

struct ActiveSubscription {
    sid: u64,
    args: KalshiSubscription,
}

struct PendingCommand {
    topic: String,
    operation: KalshiOperation,
    desired: Option<KalshiSubscription>,
    deadline: time::Instant,
    book_cancel: Option<CancellationToken>,
}

/// Returns how `seq` advances from `previous`.
///
/// `None` means `seq` is not strictly newer and the frame must be ignored. `Some(false)` means
/// `seq` is the next value. `Some(true)` means at least one sequence value was skipped.
#[must_use]
pub fn advance_sid_sequence(previous: u64, seq: u64) -> Option<bool> {
    if seq <= previous {
        return None;
    }

    Some(previous.checked_add(1) != Some(seq))
}

#[cfg(test)]
mod sequence_tests {
    use rstest::rstest;

    use super::advance_sid_sequence;

    #[rstest]
    fn advance_sid_sequence_rejects_stale_values_and_reports_gaps() {
        assert_eq!(advance_sid_sequence(4, 4), None);
        assert_eq!(advance_sid_sequence(4, 3), None);
        assert_eq!(advance_sid_sequence(4, 5), Some(false));
        assert_eq!(advance_sid_sequence(4, 7), Some(true));
        assert_eq!(advance_sid_sequence(u64::MAX, u64::MAX), None);
    }
}

#[cfg(all(test, not(all(feature = "simulation", madsim))))]
mod tests {
    use std::{
        sync::{Arc, atomic::Ordering},
        time::Duration,
    };

    use futures_util::{SinkExt, StreamExt};
    use nautilus_common::live::dst::{task, time};
    use nautilus_network::{
        RECONNECTED,
        dst::net::TcpListener,
        transport::Message as TransportMessage,
        websocket::{WebSocketClient, auth::AuthState},
    };
    use rstest::rstest;
    use tokio_tungstenite::{accept_async, tungstenite::Message};
    use tokio_util::sync::CancellationToken;

    use super::{HandlerCommand, KalshiWsHandler};
    use crate::{
        config::KalshiDataClientConfig,
        websocket::{
            client::{
                KalshiWsEvent,
                tests::{shared, transport_config},
            },
            messages::{
                KalshiChannel, KalshiDataMessage, KalshiOperation, KalshiSubscription,
                KalshiUpdated, KalshiWsMessage,
            },
        },
    };

    #[allow(
        dead_code,
        reason = "shared key fixtures also serve authentication tests"
    )]
    mod keys {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/integration/keys.rs"
        ));
    }

    #[tokio::test]
    async fn stale_send_leaves_healthy_replacement_active() {
        time::timeout(Duration::from_secs(15), async {
            let (listener, config) = peer_config().await;
            let peer = task::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                ws_client.close(None).await.unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                assert!(matches!(
                    ws_client.next().await,
                    None | Some(Err(_) | Ok(Message::Close(_)))
                ));
            });

            let (shared, mut events) = shared(config.clone());
            let (ws_client, receiver) = reconnected_transport(&config, 1).await;
            ws_client.set_auth_tracker(shared.auth.clone(), false);

            let mut handler = KalshiWsHandler::new(
                shared,
                Arc::clone(&ws_client),
                receiver,
                CancellationToken::new(),
            );

            let args = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-A".into()],
            };

            let operation = KalshiOperation::Subscribe {
                channels: [args.channel],
                market_tickers: args.market_tickers.clone(),
                use_yes_price: Some(true),
                send_initial_snapshot: None,
            };

            let sent = handler.send(args.topic(), operation, Some(args)).await;
            assert!(!sent);
            assert!(ws_client.is_active());
            assert_eq!(
                ws_client.connection_epoch_atomic().load(Ordering::Acquire),
                1
            );
            assert_eq!(
                events.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            );
            ws_client.disconnect().await;
            peer.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn obsolete_reconnect_notification_does_not_authenticate_current_connection() {
        time::timeout(Duration::from_secs(15), async {
            let (listener, config) = peer_config().await;

            let peer = task::spawn(async move {
                for _ in 0..2 {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut ws_client = accept_async(stream).await.unwrap();
                    ws_client.close(None).await.unwrap();
                }

                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                assert!(matches!(
                    ws_client.next().await,
                    None | Some(Err(_) | Ok(Message::Close(_)))
                ));
            });

            let (shared, mut events) = shared(config.clone());
            let (ws_client, receiver) = reconnected_transport(&config, 2).await;
            ws_client.set_auth_tracker(shared.auth.clone(), false);

            let mut handler = KalshiWsHandler::new(
                Arc::clone(&shared),
                Arc::clone(&ws_client),
                receiver,
                CancellationToken::new(),
            );

            let Some(HandlerCommand::Frame { epoch, message, .. }) = handler.receiver.recv().await
            else {
                panic!("missing first reconnect notification");
            };

            assert_eq!(epoch, 1);
            handler.frame(epoch, message).await;
            assert_eq!(shared.auth.auth_state(), AuthState::Unauthenticated);
            assert_eq!(
                events.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            );

            let Some(HandlerCommand::Frame { epoch, message, .. }) = handler.receiver.recv().await
            else {
                panic!("missing current reconnect notification");
            };

            assert_eq!(epoch, 2);
            handler.frame(epoch, message).await;
            assert_eq!(shared.auth.auth_state(), AuthState::Authenticated);
            assert_eq!(
                events.recv().await,
                Some(KalshiWsEvent::Ready { generation: 1 })
            );
            assert!(ws_client.is_active());
            assert_eq!(
                ws_client.connection_epoch_atomic().load(Ordering::Acquire),
                2
            );
            ws_client.disconnect().await;
            peer.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn replay_rechecks_intent_after_each_awaited_send() {
        time::timeout(Duration::from_secs(15), async {
            let (listener, config) = peer_config().await;
            let peer = task::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                let fixtures: serde_json::Value =
                    serde_json::from_str(include_str!("../../test_data/websocket.json")).unwrap();
                let expected = fixtures["book_subscribe"].as_str().unwrap();
                assert_eq!(
                    ws_client.next().await.unwrap().unwrap(),
                    Message::text(expected)
                );
                assert!(matches!(
                    ws_client.next().await,
                    None | Some(Err(_) | Ok(Message::Close(_)))
                ));
            });

            let (shared, _events) = shared(config.clone());

            let first = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-A".into()],
            };

            let removed = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-B".into()],
            };

            {
                let mut state = shared.state.lock().unwrap();
                state.desired.insert(first.topic(), first);
                state.desired.insert(removed.topic(), removed.clone());
            }

            let (ws_client, receiver) = reconnected_transport(&config, 0).await;
            ws_client.set_auth_tracker(shared.auth.clone(), false);
            shared.auth.succeed();

            let mut handler = KalshiWsHandler::new(
                Arc::clone(&shared),
                Arc::clone(&ws_client),
                receiver,
                CancellationToken::new(),
            );

            let mut replay = std::pin::pin!(handler.reconcile());
            assert!(futures_util::poll!(replay.as_mut()).is_pending());
            shared
                .state
                .lock()
                .unwrap()
                .desired
                .remove(&removed.topic());
            replay.await;
            ws_client.disconnect().await;
            peer.await.unwrap();
        })
        .await
        .unwrap();
    }

    async fn peer_config() -> (TcpListener, KalshiDataClientConfig) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let config = KalshiDataClientConfig {
            api_key_id: Some("auth-test-id".into()),
            private_key: Some(keys::rsa_pkcs8().into()),
            base_url_ws: Some(format!("ws://{address}/custom/ws/v3").into()),
            ws_timeout_secs: 5,
            ..Default::default()
        };

        (listener, config)
    }

    async fn reconnected_transport(
        config: &KalshiDataClientConfig,
        expected_epoch: u64,
    ) -> (
        Arc<WebSocketClient>,
        tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    ) {
        let mut config = transport_config(config);
        // Exercise shared epoch handling independently of Kalshi's disabled reconnect policy.
        config.reconnect_max_attempts = None;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let changed = Arc::new(tokio::sync::Notify::new());
        let notification = Arc::clone(&changed);

        let ws_client = WebSocketClient::epoch_builder()
            .config(config)
            .epoch_handler(Arc::new(move |epoch, message| {
                let reconnect = message.as_text() == Some(RECONNECTED);

                let _ = sender.send(HandlerCommand::Frame {
                    epoch,
                    received: time::Instant::now(),
                    message,
                });

                if reconnect {
                    notification.notify_one();
                }
            }))
            .connect()
            .await
            .unwrap();

        while ws_client.connection_epoch_atomic().load(Ordering::Acquire) < expected_epoch {
            changed.notified().await;
        }

        (Arc::new(ws_client), receiver)
    }

    #[tokio::test]
    async fn sequence_is_shared_by_markets_and_control_replies_before_correlation() {
        let (listener, config) = peer_config().await;
        let peer = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws_client = accept_async(stream).await.unwrap();
            let _ = ws_client.next().await;
        });

        let (shared, mut events) = shared(config.clone());
        let (ws_client, receiver) = reconnected_transport(&config, 0).await;

        let mut handler = KalshiWsHandler::new(
            shared,
            Arc::clone(&ws_client),
            receiver,
            CancellationToken::new(),
        );

        handler.sequences.insert(7, 0);
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/books.json")).unwrap();

        let data = |seq, market: &str| {
            let mut msg = fixture["snapshot"].clone();
            msg["market_ticker"] = market.into();
            KalshiWsMessage::OrderbookSnapshot(KalshiDataMessage {
                id: None,
                sid: 7,
                seq: Some(seq),
                sending_ts_ms: None,
                msg,
            })
        };

        assert!(handler.sequence(&data(1, "MARKET-A")));
        assert!(handler.sequence(&data(2, "MARKET-B")));
        assert!(handler.sequence(&KalshiWsMessage::Ok {
            id: Some(999),
            sid: Some(7),
            seq: Some(3),
            sending_ts_ms: None,
            msg: Some(KalshiUpdated {
                market_tickers: vec!["MARKET-A".into(), "MARKET-B".into()]
            })
        }));
        assert!(handler.sequence(&data(4, "MARKET-A")));
        assert!(!handler.sequence(&data(4, "MARKET-A")));
        assert!(!handler.sequence(&data(2, "MARKET-B")));
        assert!(events.try_recv().is_err());
        assert!(handler.sequence(&data(6, "MARKET-B")));
        assert_eq!(
            events.try_recv().unwrap(),
            KalshiWsEvent::BookSequenceGap {
                generation: 0,
                sid: 7
            }
        );
        assert_eq!(handler.sequences.get(&7), Some(&6));
        ws_client.disconnect().await;
        peer.await.unwrap();
    }

    #[rstest]
    #[case::timely_timer(true, true)]
    #[case::late_timer(false, true)]
    #[case::late_receiver(false, false)]
    #[tokio::test]
    async fn queued_acknowledgement_uses_arrival_time(#[case] timely: bool, #[case] timer: bool) {
        time::timeout(Duration::from_secs(5), async {
            let (listener, config) = peer_config().await;
            let peer = task::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                let _ = ws_client.next().await.unwrap().unwrap();
                let fixture: serde_json::Value =
                    serde_json::from_str(include_str!("../../test_data/books.json")).unwrap();
                ws_client
                    .send(Message::text(fixture["subscribed"].to_string()))
                    .await
                    .unwrap();
                let _ = ws_client.next().await;
            });

            let (shared, mut events) = shared(config.clone());

            let args = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-A".into()],
            };

            shared
                .state
                .lock()
                .unwrap()
                .desired
                .insert(args.topic(), args.clone());
            let (ws_client, receiver) = reconnected_transport(&config, 0).await;
            ws_client.set_auth_tracker(shared.auth.clone(), false);
            shared.auth.succeed();
            let cancellation = CancellationToken::new();

            let mut handler = KalshiWsHandler::new(
                shared,
                Arc::clone(&ws_client),
                receiver,
                cancellation.clone(),
            );

            let operation = KalshiOperation::Subscribe {
                channels: [args.channel],
                market_tickers: args.market_tickers.clone(),
                use_yes_price: Some(true),
                send_initial_snapshot: None,
            };

            assert!(handler.send(args.topic(), operation, Some(args)).await);
            let frame = handler.receiver.recv().await.unwrap();

            let HandlerCommand::Frame { received, .. } = &frame else {
                panic!("expected acknowledgement frame");
            };

            let deadline = if timely {
                *received + Duration::from_nanos(1)
            } else {
                *received - Duration::from_nanos(1)
            };

            assert!(deadline < time::Instant::now());
            handler.pending.get_mut(&1).unwrap().deadline = deadline;

            if timer {
                let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
                sender.send(frame).unwrap();
                handler.receiver = receiver;
                handler.expire().await;
            } else {
                handler.command(frame).await;
            }

            let first = events.try_recv().unwrap();

            if timely {
                assert!(matches!(
                    first,
                    KalshiWsEvent::Message {
                        message: KalshiWsMessage::Subscribed {
                            id: 1,
                            msg: crate::websocket::messages::KalshiSubscribed { sid: 7, .. }
                        },
                        generation: 0
                    }
                ));
            } else {
                assert_eq!(
                    first,
                    KalshiWsEvent::Error {
                        generation: 0,
                        message: "Kalshi WebSocket command acknowledgement timed out".into()
                    }
                );
            }

            ws_client.disconnect().await;
            peer.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn frames_arriving_during_expiry_cannot_extend_the_drain() {
        time::timeout(Duration::from_secs(5), async {
            let (listener, config) = peer_config().await;
            let peer = task::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();

                for _ in 0..2 {
                    assert!(matches!(ws_client.next().await, Some(Ok(Message::Text(_)))));
                }

                let _ = ws_client.next().await;
            });

            let (shared, mut events) = shared(config.clone());

            let first = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-A".into()],
            };

            let second = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-B".into()],
            };

            shared
                .state
                .lock()
                .unwrap()
                .desired
                .insert(second.topic(), second);
            let (ws_client, receiver) = reconnected_transport(&config, 0).await;
            ws_client.set_auth_tracker(shared.auth.clone(), false);
            shared.auth.succeed();

            let mut handler = KalshiWsHandler::new(
                shared,
                Arc::clone(&ws_client),
                receiver,
                CancellationToken::new(),
            );

            let operation = KalshiOperation::Subscribe {
                channels: [first.channel],
                market_tickers: first.market_tickers.clone(),
                use_yes_price: Some(true),
                send_initial_snapshot: None,
            };

            assert!(handler.send(first.topic(), operation, Some(first)).await);
            let deadline = time::Instant::now() - Duration::from_secs(1);
            handler.pending.get_mut(&1).unwrap().deadline = deadline;
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            sender.send(HandlerCommand::Reconcile).unwrap();
            handler.receiver = receiver;
            {
                let mut expiry = std::pin::pin!(handler.expire());
                assert!(futures_util::poll!(expiry.as_mut()).is_pending());

                for _ in 0..256 {
                    sender
                        .send(HandlerCommand::Frame {
                            epoch: 0,
                            received: deadline,
                            message: TransportMessage::Ping(Vec::new().into()),
                        })
                        .unwrap();
                }

                expiry.await;
            }

            assert_eq!(handler.receiver.len(), 256);
            assert!(handler.pending.is_empty());
            assert_eq!(
                events.try_recv().unwrap(),
                KalshiWsEvent::Error {
                    generation: 0,
                    message: "Kalshi WebSocket command acknowledgement timed out".into()
                }
            );
            ws_client.disconnect().await;
            peer.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn snapshot_writer_overflow_cannot_confirm_recovery() {
        time::timeout(Duration::from_secs(5), async {
            let (listener, config) = peer_config().await;
            let peer = task::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                while let Some(Ok(message)) = ws_client.next().await {
                    assert!(!matches!(message, Message::Text(_)));

                    if matches!(message, Message::Close(_)) {
                        break;
                    }
                }
            });

            let (shared, mut events) = shared(config.clone());
            shared.auth.succeed();
            let mut transport = transport_config(&config);
            transport.writer_capacity = Some(1);

            let ws_client = Arc::new(
                WebSocketClient::epoch_builder()
                    .config(transport)
                    .epoch_handler(Arc::new(|_, _| {}))
                    .connect()
                    .await
                    .unwrap(),
            );

            let (_sender, receiver) = tokio::sync::mpsc::unbounded_channel();

            let mut handler = KalshiWsHandler::new(
                shared,
                Arc::clone(&ws_client),
                receiver,
                CancellationToken::new(),
            );

            let args = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-A".into()],
            };

            let topic = args.topic();
            handler
                .active
                .insert(topic.clone(), super::ActiveSubscription { sid: 7, args });
            ws_client
                .send_pong_on_connection(Vec::new(), 0)
                .await
                .unwrap();
            let result = handler
                .book_snapshot(topic, vec!["MARKET-A".into()], 41, CancellationToken::new())
                .await;
            assert_eq!(
                result.unwrap_err().to_string(),
                "client error: snapshot write did not complete on the intended connection"
            );
            assert!(handler.pending.is_empty());
            assert_eq!(
                events.try_recv().unwrap(),
                KalshiWsEvent::Error {
                    generation: 0,
                    message: "Kalshi WebSocket command send failed".into()
                }
            );
            assert!(events.try_recv().is_err());
            ws_client.disconnect().await;
            peer.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn concurrent_book_snapshots_keep_independent_pending_correlations() {
        time::timeout(Duration::from_secs(5), async {
            let (listener, config) = peer_config().await;
            let peer = task::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                for (id, market) in [(1, "MARKET-A"), (2, "MARKET-B")] {
                    let Message::Text(wire) = ws_client.next().await.unwrap().unwrap() else {
                        panic!("expected snapshot command")
                    };

                    let command: serde_json::Value = serde_json::from_str(&wire).unwrap();
                    assert_eq!(
                        command,
                        serde_json::json!({
                            "id": id,
                            "cmd": "update_subscription",
                            "params": {"sid": 7, "market_tickers": [market], "action": "get_snapshot"}
                        })
                    );
                }

                let _ = ws_client.next().await;
            });

            let (shared, mut events) = shared(config.clone());
            shared.auth.succeed();
            let (ws_client, receiver) = reconnected_transport(&config, 0).await;

            let mut handler = KalshiWsHandler::new(
                shared,
                Arc::clone(&ws_client),
                receiver,
                CancellationToken::new(),
            );

            let args = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-A".into(), "MARKET-B".into()],
            };

            let topic = args.topic();
            handler
                .active
                .insert(topic.clone(), super::ActiveSubscription { sid: 7, args });
            handler
                .book_snapshot(
                    topic.clone(),
                    vec!["MARKET-A".into()],
                    41,
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            handler
                .book_snapshot(topic, vec!["MARKET-B".into()], 42, CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(handler.pending.len(), 2);

            for (id, attempt) in [(1, 41), (2, 42)] {
                assert_eq!(
                    events.try_recv().unwrap(),
                    KalshiWsEvent::BookSnapshotRequested {
                        generation: 0,
                        sid: 7,
                        id,
                        attempt
                    }
                );
            }

            let reply = |id, market: &str| KalshiDataMessage {
                id: Some(id),
                sid: 7,
                seq: Some(1),
                sending_ts_ms: None,
                msg: serde_json::json!({"market_ticker":market}),
            };

            assert_eq!(handler.acknowledge_snapshot(&reply(2, "MARKET-A")), None);
            assert_eq!(handler.pending.len(), 2);
            assert_eq!(
                handler.acknowledge_snapshot(&reply(2, "MARKET-B")),
                Some(true)
            );
            assert!(handler.pending.contains_key(&1));
            assert_eq!(
                handler.acknowledge_snapshot(&reply(1, "MARKET-A")),
                Some(true)
            );
            assert!(handler.pending.is_empty());
            ws_client.disconnect().await;
            peer.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[rstest]
    #[case::managed_rejection(true, true)]
    #[case::managed_deadline(false, true)]
    #[case::public_rejection(true, false)]
    #[tokio::test]
    async fn snapshot_failures_preserve_their_ownership(
        #[case] rejection: bool,
        #[case] managed: bool,
    ) {
        time::timeout(Duration::from_secs(5), async {
            let (listener, config) = peer_config().await;
            let peer = task::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                let _ = ws_client.next().await;
                let _ = ws_client.next().await;
            });

            let (shared, mut events) = shared(config.clone());
            shared.auth.succeed();
            let (ws_client, receiver) = reconnected_transport(&config, 0).await;

            let mut handler = KalshiWsHandler::new(
                shared,
                Arc::clone(&ws_client),
                receiver,
                CancellationToken::new(),
            );

            let args = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-A".into()],
            };

            let topic = args.topic();
            handler
                .shared
                .state
                .lock()
                .unwrap()
                .desired
                .insert(topic.clone(), args.clone());
            handler
                .active
                .insert(topic.clone(), super::ActiveSubscription { sid: 7, args });

            if managed {
                handler
                    .book_snapshot(
                        topic.clone(),
                        vec!["MARKET-A".into()],
                        41,
                        CancellationToken::new(),
                    )
                    .await
                    .unwrap();
                assert!(matches!(
                    events.try_recv().unwrap(),
                    KalshiWsEvent::BookSnapshotRequested { .. }
                ));
            } else {
                handler
                    .snapshot(topic.clone(), vec!["MARKET-A".into()])
                    .await;
            }

            if rejection {
                let error = serde_json::from_str::<serde_json::Value>(include_str!(
                    "../../test_data/websocket_messages.json"
                ))
                .unwrap()["snapshot_rate_limit"]
                    .clone();
                handler
                    .frame(
                        0,
                        TransportMessage::Text(error.to_string().into_bytes().into()),
                    )
                    .await;

                if !managed {
                    assert!(matches!(
                        events.try_recv().unwrap(),
                        KalshiWsEvent::Error { .. }
                    ));
                }

                assert!(matches!(
                    events.try_recv().unwrap(),
                    KalshiWsEvent::Message {
                        message: KalshiWsMessage::Error { .. },
                        ..
                    }
                ));
                assert_eq!(handler.blocked.contains_key(&topic), !managed);
            } else {
                handler.pending.get_mut(&1).unwrap().deadline =
                    time::Instant::now() - Duration::from_secs(1);
                handler.expire_pending();
                assert!(
                    handler.shared.auth.is_authenticated(),
                    "snapshot deadline forced a transport reconnect"
                );
            }

            ws_client.disconnect().await;
            peer.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn canceled_snapshot_retires_pending_command_before_unsubscribe() {
        time::timeout(Duration::from_secs(5), async {
            let (listener, config) = peer_config().await;
            let peer = task::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                let snapshot = ws_client.next().await.unwrap().unwrap();
                let unsubscribe = ws_client.next().await.unwrap().unwrap();
                let snapshot: serde_json::Value =
                    serde_json::from_str(snapshot.to_text().unwrap()).unwrap();
                let unsubscribe: serde_json::Value =
                    serde_json::from_str(unsubscribe.to_text().unwrap()).unwrap();
                ws_client.close(None).await.unwrap();
                (snapshot, unsubscribe)
            });

            let (shared, mut events) = shared(config.clone());
            shared.auth.succeed();
            let (ws_client, receiver) = reconnected_transport(&config, 0).await;
            let cancellation = CancellationToken::new();

            let mut handler = KalshiWsHandler::new(
                shared,
                Arc::clone(&ws_client),
                receiver,
                cancellation.clone(),
            );

            let args = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-A".into()],
            };

            let topic = args.topic();
            handler
                .active
                .insert(topic.clone(), super::ActiveSubscription { sid: 7, args });
            let snapshot_cancel = CancellationToken::new();
            handler
                .book_snapshot(topic, vec!["MARKET-A".into()], 41, snapshot_cancel.clone())
                .await
                .unwrap();
            let event = events.try_recv().unwrap();
            snapshot_cancel.cancel();

            let running = task::spawn(handler.run());
            let (snapshot, unsubscribe) = peer.await.unwrap();
            cancellation.cancel();
            running.await.unwrap();
            assert!(matches!(
                event,
                KalshiWsEvent::BookSnapshotRequested {
                    generation: 0,
                    sid: 7,
                    id: 1,
                    attempt: 41
                }
            ));
            let fixtures: serde_json::Value =
                serde_json::from_str(include_str!("../../test_data/websocket_messages.json"))
                    .unwrap();
            let expected_snapshot = fixtures["managed_snapshot"].clone();
            let expected_unsubscribe = fixtures["managed_unsubscribe"].clone();
            assert_eq!(snapshot, expected_snapshot);
            assert_eq!(unsubscribe, expected_unsubscribe);
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn running_snapshot_cancellation_restores_readiness_without_a_frame() {
        time::timeout(Duration::from_secs(5), async {
            let (listener, mut config) = peer_config().await;
            config.ws_timeout_secs = 30;

            let peer = task::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                let _ = ws_client.next().await;
                let _ = ws_client.next().await;
            });

            let (shared, mut events) = shared(config.clone());
            shared.auth.succeed();
            let (ws_client, _transport_receiver) = reconnected_transport(&config, 0).await;
            let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
            let cancellation = CancellationToken::new();

            let mut handler = KalshiWsHandler::new(
                Arc::clone(&shared),
                ws_client,
                receiver,
                cancellation.clone(),
            );

            let args = KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers: vec!["MARKET-A".into()],
            };

            let topic = args.topic();
            shared
                .state
                .lock()
                .unwrap()
                .desired
                .insert(topic.clone(), args.clone());
            handler
                .active
                .insert(topic.clone(), super::ActiveSubscription { sid: 7, args });

            let running = task::spawn(handler.run());
            let snapshot_cancel = CancellationToken::new();
            let (completed, result) = tokio::sync::oneshot::channel();
            commands
                .send(HandlerCommand::BookSnapshot {
                    topic,
                    markets: vec!["MARKET-A".into()],
                    attempt: 41,
                    cancel: snapshot_cancel.clone(),
                    completed,
                })
                .unwrap();

            result.await.unwrap().unwrap();
            let requested = events.recv().await.unwrap();
            snapshot_cancel.cancel();
            let ready = time::timeout(Duration::from_secs(1), events.recv()).await;
            let active = shared.ready.load(Ordering::Acquire);
            cancellation.cancel();
            running.await.unwrap();
            peer.await.unwrap();
            assert!(matches!(
                requested,
                KalshiWsEvent::BookSnapshotRequested {
                    generation: 0,
                    sid: 7,
                    id: 1,
                    attempt: 41
                }
            ));
            assert!(
                matches!(ready, Ok(Some(KalshiWsEvent::Ready { generation: 0 }))),
                "cancelled request waited for the native command timeout"
            );
            assert!(active);
        })
        .await
        .unwrap();
    }
}
