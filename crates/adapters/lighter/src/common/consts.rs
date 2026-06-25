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

//! Venue identifiers and tuning constants for the Lighter adapter.

use std::{sync::LazyLock, time::Duration};

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// Venue name string for Lighter.
pub const LIGHTER: &str = "LIGHTER";

/// Lighter venue identifier.
pub static LIGHTER_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(LIGHTER)));

/// L2 chain id for Lighter mainnet.
///
/// Mirrors the upstream `lighter-go` constant. Used as the first element of
/// the L2 transaction hash preimage.
pub const LIGHTER_MAINNET_CHAIN_ID: u32 = 304;

/// L2 chain id for Lighter testnet.
///
/// Mirrors `lighter-go`'s testnet chain id and matches the value the oracle
/// generator emits.
pub const LIGHTER_TESTNET_CHAIN_ID: u32 = 300;

/// Nautilus integrator account index on Lighter.
pub const LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX: u64 = 723_813;

/// Venue error code for missing integrator approval.
pub const LIGHTER_ERROR_CODE_INTEGRATOR_NOT_APPROVED: u64 = 21_149;

/// Venue error code for an invalid (non-contiguous) transaction nonce.
pub const LIGHTER_ERROR_CODE_INVALID_NONCE: i64 = 21_104;

/// Venue error-code range for L2 transaction failures.
///
/// Observed codes follow a domain split: `20xxx` request validation, `21xxx`
/// transaction processing (21104 invalid nonce, 21149 integrator not
/// approved), `30xxx` WebSocket subscription state (30003 "Already
/// Subscribed"). Bare error frames are attributed to in-flight `sendTx`
/// requests only when the code falls in this range.
pub const LIGHTER_ERROR_CODE_TX_RANGE: std::ops::Range<u64> = 21_000..22_000;

/// Public docs anchor for integrator approval.
pub const LIGHTER_INTEGRATOR_APPROVAL_DOCS_URL: &str =
    "https://nautilustrader.io/docs/nightly/integrations/lighter.html#integrator-attribution";

/// Maximum batch size for `sendTxBatch` on the WebSocket transport.
pub const LIGHTER_MAX_BATCH_TX: usize = 15;

/// Maximum auth-token expiry permitted by the venue (8 hours).
pub const LIGHTER_AUTH_TOKEN_MAX_TTL: Duration = Duration::from_secs(8 * 60 * 60);

/// Default refresh window before an auth token expires.
///
/// The adapter rotates the auth token this far ahead of expiry to avoid races
/// during long-running WebSocket sessions.
pub const LIGHTER_AUTH_TOKEN_REFRESH_LEAD: Duration = Duration::from_secs(15 * 60);

/// Default WebSocket heartbeat interval.
///
/// Lighter requires a frame at least every 2 minutes; we send well below that.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Base reconnect backoff for the WebSocket client.
pub const RECONNECT_BASE_BACKOFF: Duration = Duration::from_millis(250);

/// Maximum reconnect backoff for the WebSocket client.
pub const RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Default HTTP request timeout.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum subscribe messages awaiting venue acknowledgement at once.
///
/// Held below Lighter's 50-per-IP inflight cap; see the WebSocket rate-limit
/// strategy in [`crate::common::rate_limit`].
pub const SUBSCRIBE_INFLIGHT_MAX: usize = 35;

/// Outbound command queue depth before backpressure kicks in.
pub const QUEUE_MAX: usize = 1000;
