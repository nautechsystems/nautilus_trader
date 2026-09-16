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

//! Constants for the Kalshi adapter.

use nautilus_model::identifiers::ClientId;

/// The Kalshi venue name.
pub const KALSHI: &str = "KALSHI";

/// The Kalshi venue identifier.
pub const KALSHI_VENUE: &str = KALSHI;

/// The default client identifier for the Kalshi data client.
pub const KALSHI_DATA_CLIENT_ID: &str = "KALSHI-DATA";

/// The default client identifier for the Kalshi execution client.
pub const KALSHI_EXEC_CLIENT_ID: &str = "KALSHI-EXEC";

/// The client identifier the Kalshi execution client registers under.
///
/// The data client registers under [`KALSHI_DATA_CLIENT_ID`].
pub const KALSHI_CLIENT_ID: &str = KALSHI_EXEC_CLIENT_ID;

/// The default account identifier for the Kalshi execution client.
pub const KALSHI_ACCOUNT_ID: &str = "KALSHI-001";

/// Decimal places on Kalshi prices. Prices are fixed-point dollars with up to four decimal places.
pub const KALSHI_PRICE_PRECISION: u8 = 4;

/// Decimal places on Kalshi contract quantities, which have a minimum granularity of 0.01.
pub const KALSHI_SIZE_PRECISION: u8 = 2;

/// The currency every Kalshi contract is denominated in.
pub const KALSHI_CURRENCY: &str = "USD";

/// Maximum page size the exchange accepts on paginated market-data endpoints.
pub const MAX_PAGE_LIMIT: u32 = 1_000;

/// Environment variable holding the Kalshi API key ID.
pub const KALSHI_API_KEY_ID_ENV: &str = "KALSHI_API_KEY_ID";

/// Environment variable holding the Kalshi API key private key PEM.
pub const KALSHI_API_KEY_PEM_ENV: &str = "KALSHI_API_KEY_PEM";

/// Returns the data client identifier for the venue.
#[must_use]
pub fn data_client_id() -> ClientId {
    ClientId::from(KALSHI_DATA_CLIENT_ID)
}

/// Returns the execution client identifier for the venue.
#[must_use]
pub fn exec_client_id() -> ClientId {
    ClientId::from(KALSHI_EXEC_CLIENT_ID)
}
