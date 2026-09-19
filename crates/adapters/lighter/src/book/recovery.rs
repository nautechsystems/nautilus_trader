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

//! Connection-scoped book writes and bounded recovery futures.

use std::{future::Future, pin::Pin, sync::Arc};

use nautilus_common::live::dst::time::{self, Duration};
use nautilus_live::book::{
    recovery::BookRecovery,
    snapshot::{SnapshotGate, snapshot_expired},
};
use nautilus_network::websocket::{SubscriptionState, WebSocketClient};
use tokio_util::sync::CancellationToken;

use crate::{
    common::rate_limit::LIGHTER_WS_MESSAGE_RATE_LIMIT_KEY,
    websocket::{
        error::LighterWsError,
        handler::HandlerCommand,
        messages::{LighterWsChannel, LighterWsRequest},
    },
};

pub(crate) fn recover(
    market_index: i64,
    recovery: Arc<BookRecovery<LighterWsError>>,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    snapshot_timeout: Duration,
) -> BookWork {
    Box::pin(async move {
        let result = recovery
            .run(
                snapshot_timeout,
                |cancel, gate| {
                    let cmd_tx = cmd_tx.clone();

                    async move {
                        let (completion, rx) = tokio::sync::oneshot::channel();
                        cmd_tx
                            .send(HandlerCommand::RecoverBook {
                                market_index,
                                cancel,
                                gate,
                                completion,
                            })
                            .map_err(|e| LighterWsError::Client(e.to_string()))?;

                        rx.await
                            .map_err(|e| LighterWsError::Network(e.to_string()))?
                    }
                },
                |e| matches!(e, LighterWsError::Network(_) | LighterWsError::Transport(_)),
                LighterWsError::Client,
                || LighterWsError::Network("book snapshot deadline expired".into()),
            )
            .await;

        BookWorkResult::Recovery {
            market_index,
            recovery,
            result,
        }
    })
}

pub(crate) fn subscribe(
    market_index: i64,
    generation: u64,
    cancel: CancellationToken,
    write: Option<BookWrite>,
    client: Option<Arc<WebSocketClient>>,
    subscriptions: SubscriptionState,
    snapshot_timeout: Duration,
) -> BookWork {
    Box::pin(async move {
        let send = async {
            let topic = LighterWsChannel::OrderBook(market_index).topic_key();
            if subscriptions.get_reference_count(&topic) == 0 {
                return Err(LighterWsError::Client("book subscription cancelled".into()));
            }

            let client = client
                .ok_or_else(|| LighterWsError::Network("no active WebSocket client".into()))?;
            let epoch = client.connection_epoch();
            let channel = LighterWsChannel::OrderBook(market_index).subscription_channel();

            if write.is_some() {
                let payload =
                    serde_json::to_string(&LighterWsRequest::unsubscribe(channel.clone()))
                        .map_err(|e| LighterWsError::Client(e.to_string()))?;
                client
                    .send_text_on_connection(
                        payload,
                        Some(LIGHTER_WS_MESSAGE_RATE_LIMIT_KEY.as_slice()),
                        epoch,
                    )
                    .await?;
            }

            if subscriptions.get_reference_count(&topic) == 0 {
                return Err(LighterWsError::Client("book subscription cancelled".into()));
            }

            let payload = serde_json::to_string(&LighterWsRequest::subscribe(channel))
                .map_err(|e| LighterWsError::Client(e.to_string()))?;
            client
                .send_text_on_connection(
                    payload,
                    Some(LIGHTER_WS_MESSAGE_RATE_LIMIT_KEY.as_slice()),
                    epoch,
                )
                .await?;
            Ok::<_, LighterWsError>(epoch)
        };

        let send = send_with_deadline(send, snapshot_timeout);
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => Err(LighterWsError::Client("book send cancelled".into())),
            result = send => result,
        };

        BookWorkResult::Sent {
            market_index,
            generation,
            cancel,
            write,
            result,
        }
    })
}

async fn send_with_deadline(
    send: impl Future<Output = Result<u64, LighterWsError>>,
    snapshot_timeout: Duration,
) -> Result<u64, LighterWsError> {
    if snapshot_timeout.is_zero() {
        send.await
    } else {
        time::timeout(snapshot_timeout, send)
            .await
            .unwrap_or_else(|_| {
                Err(LighterWsError::Network(
                    "book write deadline expired".into(),
                ))
            })
    }
}

pub(crate) fn wait_for_snapshot(
    market_index: i64,
    cancel: CancellationToken,
    snapshot_timeout: Duration,
) -> BookWork {
    Box::pin(async move {
        if snapshot_timeout.is_zero() {
            // No deadline: resolve only when cancelled, which the handler ignores.
            cancel.cancelled().await;
        } else {
            snapshot_expired(&cancel, snapshot_timeout).await;
        }

        BookWorkResult::Initial {
            market_index,
            cancel,
        }
    })
}

pub(crate) type BookWork = Pin<Box<dyn Future<Output = BookWorkResult> + Send + Sync + 'static>>;

pub(crate) enum BookWorkResult {
    Sent {
        market_index: i64,
        generation: u64,
        cancel: CancellationToken,
        write: Option<BookWrite>,
        result: Result<u64, LighterWsError>,
    },
    Initial {
        market_index: i64,
        cancel: CancellationToken,
    },
    Recovery {
        market_index: i64,
        recovery: Arc<BookRecovery<LighterWsError>>,
        result: Result<(), LighterWsError>,
    },
}

pub(crate) struct BookWrite {
    pub(crate) cancel: CancellationToken,
    pub(crate) gate: SnapshotGate,
    pub(crate) completion: tokio::sync::oneshot::Sender<Result<(), LighterWsError>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wait_for_snapshot_zero_disables_deadline() {
        let cancel = CancellationToken::new();
        let mut work = wait_for_snapshot(0, cancel.clone(), Duration::ZERO);

        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut work)
                .await
                .is_err()
        );

        cancel.cancel();
        let result = work.await;

        let BookWorkResult::Initial {
            market_index,
            cancel: completed,
        } = result
        else {
            panic!("expected deadline completion");
        };

        assert_eq!(market_index, 0);
        assert!(completed.is_cancelled());
    }

    #[tokio::test]
    async fn wait_for_snapshot_short_timeout_expires_without_cancellation() {
        let result =
            wait_for_snapshot(0, CancellationToken::new(), Duration::from_millis(20)).await;

        let BookWorkResult::Initial {
            market_index,
            cancel,
        } = result
        else {
            panic!("expected deadline completion");
        };

        assert_eq!(market_index, 0);
        assert!(!cancel.is_cancelled());
    }

    #[tokio::test]
    async fn send_with_deadline_zero_waits_for_slow_send() {
        let send = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, LighterWsError>(7)
        };

        assert_eq!(send_with_deadline(send, Duration::ZERO).await.unwrap(), 7);
    }

    #[tokio::test]
    async fn send_with_deadline_short_timeout_expires_pending_send() {
        let send = std::future::pending::<Result<u64, LighterWsError>>();
        let result = send_with_deadline(send, Duration::from_millis(20)).await;

        let Err(LighterWsError::Network(message)) = result else {
            panic!("expected deadline expiry");
        };

        assert_eq!(message, "book write deadline expired");
    }
}
