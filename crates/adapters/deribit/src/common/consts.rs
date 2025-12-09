// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Core constants for the Deribit adapter.

use std::sync::LazyLock;

use ahash::AHashSet;
use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// Venue identifier string.
pub const DERIBIT: &str = "DERIBIT";

/// Static venue instance.
pub static DERIBIT_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(DERIBIT)));

// Production URLs
pub const DERIBIT_HTTP_URL: &str = "https://www.deribit.com";
pub const DERIBIT_WS_URL: &str = "wss://www.deribit.com/ws/api/v2";

// Testnet URLs
pub const DERIBIT_TESTNET_HTTP_URL: &str = "https://test.deribit.com";
pub const DERIBIT_TESTNET_WS_URL: &str = "wss://test.deribit.com/ws/api/v2";

// API paths
pub const DERIBIT_API_VERSION: &str = "v2";
pub const DERIBIT_API_PATH: &str = "/api/v2";

// JSON-RPC constants
pub const JSONRPC_VERSION: &str = "2.0";

/// Deribit error codes that should trigger retries.
///
/// Only retry on temporary network/system issues that are likely to resolve.
/// Based on Deribit API documentation error codes.
///
/// # Error Code Categories
///
/// **Retriable (temporary issues):**
/// - `10028`: "too_many_requests" - Rate limit exceeded
/// - `10040`: "retry" - Explicitly says request should be retried
/// - `10041`: "settlement_in_progress" - Settlement calculation in progress (few seconds)
/// - `10047`: "matching_engine_queue_full" - Matching engine queue full
/// - `10066`: "too_many_concurrent_requests" - Too many concurrent public requests
/// - `11051`: "system_maintenance" - System under maintenance
/// - `11094`: "internal_server_error" - Unhandled server error
/// - `13028`: "temporarily_unavailable" - Service not responding or too slow
/// - `13888`: "timed_out" - Server timeout processing request
///
/// **Non-retriable (permanent errors):**
/// - `10000`: "authorization_required" - Auth issue, invalid signature
/// - `10004`: "order_not_found" - Order can't be found
/// - `10009`: "not_enough_funds" - Insufficient funds
/// - `10020`: "invalid_or_unsupported_instrument" - Invalid instrument name
/// - `10029`: "not_owner_of_order" - Attempt to operate with not own order
/// - `11029`: "invalid_arguments" - Invalid input detected
/// - `11050`: "bad_request" - Request not parsed properly
/// - `13004`: "invalid_credentials" - Invalid API credentials
/// - `13009`: "unauthorized" - Wrong/expired token or bad signature
/// - `13020`: "not_found" - Instrument not found
/// - `13021`: "forbidden" - Not enough permissions
///
/// # References
///
/// <https://docs.deribit.com/#rpc-error-codes>
pub static DERIBIT_RETRY_ERROR_CODES: LazyLock<AHashSet<i64>> = LazyLock::new(|| {
    let mut codes = AHashSet::new();

    // Rate limiting (temporary - will resolve after backoff)
    codes.insert(10028); // too_many_requests
    codes.insert(10066); // too_many_concurrent_requests

    // Explicit retry instruction
    codes.insert(10040); // retry - API explicitly says to retry

    // System issues (temporary - maintenance, settlement, or overload)
    codes.insert(10041); // settlement_in_progress - daily settlement (few seconds)
    codes.insert(10047); // matching_engine_queue_full
    codes.insert(11051); // system_maintenance
    codes.insert(11094); // internal_server_error
    codes.insert(13028); // temporarily_unavailable

    // Timeout (temporary - may succeed on retry)
    codes.insert(13888); // timed_out

    codes
});

/// Determines if a Deribit error code should trigger a retry.
///
/// # Arguments
///
/// * `error_code` - The Deribit error code from the JSON-RPC error response
///
/// # Returns
///
/// `true` if the error is temporary and should be retried, `false` otherwise
pub fn should_retry_error_code(error_code: i64) -> bool {
    DERIBIT_RETRY_ERROR_CODES.contains(&error_code)
}
