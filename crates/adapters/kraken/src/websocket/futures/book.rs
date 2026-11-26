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

use std::{collections::HashMap, sync::Arc};

use futures_util::{StreamExt, future::BoxFuture, stream::FuturesUnordered};
use nautilus_common::live::dst::time::{Duration, Instant, sleep_until};
use nautilus_core::string::secret::SecretString;
use nautilus_live::book::{
    DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS,
    recovery::{BookRecovery, BookRecoveryOutcome},
};
use nautilus_network::{error::SendError, websocket::WebSocketClient};
use parking_lot::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::messages::{
    KrakenFuturesEvent, KrakenFuturesFeed, KrakenFuturesRequest, KrakenFuturesWsMessage,
};
use crate::{common::consts::KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION, websocket::error::KrakenWsError};

type Completion = tokio::sync::oneshot::Sender<Result<(), KrakenWsError>>;

pub(crate) enum BookCommand {
    Subscribe(Ustr, u64),
    Unsubscribe(Ustr),
    Replace {
        symbol: Ustr,
        generation: u64,
        cancel: CancellationToken,
        recovery: Arc<BookRecovery<KrakenWsError>>,
        result: Completion,
    },
}

#[expect(
    clippy::large_enum_variant,
    reason = "Keeping feed messages inline avoids allocating for every market update"
)]
pub(crate) enum BookMessage {
    Data(KrakenFuturesWsMessage, Option<BookStream>),
    Rejected(Ustr, u64),
}

#[derive(Clone, Debug)]
pub(crate) struct BookStream {
    pub(crate) generation: u64,
    retired: Arc<Mutex<bool>>,
}

impl BookStream {
    pub(crate) fn new(generation: u64) -> Self {
        Self {
            generation,
            retired: Arc::default(),
        }
    }

    pub(crate) fn lock_active(&self) -> Option<MutexGuard<'_, bool>> {
        let retired = self.retired.lock();
        (!*retired).then_some(retired)
    }

    pub(crate) fn retire(&self) {
        *self.retired.lock() = true;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Subscribe(u64),
    Unsubscribe,
}

struct Pending {
    id: u64,
    operation: Operation,
    cancel: CancellationToken,
    written_at: Option<Instant>,
    invalid_product_alert: bool,
}

struct Replacement {
    cancel: CancellationToken,
    result: Completion,
}

#[derive(Default)]
struct Subscription {
    desired: Option<u64>,
    wire: Option<BookStream>,
    pending: Option<Pending>,
    replacement: Option<Replacement>,
    recovery: Option<Arc<BookRecovery<KrakenWsError>>>,
    uncertain: bool,
}

pub(super) struct BookSubscriptions {
    books: HashMap<Ustr, Subscription>,
    transitions: FuturesUnordered<BoxFuture<'static, TransitionEvent>>,
    next_id: u64,
    epoch: u64,
}

struct TransitionEvent {
    symbol: Ustr,
    id: u64,
    outcome: TransitionOutcome,
}

enum TransitionOutcome {
    Write(Result<bool, SendError>),
    AckTimeout,
}

impl Default for BookSubscriptions {
    fn default() -> Self {
        Self {
            books: HashMap::new(),
            transitions: FuturesUnordered::new(),
            next_id: 0,
            epoch: 0,
        }
    }
}

impl BookSubscriptions {
    pub(super) fn stream(&self, msg: &KrakenFuturesWsMessage) -> Option<&BookStream> {
        let symbol = book_product(msg)?;
        self.books
            .get(&symbol)?
            .wire
            .as_ref()
            .filter(|s| s.lock_active().is_some())
    }

    pub(super) fn command(&mut self, command: BookCommand, client: &Arc<WebSocketClient>) {
        match command {
            BookCommand::Subscribe(symbol, generation) => {
                let book = self.books.entry(symbol).or_default();
                if book.desired != Some(generation) {
                    book.recovery = None;
                }

                book.desired = Some(generation);
                self.drive(symbol, client);
            }
            BookCommand::Unsubscribe(symbol) => {
                let Some(book) = self.books.get_mut(&symbol) else {
                    return;
                };

                book.desired = None;
                book.retire();
                book.recovery = None;
                book.finish_replacement(Err(inactive()));
                if let Some(pending) = &book.pending
                    && matches!(pending.operation, Operation::Subscribe(_))
                {
                    pending.cancel.cancel();
                }

                self.watch_retiring(symbol);
                self.drive(symbol, client);
            }
            BookCommand::Replace {
                symbol,
                generation,
                cancel,
                recovery,
                result,
            } => {
                self.replace(
                    symbol,
                    generation,
                    Replacement { cancel, result },
                    recovery,
                    client,
                );
            }
        }
    }

    fn replace(
        &mut self,
        symbol: Ustr,
        generation: u64,
        replacement: Replacement,
        recovery: Arc<BookRecovery<KrakenWsError>>,
        client: &Arc<WebSocketClient>,
    ) {
        let Some(book) = self
            .books
            .get_mut(&symbol)
            .filter(|b| b.desired == Some(generation))
        else {
            let _ = replacement.result.send(Err(inactive()));
            return;
        };

        if replacement.cancel.is_cancelled() || !book.retire_for_replacement(&recovery) {
            let _ = replacement.result.send(Err(inactive()));
            return;
        }

        book.recovery = Some(recovery);
        if book
            .pending
            .as_ref()
            .is_some_and(|pending| pending.invalid_product_alert)
        {
            let _ = replacement.result.send(Err(invalid_product()));
            return;
        }

        if !book.replacement_active()
            && !book.uncertain
            && book
                .pending
                .as_ref()
                .is_some_and(|p| p.waiting_for_subscribe(generation, Instant::now()))
        {
            book.replacement = Some(replacement);
            return;
        }

        if book.pending.is_some() || book.uncertain {
            if client.connection_epoch() != self.epoch
                || client.connection_mode().is_reconnect()
                || book.pending.as_ref().is_some_and(|pending| {
                    pending.written_at.is_none()
                        && (pending.cancel.is_cancelled()
                            || pending.operation == Operation::Subscribe(generation))
                })
            {
                book.replacement = Some(replacement);
                return;
            }

            // Futures acknowledgements carry no request ID. Reset the connection
            // before issuing another transition after an unconfirmed write.
            reconnect_unconfirmed(symbol, client);
            let _ = replacement.result.send(Err(KrakenWsError::ConnectionError(
                "Unconfirmed Futures book transition".into(),
            )));
            return;
        }

        book.replacement = Some(replacement);
        self.drive(symbol, client);
    }

    pub(super) fn confirmed(
        &mut self,
        symbol: Ustr,
        subscribed: bool,
        success: bool,
        client: &Arc<WebSocketClient>,
    ) -> Option<u64> {
        let book = self.books.get_mut(&symbol)?;
        let pending = book.pending.as_ref()?;

        if matches!(pending.operation, Operation::Subscribe(_)) != subscribed {
            return None;
        }

        let pending = book.pending.take().expect("operation was checked");
        if !success {
            pending.cancel.cancel();
            book.uncertain = !subscribed;
            let error = KrakenWsError::SubscriptionError("Futures book transition rejected".into());
            book.finish_replacement(Err(error.clone()));
            if book.retires(pending.operation) {
                self.drive(symbol, client);
            } else if let Operation::Subscribe(generation) = pending.operation
                && !book.reject_recovery(error)
            {
                return Some(generation);
            }

            return None;
        }

        let cancelled = pending.cancel.is_cancelled() && !book.replacement_active();
        let retiring = book.retires(pending.operation);
        match pending.operation {
            Operation::Subscribe(generation) => book.confirm_subscribe(generation, cancelled),
            Operation::Unsubscribe => book.wire = None,
        }

        if !cancelled || retiring {
            self.drive(symbol, client);
        }

        None
    }

    pub(super) fn reject_invalid_product(&mut self, symbol: Ustr) -> Option<u64> {
        let book = self.books.get_mut(&symbol)?;
        let pending = book.pending.as_mut()?;
        // Alerts carry no feed, so keep ownership of the pending book write and its reply
        pending.invalid_product_alert = true;
        let operation = pending.operation;

        if book.replacement.is_some() {
            book.finish_replacement(Err(invalid_product()));
            return None;
        }

        let Operation::Subscribe(generation) = operation else {
            return None;
        };

        (book.desired == Some(generation) && !book.reject_recovery(invalid_product()))
            .then_some(generation)
    }

    pub(super) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(super) fn reconnect(&mut self, epoch: u64, client: Option<&Arc<WebSocketClient>>) {
        self.epoch = epoch;
        self.transitions.clear();
        self.books.retain(|_, book| {
            book.retire();
            book.wire = None;
            book.pending = None;
            book.uncertain = false;
            book.desired.is_some()
        });

        if let Some(client) = client.filter(|client| self.owns_connection(client)) {
            let wanted: Vec<_> = self
                .books
                .iter()
                .filter_map(|(&symbol, book)| Some((symbol, book.desired?)))
                .collect();

            for (symbol, generation) in wanted {
                self.queue(
                    symbol,
                    Operation::Subscribe(generation),
                    client,
                    CancellationToken::new(),
                );
            }
        }
    }

    pub(super) fn has_transitions(&self) -> bool {
        !self.transitions.is_empty()
    }

    pub(super) async fn poll_transition(&mut self, client: &Arc<WebSocketClient>) {
        let Some(event) = self.transitions.next().await else {
            return;
        };

        let Some(book) = self.books.get_mut(&event.symbol) else {
            return;
        };

        let Some(pending) = book.pending.as_mut().filter(|p| p.id == event.id) else {
            return;
        };

        let TransitionOutcome::Write(result) = event.outcome else {
            let operation = pending.operation;
            if !pending.invalid_product_alert
                && book.retires(operation)
                && self.owns_connection(client)
            {
                reconnect_unconfirmed(event.symbol, client);
            }

            return;
        };

        match result {
            Ok(true) => {
                pending.written_at = Some(Instant::now());
                self.watch_retiring(event.symbol);
            }
            Err(SendError::ConnectionChanged) => {}
            Err(SendError::BrokenPipe(_) | SendError::WriteTimeout) => {
                // The writer reconnects after a transport failure. Keep the replacement for replay.
                book.uncertain = true;
            }
            result => {
                let cancelled = pending.cancel.is_cancelled();
                let operation = pending.operation;
                let retire = matches!(result, Ok(false)) && book.retires(operation);
                book.pending = None;
                if cancelled && book.replacement_active() {
                    self.drive(event.symbol, client);
                    return;
                }

                book.finish_replacement(Err(result
                    .err()
                    .map_or_else(inactive, |e| KrakenWsError::ConnectionError(e.to_string()))));

                if retire {
                    self.drive(event.symbol, client);
                }
            }
        }
    }

    fn watch_retiring(&self, symbol: Ustr) {
        let book = &self.books[&symbol];
        let Some(pending) = book
            .pending
            .as_ref()
            .filter(|p| !p.invalid_product_alert && book.retires(p.operation))
        else {
            return;
        };

        let Some(deadline) = pending.ack_deadline() else {
            return;
        };

        let id = pending.id;
        self.transitions.push(Box::pin(async move {
            sleep_until(deadline).await;

            TransitionEvent {
                symbol,
                id,
                outcome: TransitionOutcome::AckTimeout,
            }
        }));
    }

    fn drive(&mut self, symbol: Ustr, client: &Arc<WebSocketClient>) {
        if !self.owns_connection(client) {
            return;
        }

        let Some(book) = self.books.get_mut(&symbol) else {
            return;
        };

        if book.uncertain {
            let _ = client.request_reconnect();
            return;
        }

        if book.pending.is_some() {
            return;
        }

        let operation = match (&book.wire, book.desired) {
            (Some(_), None) => Operation::Unsubscribe,
            (Some(wire), Some(generation))
                if wire.generation != generation || wire.lock_active().is_none() =>
            {
                Operation::Unsubscribe
            }
            (None, Some(generation)) => Operation::Subscribe(generation),
            (None, None) => {
                self.books.remove(&symbol);
                return;
            }
            (Some(_), Some(_)) => return,
        };

        let cancel = book
            .replacement
            .as_ref()
            .filter(|replacement| !replacement.cancel.is_cancelled())
            .map_or_else(CancellationToken::new, |r| r.cancel.child_token());
        self.queue(symbol, operation, client, cancel);
    }

    fn owns_connection(&self, client: &WebSocketClient) -> bool {
        client.connection_epoch() == self.epoch && client.connection_mode().is_active()
    }

    fn queue(
        &mut self,
        symbol: Ustr,
        operation: Operation,
        client: &Arc<WebSocketClient>,
        cancel: CancellationToken,
    ) {
        let book = self.books.get_mut(&symbol).expect("queued book is held");
        let payload = match Self::payload(symbol, operation) {
            Ok(payload) => payload,
            Err(e) => {
                book.finish_replacement(Err(e));
                return;
            }
        };

        self.next_id += 1;
        let id = self.next_id;
        book.pending = Some(Pending {
            id,
            operation,
            cancel: cancel.clone(),
            written_at: None,
            invalid_product_alert: false,
        });

        let epoch = self.epoch;
        let client = Arc::clone(client);
        self.transitions.push(Box::pin(async move {
            // Once the send begins, keep its result so a late ACK cannot settle the next transition
            let result = if cancel.is_cancelled() {
                Ok(false)
            } else {
                client
                    .send_text_on_connection(
                        payload.expose_secret().to_owned(),
                        Some(KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()),
                        epoch,
                    )
                    .await
                    .map(|()| true)
            };

            TransitionEvent {
                symbol,
                id,
                outcome: TransitionOutcome::Write(result),
            }
        }));
    }

    fn payload(symbol: Ustr, operation: Operation) -> Result<SecretString, KrakenWsError> {
        let event = match operation {
            Operation::Subscribe(_) => KrakenFuturesEvent::Subscribe,
            Operation::Unsubscribe => KrakenFuturesEvent::Unsubscribe,
        };

        Ok(SecretString::from(serde_json::to_string(
            &KrakenFuturesRequest {
                event,
                feed: KrakenFuturesFeed::Book,
                product_ids: vec![symbol.to_string()],
            },
        )?))
    }
}

impl Pending {
    fn waiting_for_subscribe(&self, generation: u64, now: Instant) -> bool {
        self.operation == Operation::Subscribe(generation)
            && !self.cancel.is_cancelled()
            && self.ack_deadline().is_none_or(|deadline| now < deadline)
    }

    fn ack_deadline(&self) -> Option<Instant> {
        self.written_at
            .map(|written| written + Duration::from_secs(DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS))
    }
}

impl Subscription {
    fn confirm_subscribe(&mut self, generation: u64, cancelled: bool) {
        let stream = BookStream::new(generation);
        let valid = self.desired == Some(generation) && !cancelled;
        if valid {
            if let Some(recovery) = &self.recovery {
                recovery.gate.open();
            }
        } else {
            stream.retire();
        }

        self.wire = Some(stream);
        self.finish_replacement(if valid { Ok(()) } else { Err(inactive()) });
    }

    fn replacement_active(&self) -> bool {
        self.replacement
            .as_ref()
            .is_some_and(|r| !r.cancel.is_cancelled())
    }

    fn reject_recovery(&self, error: KrakenWsError) -> bool {
        let Some(recovery) = &self.recovery else {
            return false;
        };

        let gate = recovery.gate.lock();
        if !recovery.is_running() {
            return false;
        }

        // A closed gate means the attempt still needs its own write result
        if !gate.is_closed() {
            recovery
                .outcome
                .send_replace(BookRecoveryOutcome::Rejected(error));
        }

        true
    }

    fn retires(&self, operation: Operation) -> bool {
        let generation = match operation {
            Operation::Subscribe(generation) => Some(generation),
            Operation::Unsubscribe => self.wire.as_ref().map(|wire| wire.generation),
        };

        self.desired != generation
    }

    fn retire(&self) {
        if let Some(wire) = &self.wire {
            wire.retire();
        }
    }

    fn retire_for_replacement(&self, recovery: &BookRecovery<KrakenWsError>) -> bool {
        let retired = self.wire.as_ref().map(|wire| wire.retired.lock());

        if !recovery.is_running() {
            return false;
        }

        if let Some(mut retired) = retired {
            *retired = true;
        }

        true
    }

    fn finish_replacement(&mut self, result: Result<(), KrakenWsError>) {
        if let Some(replacement) = self.replacement.take() {
            let _ = replacement.result.send(result);
        }
    }
}

pub(crate) fn book_product(msg: &KrakenFuturesWsMessage) -> Option<Ustr> {
    match msg {
        KrakenFuturesWsMessage::BookSnapshot(book) => Some(book.product_id),
        KrakenFuturesWsMessage::BookDelta(book) => Some(book.product_id),
        _ => None,
    }
}

fn inactive() -> KrakenWsError {
    KrakenWsError::Disconnected("Futures book subscription retired".into())
}

fn invalid_product() -> KrakenWsError {
    KrakenWsError::SubscriptionError("Invalid Futures book product".into())
}

fn reconnect_unconfirmed(symbol: Ustr, client: &WebSocketClient) {
    log::warn!("Reconnecting after an unconfirmed Futures book transition for {symbol}");
    let _ = client.request_reconnect();
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use nautilus_network::{
        RECONNECTED,
        ratelimiter::quota::Quota,
        websocket::{WebSocketConfig, channel_epoch_message_handler},
    };
    use rstest::rstest;
    use serde_json::{Value, json};

    use super::*;

    #[rstest]
    fn test_futures_subscribe_reattachment_expires_after_the_acknowledgement_window() {
        let now = Instant::now();
        let window = Duration::from_secs(DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS);

        let pending = Pending {
            id: 1,
            operation: Operation::Subscribe(41),
            cancel: CancellationToken::new(),
            written_at: Some(now),
            invalid_product_alert: false,
        };

        assert!(pending.waiting_for_subscribe(41, now + window - Duration::from_millis(1)));
        assert!(!pending.waiting_for_subscribe(41, now + window));
    }

    #[rstest]
    #[case::in_flight(false)]
    #[case::feedless_rejection_during_turnover(true)]
    #[tokio::test]
    async fn test_retiring_futures_subscribe_keeps_ownership_of_a_started_send(
        #[case] feedless_alert: bool,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = WebSocketConfig::builder()
            .url(format!("ws://{}", listener.local_addr().unwrap()))
            .build()
            .unwrap();

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let first = socket.next().await.unwrap().unwrap();
            assert_eq!(first.to_text().unwrap(), "consume-quota");

            let events = ["subscribe", "unsubscribe", "subscribe"];
            for event in &events[..2 + usize::from(feedless_alert)] {
                let message = socket.next().await.unwrap().unwrap();
                let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                assert_eq!(
                    request,
                    json!({"event": event, "feed": "book", "product_ids": ["PI_XBTUSD"]})
                );
            }

            let _ = socket.next().await;
        });

        let (receive, _events) = channel_epoch_message_handler();
        let keys = KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice();

        let client = Arc::new(
            WebSocketClient::epoch_builder()
                .config(config)
                .epoch_handler(receive)
                .keyed_quotas(vec![(
                    keys[0].to_string(),
                    Quota::per_second(NonZeroU32::MIN).unwrap(),
                )])
                .connect()
                .await
                .unwrap(),
        );

        client
            .send_text_on_connection("consume-quota".into(), Some(keys), 0)
            .await
            .unwrap();

        let symbol = Ustr::from("PI_XBTUSD");
        let mut books = BookSubscriptions::default();
        books.command(BookCommand::Subscribe(symbol, 41), &client);

        {
            let mut send = std::pin::pin!(books.poll_transition(&client));
            assert!(futures_util::poll!(&mut send).is_pending());
        }

        assert_eq!(
            books.books[&symbol].pending.as_ref().unwrap().written_at,
            None
        );

        if feedless_alert {
            assert_eq!(books.reject_invalid_product(symbol), Some(41));
        }

        books.command(BookCommand::Unsubscribe(symbol), &client);
        if feedless_alert {
            books.command(BookCommand::Subscribe(symbol, 42), &client);
        }

        tokio::time::timeout(Duration::from_secs(3), books.poll_transition(&client))
            .await
            .unwrap();
        assert!(
            books.books[&symbol]
                .pending
                .as_ref()
                .unwrap()
                .written_at
                .is_some()
        );
        assert!(client.is_active());

        assert_eq!(books.confirmed(symbol, true, true, &client), None);
        assert_eq!(
            books.books[&symbol].pending.as_ref().unwrap().operation,
            Operation::Unsubscribe
        );
        tokio::time::timeout(Duration::from_secs(3), books.poll_transition(&client))
            .await
            .unwrap();
        assert_eq!(books.confirmed(symbol, false, true, &client), None);

        if feedless_alert {
            assert!(books.books[&symbol].wire.is_none());
            assert_eq!(
                books.books[&symbol].pending.as_ref().unwrap().operation,
                Operation::Subscribe(42)
            );
            tokio::time::timeout(Duration::from_secs(3), books.poll_transition(&client))
                .await
                .unwrap();
            assert_eq!(books.confirmed(symbol, true, true, &client), None);
            let stream = books.books[&symbol].wire.as_ref().unwrap();
            assert_eq!(stream.generation, 42);
            assert!(stream.lock_active().is_some());
            assert!(books.books[&symbol].pending.is_none());
        } else {
            assert!(!books.books.contains_key(&symbol));
        }

        client.disconnect().await;
        server.await.unwrap();
    }

    #[rstest]
    #[case::recovery_after_alert(false)]
    #[case::alert_after_retirement_timer_started(true)]
    #[tokio::test]
    async fn test_invalid_product_alert_keeps_healthy_futures_books_connected(
        #[case] alert_after_retirement: bool,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = WebSocketConfig::builder()
            .url(format!("ws://{}", listener.local_addr().unwrap()))
            .build()
            .unwrap();

        let active = Ustr::from("PF_XBTUSD");
        let invalid = Ustr::from("PI_NOT_A_REAL_CONTRACT");
        let next = Ustr::from("PF_ETHUSD");

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            for product in [active, invalid, next] {
                let message = socket.next().await.unwrap().unwrap();
                let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                assert_eq!(
                    request,
                    json!({"event": "subscribe", "feed": "book", "product_ids": [product.as_str()]})
                );
            }

            let _ = socket.next().await;
        });

        let (receive, _events) = channel_epoch_message_handler();

        let client = Arc::new(
            WebSocketClient::epoch_builder()
                .config(config)
                .epoch_handler(receive)
                .connect()
                .await
                .unwrap(),
        );

        let mut books = BookSubscriptions::default();
        books.command(BookCommand::Subscribe(active, 40), &client);
        books.poll_transition(&client).await;
        assert_eq!(books.confirmed(active, true, true, &client), None);
        books.command(BookCommand::Subscribe(invalid, 41), &client);
        books.poll_transition(&client).await;

        let pending = books
            .books
            .get_mut(&invalid)
            .unwrap()
            .pending
            .as_mut()
            .unwrap();
        let write_id = pending.id;
        pending.written_at =
            Some(Instant::now() - Duration::from_secs(DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS));

        if alert_after_retirement {
            books.command(BookCommand::Unsubscribe(invalid), &client);
            assert!(books.has_transitions());
            assert_eq!(books.reject_invalid_product(invalid), None);
            books.poll_transition(&client).await;
        } else {
            assert_eq!(books.reject_invalid_product(invalid), Some(41));
            let recovery = Arc::new(BookRecovery::default());

            for _ in 0..2 {
                let (result, mut response) = tokio::sync::oneshot::channel();
                books.replace(
                    invalid,
                    41,
                    Replacement {
                        cancel: CancellationToken::new(),
                        result,
                    },
                    recovery.clone(),
                    &client,
                );

                let response = response.try_recv();
                assert!(
                    matches!(response, Ok(Err(KrakenWsError::SubscriptionError(message))) if message == "Invalid Futures book product")
                );
            }

            books.command(BookCommand::Unsubscribe(invalid), &client);
        }

        assert!(!books.has_transitions());
        assert_eq!(books.books[&invalid].pending.as_ref().unwrap().id, write_id);
        assert_eq!(books.books[&invalid].desired, None);
        assert!(client.is_active());
        assert_eq!(client.connection_epoch(), 0);

        books.command(BookCommand::Subscribe(next, 42), &client);
        books.poll_transition(&client).await;
        assert_eq!(books.confirmed(next, true, true, &client), None);

        for (symbol, generation) in [(active, 40), (next, 42)] {
            let stream = books.books[&symbol].wire.as_ref().unwrap();
            assert_eq!(stream.generation, generation);
            assert!(stream.lock_active().is_some());
        }

        client.disconnect().await;
        server.await.unwrap();
    }

    #[rstest]
    #[case::final_unsubscribe(false)]
    #[case::turnover(true)]
    #[tokio::test]
    async fn test_futures_retiring_subscribe_reconnects_after_the_ack_deadline(
        #[case] resubscribe: bool,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = WebSocketConfig::builder()
            .url(format!("ws://{}", listener.local_addr().unwrap()))
            .build()
            .unwrap();

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let request = socket.next().await.unwrap().unwrap();
            let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
            assert_eq!(
                request,
                json!({"event": "subscribe", "feed": "book", "product_ids": ["PI_XBTUSD"]})
            );
            assert!(
                socket
                    .next()
                    .await
                    .is_none_or(|msg| msg.is_err() || msg.unwrap().is_close())
            );
        });

        let (receive, _events) = channel_epoch_message_handler();

        let client = Arc::new(
            WebSocketClient::epoch_builder()
                .config(config)
                .epoch_handler(receive)
                .connect()
                .await
                .unwrap(),
        );

        let symbol = Ustr::from("PI_XBTUSD");
        let mut books = BookSubscriptions::default();
        books.command(BookCommand::Subscribe(symbol, 41), &client);
        books.poll_transition(&client).await;

        let pending = books
            .books
            .get_mut(&symbol)
            .unwrap()
            .pending
            .as_mut()
            .unwrap();
        assert!(pending.written_at.is_some());
        pending.written_at =
            Some(Instant::now() - Duration::from_secs(DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS));
        let old_id = pending.id;
        books.command(BookCommand::Unsubscribe(symbol), &client);
        if resubscribe {
            books.command(BookCommand::Subscribe(symbol, 43), &client);
        }

        let book = &books.books[&symbol];
        assert_eq!(book.desired, resubscribe.then_some(43));
        let pending = book.pending.as_ref().unwrap();
        assert_eq!(pending.id, old_id);
        assert_eq!(pending.operation, Operation::Subscribe(41));
        assert!(client.is_active());
        tokio::time::timeout(Duration::from_secs(2), books.poll_transition(&client))
            .await
            .unwrap();
        assert!(client.is_reconnecting());

        client.disconnect().await;
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_book_writes_wait_for_observed_epoch() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let config = WebSocketConfig::builder()
                .url(format!("ws://{}", listener.local_addr().unwrap()))
                .build()
                .unwrap();

            let server = tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut original = tokio_tungstenite::accept_async(tcp).await.unwrap();
                let old = tokio::spawn(async move { while original.next().await.is_some() {} });
                let (tcp, _) = listener.accept().await.unwrap();
                let mut replacement = tokio_tungstenite::accept_async(tcp).await.unwrap();
                let request = replacement.next().await.unwrap().unwrap();
                let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
                assert_eq!(
                    request,
                    json!({ "event": "subscribe", "feed": "book", "product_ids": ["PI_XBTUSD"] })
                );
                let _ = replacement.next().await;
                old.abort();
            });

            let (receive, mut events) = channel_epoch_message_handler();

            let client = Arc::new(
                WebSocketClient::epoch_builder()
                    .config(config)
                    .epoch_handler(receive)
                    .connect()
                    .await
                    .unwrap(),
            );

            let _ = client.request_reconnect();
            let (epoch, frame) = events.recv().await.unwrap();
            assert_eq!(frame.to_text().unwrap(), RECONNECTED);
            assert_eq!(epoch, 1);

            let symbol = Ustr::from("PI_XBTUSD");
            let mut books = BookSubscriptions::default();
            books.command(BookCommand::Subscribe(symbol, 41), &client);
            books.command(BookCommand::Unsubscribe(symbol), &client);
            books.command(BookCommand::Subscribe(symbol, 57), &client);
            assert!(!books.has_transitions());
            assert!(books.books[&symbol].pending.is_none());

            books.reconnect(epoch, Some(&client));
            assert_eq!(
                books.books[&symbol].pending.as_ref().unwrap().operation,
                Operation::Subscribe(57)
            );
            books.poll_transition(&client).await;
            client.disconnect().await;
            server.await.unwrap();
        })
        .await
        .expect("the desired book replays on the observed connection");
    }

    #[tokio::test]
    async fn test_futures_retry_waits_for_cancelled_pending_write() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = WebSocketConfig::builder()
            .url(format!("ws://{}", listener.local_addr().unwrap()))
            .build()
            .unwrap();

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let mut products = Vec::new();

            while let Some(Ok(frame)) = socket.next().await {
                if frame.is_text() {
                    let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
                    products.push(request["product_ids"][0].as_str().unwrap().to_owned());
                }
            }

            products
        });

        let (receive, _events) = channel_epoch_message_handler();

        let client = Arc::new(
            WebSocketClient::epoch_builder()
                .config(config)
                .epoch_handler(receive)
                .connect()
                .await
                .unwrap(),
        );

        let symbol = Ustr::from("PI_XBTUSD");
        let mut books = BookSubscriptions::default();
        let rejected = Ustr::from("PI_SOLUSD");
        books.command(BookCommand::Subscribe(rejected, 39), &client);
        let pending = books.books[&rejected]
            .pending
            .as_ref()
            .unwrap()
            .cancel
            .clone();
        assert_eq!(books.confirmed(rejected, true, false, &client), Some(39));
        books.command(BookCommand::Unsubscribe(rejected), &client);
        books.poll_transition(&client).await;
        assert!(pending.is_cancelled());
        assert!(!books.books.contains_key(&rejected));

        let retired = Ustr::from("PI_ETHUSD");
        books.command(BookCommand::Subscribe(retired, 40), &client);
        books.command(BookCommand::Unsubscribe(retired), &client);
        books.poll_transition(&client).await;
        assert!(!books.books.contains_key(&retired));
        assert!(client.is_active());
        books.command(BookCommand::Subscribe(symbol, 41), &client);
        books.poll_transition(&client).await;
        let written = books.books[&symbol].pending.as_ref().unwrap().id;

        for cancelled in [true, false] {
            let cancel = CancellationToken::new();
            let (result, mut response) = tokio::sync::oneshot::channel();
            books.command(
                BookCommand::Replace {
                    symbol,
                    generation: 41,
                    cancel: cancel.clone(),
                    recovery: Arc::default(),
                    result,
                },
                &client,
            );

            assert!(matches!(
                response.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            assert_eq!(books.books[&symbol].pending.as_ref().unwrap().id, written);

            if cancelled {
                cancel.cancel();
            }

            assert!(client.is_active());
        }

        assert_eq!(books.confirmed(symbol, true, true, &client), None);

        let cancellation = CancellationToken::new();
        let (result, _response) = tokio::sync::oneshot::channel();
        books.command(
            BookCommand::Replace {
                symbol,
                generation: 41,
                cancel: cancellation.clone(),
                recovery: Arc::default(),
                result,
            },
            &client,
        );

        let old = books.books[&symbol].pending.as_ref().unwrap().id;
        cancellation.cancel();
        let (result, mut response) = tokio::sync::oneshot::channel();
        books.command(
            BookCommand::Replace {
                symbol,
                generation: 41,
                cancel: CancellationToken::new(),
                recovery: Arc::default(),
                result,
            },
            &client,
        );

        assert!(client.is_active());
        assert_eq!(books.books[&symbol].pending.as_ref().unwrap().id, old);
        books.poll_transition(&client).await;
        let next = books.books[&symbol].pending.as_ref().unwrap();
        assert_eq!(next.operation, Operation::Unsubscribe);
        assert_eq!(next.id, old + 1);
        assert!(!next.cancel.is_cancelled());
        assert!(matches!(
            response.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(client.is_active());

        books.command(BookCommand::Unsubscribe(symbol), &client);
        books.command(BookCommand::Subscribe(symbol, 43), &client);
        assert_eq!(books.confirmed(symbol, true, false, &client), None);
        assert_eq!(
            books.books[&symbol].pending.as_ref().unwrap().operation,
            Operation::Unsubscribe
        );
        assert_eq!(books.confirmed(symbol, false, false, &client), None);
        assert_eq!(books.books[&symbol].desired, Some(43));
        assert!(client.is_reconnecting());

        client.disconnect().await;
        let products = server.await.unwrap();
        assert!(!products.iter().any(|product| product == rejected.as_str()));
    }
}
