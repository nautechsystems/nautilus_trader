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

//! Recovery episodes for Deribit order books over the shared runner.
//!
//! Each replacement sends [`HandlerCommand::RecoverBook`] back to the feed handler, which
//! unsubscribes and resubscribes the book's channels, then completes once the venue confirms
//! the resubscribe. The feed handler polls the returned work alongside commands and frames.

use std::{future::Future, pin::Pin, sync::Arc};

use nautilus_common::live::dst::time::{self, Duration};
use nautilus_live::book::recovery::BookRecovery as Recovery;

use super::sync::BookKey;
use crate::websocket::{
    error::{DeribitWsError, should_retry_deribit_ws_error},
    handler::HandlerCommand,
};

pub(crate) type BookRecovery = Recovery<DeribitWsError>;

/// Classifies book recovery errors, retrying corrupt frames fast: one bad frame replaces
/// again in about a second while persistent corruption still exhausts the budget into the
/// ceiling.
fn should_retry_book_recovery_error(error: &DeribitWsError) -> bool {
    matches!(error, DeribitWsError::ParsingError(_)) || should_retry_deribit_ws_error(error)
}

/// Runs a claimed recovery episode until a snapshot is accepted or the episode is cancelled.
///
/// The snapshot timeout bounds each replacement wait when enabled; a zero timeout waits until
/// the episode is cancelled.
pub(crate) fn recover(
    key: BookKey,
    recovery: Arc<BookRecovery>,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    snapshot_timeout: Duration,
) -> BookWork {
    Box::pin(async move {
        recovery
            .run(
                key.clone(),
                snapshot_timeout,
                |cancel, gate| {
                    let cmd_tx = cmd_tx.clone();
                    let recovery = Arc::clone(&recovery);
                    let (instrument, channel) = key.command_parts();
                    async move {
                        let (completion, rx) = tokio::sync::oneshot::channel();
                        cmd_tx
                            .send(HandlerCommand::RecoverBook {
                                instrument,
                                channel,
                                cancel,
                                gate,
                                completion,
                                recovery,
                            })
                            .map_err(|e| DeribitWsError::Send(e.to_string()))?;

                        await_completion(rx, snapshot_timeout).await
                    }
                },
                should_retry_book_recovery_error,
                DeribitWsError::Transport,
                || DeribitWsError::Timeout("book snapshot".to_owned()),
            )
            .await;

        BookWorkResult::Recovery
    })
}

async fn await_completion(
    rx: tokio::sync::oneshot::Receiver<Result<(), DeribitWsError>>,
    timeout: Duration,
) -> Result<(), DeribitWsError> {
    if timeout.is_zero() {
        return rx
            .await
            .map_err(|_| DeribitWsError::ClientError("book replacement abandoned".to_owned()))?;
    }

    time::timeout(timeout, rx)
        .await
        .map_err(|_| DeribitWsError::Timeout("book replacement".to_owned()))?
        .map_err(|_| DeribitWsError::ClientError("book replacement abandoned".to_owned()))?
}

pub(crate) type BookWork = Pin<Box<dyn Future<Output = BookWorkResult> + Send + 'static>>;

/// Completion of one handler-owned book future.
pub(crate) enum BookWorkResult {
    /// A recovery episode ended after a snapshot was accepted or the episode was cancelled.
    Recovery,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::not_connected(DeribitWsError::NotConnected, true)]
    #[case::transport(DeribitWsError::Transport("reset".into()), true)]
    #[case::send(DeribitWsError::Send("closed".into()), true)]
    #[case::timeout(DeribitWsError::Timeout("book snapshot".into()), true)]
    #[case::retriable_venue(
        DeribitWsError::DeribitError { code: 10028, message: "too many requests".into() },
        true
    )]
    #[case::rejected_venue(
        DeribitWsError::DeribitError { code: 13004, message: "invalid params".into() },
        false
    )]
    #[case::subscribe(DeribitWsError::Subscribe("denied".into()), false)]
    #[case::parsing(DeribitWsError::ParsingError("bad level".into()), false)]
    #[case::json(DeribitWsError::Json("bad frame".into()), false)]
    #[case::authentication(DeribitWsError::Authentication("expired".into()), false)]
    #[case::client(DeribitWsError::ClientError("abandoned".into()), false)]
    fn recovery_retry_classification(#[case] error: DeribitWsError, #[case] expected: bool) {
        assert_eq!(should_retry_deribit_ws_error(&error), expected);
    }

    #[rstest]
    #[case::parsing(DeribitWsError::ParsingError("bad level".into()), true)]
    #[case::timeout(DeribitWsError::Timeout("book snapshot".into()), true)]
    #[case::json(DeribitWsError::Json("bad frame".into()), false)]
    fn book_recovery_retry_classification(#[case] error: DeribitWsError, #[case] expected: bool) {
        assert_eq!(should_retry_book_recovery_error(&error), expected);
    }
}
