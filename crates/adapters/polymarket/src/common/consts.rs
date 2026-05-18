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

//! Constants for the Polymarket adapter.

use std::sync::LazyLock;

use nautilus_model::identifiers::{ClientId, Venue};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

/// Venue identifier string.
pub const POLYMARKET: &str = "POLYMARKET";

/// Static venue instance.
pub static POLYMARKET_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(POLYMARKET)));

/// Static client ID instance.
pub static POLYMARKET_CLIENT_ID: LazyLock<ClientId> =
    LazyLock::new(|| ClientId::new(Ustr::from(POLYMARKET)));

/// Polymarket builder code for order attribution.
pub const POLYMARKET_NAUTILUS_BUILDER_CODE: &str =
    "0x4f2c0bba608033563f74b82300e2ed59f54f8d0de08281031f03fb2c62819e63";

pub const PUSD: &str = "pUSD";

pub const MAX_PRICE: &str = "0.999";
pub const MIN_PRICE: &str = "0.001";
pub const USDC_DECIMALS: u32 = 6;
pub const LOT_SIZE_SCALE: u32 = 2;

/// Minimum position size (in shares) reported in position status reports.
/// Smaller positions are filtered as dust during reconciliation.
pub const DUST_POSITION_THRESHOLD: f64 = 0.01;

/// Dust band (in shares) for fill quantity normalization. Set to one
/// cent-share, matching Polymarket's CLOB tick quantization.
///
/// Live-fill snapping is overfill-only: when the venue fill exceeds
/// `submitted_qty` by less than `DUST_SNAP_THRESHOLD`, the fill is snapped
/// DOWN to `submitted_qty`. Underfill is preserved on the per-fill path and
/// resolved at terminal `MATCHED` status by the synthetic dust fill
/// mechanism. `OrderStatusReport.filled_qty` snapping at terminal `Filled`
/// status uses this same threshold in both directions.
///
/// Two observed drift sources sit within this band:
///
/// - CLOB cent-tick truncation (underfill, up to `0.01` shares).
/// - V2 market-BUY USDC-scale truncation in `adjust_market_buy_amount`
///   (overfill, microshares; largest reproduced production overage is
///   `0.000066` shares).
///
/// A diff at or above this threshold is left unsnapped and surfaces to the
/// engine. See `docs/integrations/polymarket.md` (Fill quantity normalization).
pub const DUST_SNAP_THRESHOLD: f64 = 0.01;

/// Decimal form of [`DUST_SNAP_THRESHOLD`] for `Decimal` arithmetic paths.
pub const DUST_SNAP_THRESHOLD_DEC: Decimal = dec!(0.01);

pub const WS_MAX_SUBSCRIPTIONS: usize = 200;
pub const WS_DEFAULT_SUBSCRIPTIONS: usize = 200;

/// Maximum orders per `POST /orders` batch request; Polymarket caps batch submits at 15.
pub const BATCH_ORDER_LIMIT: usize = 15;

/// Requests per minute.
pub const HTTP_RATE_LIMIT: u32 = 100;

/// Maximum number of `condition_ids` accepted in a single Gamma `/markets` query.
pub const GAMMA_CONDITION_IDS_BATCH_SIZE: usize = 100;

pub const INVALID_API_KEY: &str = "Unauthorized/Invalid api key";
pub const CANCEL_ALREADY_DONE: &str = "already canceled or matched";
