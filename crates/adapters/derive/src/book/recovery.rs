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

use std::sync::Arc;

use nautilus_common::live::dst::time::Duration;
use nautilus_live::{book::recovery::BookRecovery as Recovery, task::TaskSpawner};
use nautilus_model::identifiers::InstrumentId;

use crate::{
    common::retry::should_retry_ws_error,
    websocket::{DeriveWebSocketSubscriptionHandle, DeriveWsError},
};

pub(crate) type BookRecovery = Recovery<Arc<DeriveWsError>>;

pub(crate) fn spawn_recovery(
    channel: String,
    instrument_id: InstrumentId,
    recovery: Arc<BookRecovery>,
    ws: DeriveWebSocketSubscriptionHandle,
    transition: Arc<tokio::sync::Mutex<()>>,
    timeout: Duration,
    tasks: &TaskSpawner,
) {
    let shutdown = tasks.cancellation_token();
    let guard = recovery.cancellation.clone().drop_guard();

    if let Err(e) = tasks.spawn(async move {
        let _guard = guard;
        tokio::select! {
            biased;
            () = shutdown.cancelled() => recovery.cancellation.cancel(),
            () = recovery.run(
                instrument_id,
                timeout,
                |cancel, gate| {
                    let ws = ws.clone();
                    let transition = Arc::clone(&transition);
                    let channel = channel.clone();
                    let recovery = Arc::clone(&recovery);
                    async move {
                        let _turn = tokio::select! {
                            biased;
                            () = cancel.cancelled() => return Err(Arc::new(DeriveWsError::NotConnected)),
                            turn = transition.lock() => turn,
                        };
                        ws.replace_book_gated(channel, cancel, &recovery, gate)
                            .await
                            .map_err(Arc::new)
                    }
                },
                |e| is_retryable_error(e),
                |reason| Arc::new(DeriveWsError::Transport(reason)),
                || Arc::new(DeriveWsError::Timeout { method: "book snapshot".to_owned() }),
            ) => {}
        }
    }) {
        log::debug!("Skipping Derive book recovery after shutdown began: {e}");
    }
}

fn is_retryable_error(error: &DeriveWsError) -> bool {
    // Reconnects can retire a replacement while its channel still needs recovery
    matches!(error, DeriveWsError::NotConnected) || should_retry_ws_error(error)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::not_connected(DeriveWsError::NotConnected, true)]
    #[case::transport(DeriveWsError::Transport("reset".into()), true)]
    #[case::request_cancelled(DeriveWsError::RequestCancelled { method: "subscribe".into() }, true)]
    #[case::timeout(DeriveWsError::Timeout { method: "subscribe".into() }, true)]
    #[case::rate_limited(DeriveWsError::RateLimited { retry_after: Duration::from_secs(2) }, true)]
    #[case::internal_error(DeriveWsError::JsonRpc { code: -32603, message: "internal".into(), data: None }, true)]
    #[case::server_error(DeriveWsError::JsonRpc { code: -32099, message: "server".into(), data: None }, true)]
    #[case::backend_unavailable(DeriveWsError::JsonRpc { code: 9002, message: "unavailable".into(), data: None }, true)]
    #[case::invalid_params(DeriveWsError::JsonRpc { code: -32602, message: "invalid".into(), data: None }, false)]
    fn recovery_retry_classification(#[case] error: DeriveWsError, #[case] expected: bool) {
        assert_eq!(is_retryable_error(&error), expected);
    }
}
