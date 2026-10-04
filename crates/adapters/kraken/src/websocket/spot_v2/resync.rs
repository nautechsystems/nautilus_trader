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

//! Retry policy for Kraken Spot checksum-driven book resyncs.

use nautilus_model::identifiers::InstrumentId;
use ustr::Ustr;

use crate::websocket::{error::KrakenWsError, spot_v2::client::KrakenSpotWebSocketClient};

/// Maximum number of attempts when retrying a resync after a transient failure.
pub(crate) const RESYNC_MAX_ATTEMPTS: u32 = 5;
/// Initial backoff (milliseconds) between resync attempts; doubles each retry up to the cap.
pub(crate) const RESYNC_INITIAL_BACKOFF_MS: u64 = 500;
/// Upper bound for the exponential backoff between resync attempts.
pub(crate) const RESYNC_MAX_BACKOFF_MS: u64 = 8_000;

/// Retries `resync_book_l3` with exponential backoff so a transient REST/auth or send failure does
/// not leave the local book stuck in `awaiting_snapshot`.
pub(crate) async fn retry_l3_resync(client: &KrakenSpotWebSocketClient, symbol: Ustr, depth: u32) {
    retry_resync("L3", symbol, || client.resync_book_l3(symbol, depth)).await;
}

/// Retries `resync_book` with the same policy, for the Spot `book` channel.
pub(crate) async fn retry_l2_resync(
    client: &KrakenSpotWebSocketClient,
    instrument_id: InstrumentId,
    depth: Option<u32>,
) {
    retry_resync("L2", instrument_id.symbol.inner(), || {
        client.resync_book(instrument_id, depth)
    })
    .await;
}

/// Runs `attempt` with exponential backoff until it succeeds or the attempts are exhausted.
///
/// On final failure logs an error and returns; the handler stream remains alive and a fresh
/// subscribe (after reconnect or manual re-subscribe) re-arms the book.
async fn retry_resync<F, Fut>(channel: &str, symbol: Ustr, mut attempt: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), KrakenWsError>>,
{
    let mut delay_ms = RESYNC_INITIAL_BACKOFF_MS;

    for attempt_number in 1..=RESYNC_MAX_ATTEMPTS {
        match attempt().await {
            Ok(()) => {
                if attempt_number > 1 {
                    log::debug!(
                        "{channel} resync succeeded on attempt {attempt_number}: symbol={symbol}"
                    );
                }
                return;
            }
            Err(e) => {
                if attempt_number < RESYNC_MAX_ATTEMPTS {
                    log::debug!(
                        "{channel} resync attempt {attempt_number}/{RESYNC_MAX_ATTEMPTS} failed: \
                         symbol={symbol}, err={e}; retrying in {delay_ms}ms"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms * 2).min(RESYNC_MAX_BACKOFF_MS);
                } else {
                    log::error!(
                        "{channel} resync exhausted {RESYNC_MAX_ATTEMPTS} attempts: \
                         symbol={symbol}, err={e}; book remains cleared until reconnect \
                         or manual re-subscribe"
                    );
                }
            }
        }
    }
}
