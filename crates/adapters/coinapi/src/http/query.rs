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

//! Typed CoinAPI query parameters.

use std::num::NonZeroUsize;

use jiff::{RoundMode, Timestamp, TimestampRound, Unit};
use serde::{Deserialize, Serialize, Serializer};

use super::error::Error;

/// Largest documented historical page size.
pub const HISTORY_LIMIT_MAX: u32 = 100_000;

/// An explicit historical time window and record limit.
///
/// This represents one page. CoinAPI does not specify the timestamp or boundary
/// inclusivity needed to guarantee lossless automatic continuation.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryQuery {
    #[serde(serialize_with = "serialize_time")]
    pub time_start: Timestamp,
    #[serde(serialize_with = "serialize_time")]
    pub time_end: Timestamp,
    pub limit: u32,
}

impl HistoryQuery {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if provider_time(self.time_start)? >= provider_time(self.time_end)? {
            return Err(Error::Validation(
                "time_start must precede time_end at CoinAPI's 100 ns resolution".into(),
            ));
        }

        if !(1..=HISTORY_LIMIT_MAX).contains(&self.limit) {
            return Err(Error::Validation(format!(
                "limit must be in [1, {HISTORY_LIMIT_MAX}]"
            )));
        }

        Ok(())
    }
}

// CoinAPI accepts only zero, three, or seven fractional digits, so bounds always carry seven
fn serialize_time<S: Serializer>(time: &Timestamp, serializer: S) -> Result<S::Ok, S::Error> {
    let time = provider_time(*time).map_err(serde::ser::Error::custom)?;
    serializer.collect_str(&format_args!("{time:.7}"))
}

// CoinAPI records sit on the 100 ns grid, so rounding both bounds up to it selects the same
// records as the exact [time_start, time_end) and loses nothing at seven digits.
pub(super) fn provider_time(time: Timestamp) -> Result<Timestamp, Error> {
    time.round(
        TimestampRound::new()
            .smallest(Unit::Nanosecond)
            .increment(100)
            .mode(RoundMode::Ceil),
    )
    .map_err(|e| Error::Validation(format!("timestamp out of range: {e}")))
}

/// A historical time window `[time_start, time_end)` fetched across as many pages as it needs.
///
/// Without a limit, the window and the client credit budget bound the records returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryRange {
    pub time_start: Timestamp,
    pub time_end: Timestamp,
    pub limit: Option<NonZeroUsize>,
}

/// Filters for exchange discovery.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ExchangesQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_exchange_id: Option<String>,
}

/// Filters for exchange-scoped active symbol discovery.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_symbol_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_asset_id: Option<String>,
}

/// Maximum depth requested for a single current order book; the provider default when absent.
#[derive(Debug, Clone, Serialize)]
pub struct BookQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_levels: Option<u32>,
}

/// Historical trades, optionally including the exchange trade identifier.
#[derive(Debug, Clone, Serialize)]
pub struct TradesQuery {
    #[serde(flatten)]
    pub range: HistoryQuery,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_id: Option<bool>,
}

/// Historical OHLCV for a provider period identifier.
#[derive(Debug, Clone, Serialize)]
pub struct OhlcvQuery {
    pub period_id: String,
    #[serde(flatten)]
    pub range: HistoryQuery,
}
