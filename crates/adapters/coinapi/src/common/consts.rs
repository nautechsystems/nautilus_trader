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

//! Client identity constants for the CoinAPI adapter.

use std::sync::LazyLock;

use nautilus_model::identifiers::ClientId;
use ustr::Ustr;

/// Provider identifier string, used as the data client and factory name.
pub const COINAPI: &str = "COINAPI";

/// Static client ID instance.
pub static COINAPI_CLIENT_ID: LazyLock<ClientId> =
    LazyLock::new(|| ClientId::new(Ustr::from(COINAPI)));

/// Referral code sent as `X-Referrer` so API Bricks can attribute usage to NautilusTrader.
///
/// <https://www.apibricks.io/platform/docs/general/referral>
pub const COINAPI_NAUTILUS_REFERRER: &str = "nautilus";
