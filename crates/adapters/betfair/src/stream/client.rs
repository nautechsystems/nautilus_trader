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

//! Betfair Exchange Stream API client.
//!
//! Connects to the Betfair raw TLS stream (CRLF-delimited JSON), authenticates,
//! and manages market/order subscriptions with automatic clk-based resubscription
//! on reconnection.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use bytes::Bytes;
use nautilus_network::socket::{SocketClient, SocketConfig, TcpMessageHandler, WriterCommand};
use tokio::sync::watch; // tokio-import-ok
use tokio_tungstenite::tungstenite::stream::Mode;

use super::{
    config::BetfairStreamConfig,
    error::BetfairStreamError,
    messages::{
        Authentication, MarketDataFilter, MarketSubscription, OrderFilter, OrderSubscription,
        StreamMarketFilter, StreamMessage, stream_decode,
    },
};
use crate::common::{credential::BetfairCredential, enums::StatusErrorCode};

/// Betfair Exchange Stream API client using raw TLS (CRLF-delimited JSON).
///
/// On connect, authenticates immediately. On reconnection, replays authentication
/// and any active subscriptions with the latest `clk` token for delta resumption.
#[derive(Debug)]
pub struct BetfairStreamClient {
    socket: SocketClient,
    market_sub_tx: watch::Sender<Option<MarketSubscription>>,
    market_clk_tx: watch::Sender<Option<String>>,
    market_initial_clk_tx: watch::Sender<Option<String>>,
    order_sub_tx: watch::Sender<Option<OrderSubscription>>,
    order_clk_tx: watch::Sender<Option<String>>,
    order_initial_clk_tx: watch::Sender<Option<String>>,
    market_active_sub_id: Arc<AtomicU64>,
    order_active_sub_id: Arc<AtomicU64>,
    request_id: AtomicU64,
    // Serialized auth bytes, prepended to every subscription send to guarantee
    // auth-first ordering even when bytes land in the reconnect buffer.
    auth_bytes: Bytes,
    // Set to true by close() to distinguish permanent shutdown from transient reconnect
    closed: AtomicBool,
}

impl BetfairStreamClient {
    /// Connects to the Betfair stream API and authenticates.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or authentication cannot be sent.
    pub async fn connect(
        credential: &BetfairCredential,
        session_token: String,
        handler: TcpMessageHandler,
        config: BetfairStreamConfig,
    ) -> Result<Self, BetfairStreamError> {
        let auth = Authentication::new(credential.app_key().to_string(), session_token);
        let auth_bytes_vec = serde_json::to_vec(&auth)?;
        let auth_bytes = Bytes::from(auth_bytes_vec.clone());
        let mode = if config.use_tls {
            Mode::Tls
        } else {
            Mode::Plain
        };

        let (market_clk_tx, market_clk_rx) = watch::channel(None::<String>);
        let (market_initial_clk_tx, market_initial_clk_rx) = watch::channel(None::<String>);
        let (order_clk_tx, order_clk_rx) = watch::channel(None::<String>);
        let (order_initial_clk_tx, order_initial_clk_rx) = watch::channel(None::<String>);
        let (market_sub_tx, market_sub_rx) = watch::channel(None::<MarketSubscription>);
        let (order_sub_tx, order_sub_rx) = watch::channel(None::<OrderSubscription>);

        // Populated after connect() returns; OnceLock gives lock-free reads thereafter.
        let shared_tx: Arc<OnceLock<tokio::sync::mpsc::UnboundedSender<WriterCommand>>> =
            Arc::new(OnceLock::new());

        // Clone senders for the handler; struct keeps originals to reset on re-subscribe.
        let (market_clk_tx_h, market_initial_clk_tx_h) =
            (market_clk_tx.clone(), market_initial_clk_tx.clone());
        let (order_clk_tx_h, order_initial_clk_tx_h) =
            (order_clk_tx.clone(), order_initial_clk_tx.clone());

        let market_active_sub_id = Arc::new(AtomicU64::new(0));
        let order_active_sub_id = Arc::new(AtomicU64::new(0));
        let market_active_sub_id_h = Arc::clone(&market_active_sub_id);
        let order_active_sub_id_h = Arc::clone(&order_active_sub_id);

        let message_handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
            if let Ok(msg) = stream_decode(data) {
                match &msg {
                    StreamMessage::MarketChange(mcm) => {
                        let active = market_active_sub_id_h.load(Ordering::SeqCst);
                        // Accept only when a subscription is active (active > 0) and
                        // the message carries no id (can't discriminate, e.g. heartbeat)
                        // or its id matches the active subscription. Reject messages that
                        // explicitly carry a different (stale) subscription id.
                        if active > 0 && mcm.id.is_none_or(|id| id == active) {
                            if mcm.clk.is_some() {
                                let _ = market_clk_tx_h.send(mcm.clk.clone());
                            }

                            if mcm.initial_clk.is_some() {
                                let _ = market_initial_clk_tx_h.send(mcm.initial_clk.clone());
                            }
                        }
                    }
                    StreamMessage::OrderChange(ocm) => {
                        let active = order_active_sub_id_h.load(Ordering::SeqCst);
                        if active > 0 && ocm.id.is_none_or(|id| id == active) {
                            if ocm.clk.is_some() {
                                let _ = order_clk_tx_h.send(ocm.clk.clone());
                            }

                            if ocm.initial_clk.is_some() {
                                let _ = order_initial_clk_tx_h.send(ocm.initial_clk.clone());
                            }
                        }
                    }
                    StreamMessage::Status(status) => {
                        // Betfair rejects stale replay tokens with INVALID_CLOCK and then
                        // closes the connection, so a loop of reconnect → same stale clk →
                        // reject would follow unless we clear the clocks here and fall back
                        // to a full-image resubscription on the next reconnect.
                        if status.error_code == Some(StatusErrorCode::InvalidClock) {
                            let _ = market_clk_tx_h.send(None);
                            let _ = market_initial_clk_tx_h.send(None);
                            let _ = order_clk_tx_h.send(None);
                            let _ = order_initial_clk_tx_h.send(None);
                            log::warn!(
                                "Betfair stream INVALID_CLOCK — clocks cleared, \
                                 next reconnect will request a full image",
                            );
                        } else if status.connection_closed {
                            log::error!(
                                "Betfair stream connection closed by server: {:?} — {:?}",
                                status.error_code,
                                status.error_message,
                            );
                        } else if status.error_code.is_some() {
                            log::warn!(
                                "Betfair stream status error: {:?} — {:?}",
                                status.error_code,
                                status.error_message,
                            );
                        }
                    }
                    _ => {}
                }
            }
            handler(data);
        });

        let auth_bytes_reconnect = auth_bytes.clone();
        let shared_tx_reconnect = Arc::clone(&shared_tx);
        let post_reconnection: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let Some(tx) = shared_tx_reconnect.get() else {
                return;
            };

            let market_sub = market_sub_rx.borrow().clone();
            let order_sub = order_sub_rx.borrow().clone();

            if market_sub.is_none() && order_sub.is_none() {
                // No subscriptions yet — re-authenticate only
                let _ = tx.send(WriterCommand::Send(auth_bytes_reconnect.clone()));
                return;
            }

            // Each subscription is sent as auth+sub in one write so that even if the
            // write fails mid-stream and the bytes enter the reconnect buffer, auth always
            // precedes the subscription when the buffer is drained on the next reconnect.
            if let Some(mut sub) = market_sub {
                sub.clk = market_clk_rx.borrow().clone();
                sub.initial_clk = market_initial_clk_rx.borrow().clone();
                if let Ok(sub_bytes) = serde_json::to_vec(&sub) {
                    let mut combined =
                        Vec::with_capacity(auth_bytes_reconnect.len() + 2 + sub_bytes.len());
                    combined.extend_from_slice(&auth_bytes_reconnect);
                    combined.extend_from_slice(b"\r\n");
                    combined.extend_from_slice(&sub_bytes);
                    let _ = tx.send(WriterCommand::Send(Bytes::from(combined)));
                }
            }

            if let Some(mut sub) = order_sub {
                sub.clk = order_clk_rx.borrow().clone();
                sub.initial_clk = order_initial_clk_rx.borrow().clone();
                if let Ok(sub_bytes) = serde_json::to_vec(&sub) {
                    let mut combined =
                        Vec::with_capacity(auth_bytes_reconnect.len() + 2 + sub_bytes.len());
                    combined.extend_from_slice(&auth_bytes_reconnect);
                    combined.extend_from_slice(b"\r\n");
                    combined.extend_from_slice(&sub_bytes);
                    let _ = tx.send(WriterCommand::Send(Bytes::from(combined)));
                }
            }
        });

        let url = format!("{}:{}", config.host, config.port);
        let socket_config = SocketConfig {
            url,
            mode,
            suffix: b"\r\n".to_vec(),
            message_handler: Some(message_handler),
            // SocketConfig.heartbeat interval is in seconds; round up to avoid zero
            heartbeat: Some((
                config.heartbeat_ms.div_ceil(1_000),
                b"{\"op\":\"heartbeat\"}".to_vec(),
            )),
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: Some(config.reconnect_delay_initial_ms),
            reconnect_delay_max_ms: Some(config.reconnect_delay_max_ms),
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            connection_max_retries: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: Some(config.idle_timeout_ms),
            certs_dir: None,
        };

        let socket = SocketClient::connect(socket_config, None, Some(post_reconnection), None)
            .await
            .map_err(|e| BetfairStreamError::ConnectionFailed(e.to_string()))?;

        // Set once — lock-free reads thereafter.
        let _ = shared_tx.set(socket.writer_tx.clone());

        socket
            .send_bytes(auth_bytes_vec)
            .await
            .map_err(|e| BetfairStreamError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            socket,
            market_sub_tx,
            market_clk_tx,
            market_initial_clk_tx,
            order_sub_tx,
            order_clk_tx,
            order_initial_clk_tx,
            market_active_sub_id,
            order_active_sub_id,
            request_id: AtomicU64::new(1),
            auth_bytes,
            closed: AtomicBool::new(false),
        })
    }

    /// Subscribes to market data for the given filter and data fields.
    ///
    /// Stores the subscription for automatic replay on reconnection.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or sending fails.
    pub async fn subscribe_markets(
        &self,
        market_filter: StreamMarketFilter,
        data_filter: MarketDataFilter,
        heartbeat_ms: Option<u64>,
        conflate_ms: Option<u64>,
    ) -> Result<(), BetfairStreamError> {
        if self.closed.load(Ordering::SeqCst) || self.socket.is_closed() {
            return Err(BetfairStreamError::Disconnected(
                "stream client is closed".to_string(),
            ));
        }
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        // Advance the active ID before clearing clocks so that any in-flight MCMs
        // from the previous subscription are immediately rejected by the handler.
        self.market_active_sub_id.store(id, Ordering::SeqCst);
        let sub = MarketSubscription {
            op: "marketSubscription".to_string(),
            id: Some(id),
            market_filter,
            market_data_filter: data_filter,
            clk: None,
            conflate_ms,
            heartbeat_ms,
            initial_clk: None,
            segmentation_enabled: None,
        };

        // Reset clocks so a disconnect before the first MCM response doesn't replay
        // stale tokens from a previous subscription with different filters.
        let _ = self.market_clk_tx.send(None);
        let _ = self.market_initial_clk_tx.send(None);
        let _ = self.market_sub_tx.send(Some(sub.clone()));

        // Auth and subscription are combined into one write so they cannot be split by a
        // write failure (which would buffer only the subscription in the reconnect buffer,
        // causing it to be replayed before post_reconnection sends auth). Sending directly
        // via writer_tx (not send_bytes) avoids the reconnect-wait, so a subscription made
        // while reconnecting is also buffered correctly with auth preceding it.
        let sub_bytes = serde_json::to_vec(&sub)?;
        let mut combined = Vec::with_capacity(self.auth_bytes.len() + 2 + sub_bytes.len());
        combined.extend_from_slice(&self.auth_bytes);
        combined.extend_from_slice(b"\r\n");
        combined.extend_from_slice(&sub_bytes);
        self.socket
            .writer_tx
            .send(WriterCommand::Send(Bytes::from(combined)))
            .map_err(|e| BetfairStreamError::ConnectionFailed(e.to_string()))?;
        Ok(())
    }

    /// Subscribes to order updates.
    ///
    /// Stores the subscription for automatic replay on reconnection.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or sending fails.
    pub async fn subscribe_orders(
        &self,
        order_filter: Option<OrderFilter>,
        heartbeat_ms: Option<u64>,
    ) -> Result<(), BetfairStreamError> {
        if self.closed.load(Ordering::SeqCst) || self.socket.is_closed() {
            return Err(BetfairStreamError::Disconnected(
                "stream client is closed".to_string(),
            ));
        }
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        self.order_active_sub_id.store(id, Ordering::SeqCst);
        let sub = OrderSubscription {
            op: "orderSubscription".to_string(),
            id: Some(id),
            order_filter,
            clk: None,
            conflate_ms: None,
            heartbeat_ms,
            initial_clk: None,
            segmentation_enabled: None,
        };

        // Reset clocks so a disconnect before the first OCM response doesn't replay
        // stale tokens from a previous subscription with different filters.
        let _ = self.order_clk_tx.send(None);
        let _ = self.order_initial_clk_tx.send(None);
        let _ = self.order_sub_tx.send(Some(sub.clone()));

        let sub_bytes = serde_json::to_vec(&sub)?;
        let mut combined = Vec::with_capacity(self.auth_bytes.len() + 2 + sub_bytes.len());
        combined.extend_from_slice(&self.auth_bytes);
        combined.extend_from_slice(b"\r\n");
        combined.extend_from_slice(&sub_bytes);
        self.socket
            .writer_tx
            .send(WriterCommand::Send(Bytes::from(combined)))
            .map_err(|e| BetfairStreamError::ConnectionFailed(e.to_string()))?;
        Ok(())
    }

    /// Returns `true` if the connection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.socket.is_active()
    }

    /// Closes the stream connection.
    pub async fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.socket.close().await;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::stream::messages::{Authentication, MarketDataFilter, StreamMarketFilter};

    #[rstest]
    fn test_invalid_clock_status_resets_clocks() {
        let (market_clk_tx, market_clk_rx) = watch::channel(Some("old-market-clk".to_string()));
        let (market_initial_clk_tx, market_initial_clk_rx) =
            watch::channel(Some("old-market-iclk".to_string()));
        let (order_clk_tx, order_clk_rx) = watch::channel(Some("old-order-clk".to_string()));
        let (order_initial_clk_tx, order_initial_clk_rx) =
            watch::channel(Some("old-order-iclk".to_string()));

        let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
            if let Ok(msg) = stream_decode(data)
                && let StreamMessage::Status(status) = &msg
                && status.error_code == Some(StatusErrorCode::InvalidClock)
            {
                let _ = market_clk_tx.send(None);
                let _ = market_initial_clk_tx.send(None);
                let _ = order_clk_tx.send(None);
                let _ = order_initial_clk_tx.send(None);
            }
        });

        handler(
            br#"{"op":"status","statusCode":"503","errorCode":"INVALID_CLOCK","connectionClosed":true}"#,
        );

        assert!(
            market_clk_rx.borrow().is_none(),
            "market clk must be cleared"
        );
        assert!(
            market_initial_clk_rx.borrow().is_none(),
            "market initialClk must be cleared"
        );
        assert!(order_clk_rx.borrow().is_none(), "order clk must be cleared");
        assert!(
            order_initial_clk_rx.borrow().is_none(),
            "order initialClk must be cleared"
        );
    }

    #[rstest]
    fn test_auth_message_serialization() {
        let auth = Authentication::new("my-app-key".to_string(), "my-session".to_string());
        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("\"op\":\"authentication\""));
        assert!(json.contains("\"appKey\":\"my-app-key\""));
        assert!(json.contains("\"session\":\"my-session\""));
    }

    #[rstest]
    fn test_clk_is_updated_from_mcm() {
        let (market_clk_tx, market_clk_rx) = watch::channel(None::<String>);
        let (market_initial_clk_tx, market_initial_clk_rx) = watch::channel(None::<String>);
        let (order_clk_tx, order_clk_rx) = watch::channel(None::<String>);
        let (order_initial_clk_tx, order_initial_clk_rx) = watch::channel(None::<String>);
        let market_active_sub_id = Arc::new(AtomicU64::new(5));
        let order_active_sub_id = Arc::new(AtomicU64::new(6));

        let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
            if let Ok(msg) = stream_decode(data) {
                match &msg {
                    StreamMessage::MarketChange(mcm) => {
                        let active = market_active_sub_id.load(Ordering::SeqCst);
                        if active > 0 && mcm.id.is_none_or(|id| id == active) {
                            if mcm.clk.is_some() {
                                let _ = market_clk_tx.send(mcm.clk.clone());
                            }

                            if mcm.initial_clk.is_some() {
                                let _ = market_initial_clk_tx.send(mcm.initial_clk.clone());
                            }
                        }
                    }
                    StreamMessage::OrderChange(ocm) => {
                        let active = order_active_sub_id.load(Ordering::SeqCst);
                        if active > 0 && ocm.id.is_none_or(|id| id == active) {
                            if ocm.clk.is_some() {
                                let _ = order_clk_tx.send(ocm.clk.clone());
                            }

                            if ocm.initial_clk.is_some() {
                                let _ = order_initial_clk_tx.send(ocm.initial_clk.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        // MCM/OCM with matching subscription id update clocks.
        handler(br#"{"op":"mcm","id":5,"pt":1000,"initialClk":"mcm-iclk","clk":"mcm-clk"}"#);
        handler(br#"{"op":"ocm","id":6,"pt":2000,"initialClk":"ocm-iclk","clk":"ocm-clk"}"#);

        assert_eq!(market_clk_rx.borrow().as_deref(), Some("mcm-clk"));
        assert_eq!(market_initial_clk_rx.borrow().as_deref(), Some("mcm-iclk"));
        assert_eq!(order_clk_rx.borrow().as_deref(), Some("ocm-clk"));
        assert_eq!(order_initial_clk_rx.borrow().as_deref(), Some("ocm-iclk"));

        // MCM without an id (e.g. heartbeat) is accepted for the active subscription.
        handler(br#"{"op":"mcm","pt":1001,"clk":"hb-clk"}"#);
        assert_eq!(market_clk_rx.borrow().as_deref(), Some("hb-clk"));

        // MCM from a stale subscription (explicit wrong id) must not overwrite stored clocks.
        handler(br#"{"op":"mcm","id":4,"pt":1002,"clk":"stale-clk"}"#);
        assert_eq!(market_clk_rx.borrow().as_deref(), Some("hb-clk"));
    }

    #[rstest]
    fn test_reconnect_callback_sends_auth_and_subscription() {
        let (market_clk_tx, market_clk_rx) = watch::channel(Some("mcm-clk1".to_string()));
        let (market_initial_clk_tx, market_initial_clk_rx) =
            watch::channel(Some("mcm-iclk1".to_string()));
        let (order_clk_tx, order_clk_rx) = watch::channel(Some("ocm-clk1".to_string()));
        let (order_initial_clk_tx, order_initial_clk_rx) =
            watch::channel(Some("ocm-iclk1".to_string()));
        let (market_sub_tx, market_sub_rx) = watch::channel(None::<MarketSubscription>);
        let (order_sub_tx, order_sub_rx) = watch::channel(None::<OrderSubscription>);
        let shared_tx: Arc<OnceLock<tokio::sync::mpsc::UnboundedSender<WriterCommand>>> =
            Arc::new(OnceLock::new());

        let auth = Authentication::new("key".to_string(), "token".to_string());
        let auth_bytes = Bytes::from(serde_json::to_vec(&auth).unwrap());

        let _ = market_sub_tx.send(Some(MarketSubscription {
            op: "marketSubscription".to_string(),
            id: Some(1),
            market_filter: StreamMarketFilter::default(),
            market_data_filter: MarketDataFilter::default(),
            clk: None,
            conflate_ms: None,
            heartbeat_ms: None,
            initial_clk: None,
            segmentation_enabled: None,
        }));
        let _ = order_sub_tx.send(Some(OrderSubscription {
            op: "orderSubscription".to_string(),
            id: Some(2),
            order_filter: None,
            clk: None,
            conflate_ms: None,
            heartbeat_ms: None,
            initial_clk: None,
            segmentation_enabled: None,
        }));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WriterCommand>();
        let _ = shared_tx.set(tx);

        // Build and invoke the reconnect closure (mirrors the logic in connect())
        let auth_bytes_reconnect = auth_bytes;
        let shared_tx_reconnect = Arc::clone(&shared_tx);
        let post_reconnection: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let Some(tx) = shared_tx_reconnect.get() else {
                return;
            };

            let market_sub = market_sub_rx.borrow().clone();
            let order_sub = order_sub_rx.borrow().clone();

            if market_sub.is_none() && order_sub.is_none() {
                let _ = tx.send(WriterCommand::Send(auth_bytes_reconnect.clone()));
                return;
            }

            if let Some(mut sub) = market_sub {
                sub.clk = market_clk_rx.borrow().clone();
                sub.initial_clk = market_initial_clk_rx.borrow().clone();
                if let Ok(sub_bytes) = serde_json::to_vec(&sub) {
                    let mut combined =
                        Vec::with_capacity(auth_bytes_reconnect.len() + 2 + sub_bytes.len());
                    combined.extend_from_slice(&auth_bytes_reconnect);
                    combined.extend_from_slice(b"\r\n");
                    combined.extend_from_slice(&sub_bytes);
                    let _ = tx.send(WriterCommand::Send(Bytes::from(combined)));
                }
            }

            if let Some(mut sub) = order_sub {
                sub.clk = order_clk_rx.borrow().clone();
                sub.initial_clk = order_initial_clk_rx.borrow().clone();
                if let Ok(sub_bytes) = serde_json::to_vec(&sub) {
                    let mut combined =
                        Vec::with_capacity(auth_bytes_reconnect.len() + 2 + sub_bytes.len());
                    combined.extend_from_slice(&auth_bytes_reconnect);
                    combined.extend_from_slice(b"\r\n");
                    combined.extend_from_slice(&sub_bytes);
                    let _ = tx.send(WriterCommand::Send(Bytes::from(combined)));
                }
            }
        });

        drop(market_clk_tx);
        drop(market_initial_clk_tx);
        drop(order_clk_tx);
        drop(order_initial_clk_tx);

        post_reconnection();

        // Each subscription is now a single combined write: auth\r\nsub (2 messages total)
        let market_cmd = rx.try_recv().expect("auth+market subscription message");
        let order_cmd = rx.try_recv().expect("auth+order subscription message");
        assert!(rx.try_recv().is_err(), "no further messages expected");

        let WriterCommand::Send(market_bytes) = market_cmd else {
            panic!("expected Send");
        };
        let WriterCommand::Send(order_bytes) = order_cmd else {
            panic!("expected Send");
        };

        let market_str = std::str::from_utf8(&market_bytes).unwrap();
        let order_str = std::str::from_utf8(&order_bytes).unwrap();

        let (market_auth, market_sub) = market_str
            .split_once("\r\n")
            .expect("CRLF separator in market combined message");
        let (order_auth, order_sub) = order_str
            .split_once("\r\n")
            .expect("CRLF separator in order combined message");

        assert!(market_auth.contains("\"op\":\"authentication\""));
        assert!(market_sub.contains("\"op\":\"marketSubscription\""));
        // Both clk and initialClk must be injected into each resubscription
        assert!(market_sub.contains("\"clk\":\"mcm-clk1\""));
        assert!(market_sub.contains("\"initialClk\":\"mcm-iclk1\""));

        assert!(order_auth.contains("\"op\":\"authentication\""));
        assert!(order_sub.contains("\"op\":\"orderSubscription\""));
        assert!(order_sub.contains("\"clk\":\"ocm-clk1\""));
        assert!(order_sub.contains("\"initialClk\":\"ocm-iclk1\""));
    }
}
