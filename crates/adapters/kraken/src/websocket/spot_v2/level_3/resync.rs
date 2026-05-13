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

//! Retry helper for Kraken Spot `level3` checksum-driven resync.

use ustr::Ustr;

use crate::websocket::spot_v2::client::KrakenSpotWebSocketClient;

/// Maximum number of attempts when retrying a Kraken L3 resync after a
/// transient `refresh_auth_token` or `send_command` failure.
pub const L3_RESYNC_MAX_ATTEMPTS: u32 = 5;
/// Initial backoff (milliseconds) between L3 resync attempts; doubles each retry up to the cap.
pub const L3_RESYNC_INITIAL_BACKOFF_MS: u64 = 500;
/// Upper bound for the exponential backoff between L3 resync attempts.
pub const L3_RESYNC_MAX_BACKOFF_MS: u64 = 8_000;

/// Retries `resync_book_l3` with exponential backoff so a transient REST/auth
/// or send failure does not leave the local book stuck in `awaiting_snapshot`.
///
/// On final failure logs an error and returns; callers surface no panic because
/// the L3 handler stream remains alive and a fresh subscribe (after reconnect
/// or manual re-subscribe) re-arms the runtime.
pub async fn retry_l3_resync(client: &KrakenSpotWebSocketClient, symbol: Ustr, depth: u32) {
    let mut delay_ms = L3_RESYNC_INITIAL_BACKOFF_MS;

    for attempt in 1..=L3_RESYNC_MAX_ATTEMPTS {
        match client.resync_book_l3(symbol, depth).await {
            Ok(()) => {
                if attempt > 1 {
                    log::info!("L3 resync succeeded on attempt {attempt}: symbol={symbol}");
                }
                return;
            }
            Err(e) => {
                if attempt < L3_RESYNC_MAX_ATTEMPTS {
                    log::warn!(
                        "L3 resync attempt {attempt}/{L3_RESYNC_MAX_ATTEMPTS} failed: \
                         symbol={symbol}, err={e}; retrying in {delay_ms}ms"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms * 2).min(L3_RESYNC_MAX_BACKOFF_MS);
                } else {
                    log::error!(
                        "L3 resync exhausted {L3_RESYNC_MAX_ATTEMPTS} attempts: \
                         symbol={symbol}, err={e}; book remains cleared until reconnect \
                         or manual re-subscribe"
                    );
                }
            }
        }
    }
}
