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

//! HTTP/REST client implementations for Kraken APIs.
//!
//! This module provides HTTP clients for interacting with Kraken's REST endpoints:
//!
//! - [`spot`]: Kraken Spot REST API
//! - [`futures`]: Kraken Futures REST API

use chrono::{DateTime, Utc};
use nautilus_model::data::Bar;

pub mod error;
pub mod futures;
pub mod models;
pub mod spot;

/// Applies a `limit` to OHLC `bars` returned oldest-first by Kraken.
///
/// Kraken's OHLC endpoints ignore any count parameter and always return their
/// full page in ascending (oldest-first) order. When the caller anchors the
/// window with `start`, the oldest `limit` bars from that anchor are the
/// intended result, so the head of the page is kept. Otherwise a count-only
/// request (`start` is `None`) means "the most recent `limit` bars", so the
/// tail is kept rather than the head (see issue #4254).
pub(crate) fn apply_bar_limit(
    bars: &mut Vec<Bar>,
    start: Option<DateTime<Utc>>,
    limit: Option<u64>,
) {
    if let Some(limit) = limit {
        let limit = limit as usize;
        if bars.len() > limit {
            if start.is_some() {
                bars.truncate(limit);
            } else {
                bars.drain(..bars.len() - limit);
            }
        }
    }
}

// Re-exports
pub use error::KrakenHttpError;
pub use futures::{
    client::{
        KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND, KrakenFuturesHttpClient,
        KrakenFuturesRawHttpClient,
    },
    query::*,
};
pub use spot::{
    client::{
        KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND, KrakenSpotHttpClient, KrakenSpotRawHttpClient,
    },
    query::*,
};

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{Bar, BarType},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    /// Builds `n` bars in ascending (oldest-first) order, as Kraken returns them,
    /// with `ts_event` set to the 1-based index so ordering is easy to assert.
    fn ascending_bars(n: u64) -> Vec<Bar> {
        let bar_type = BarType::from("ETH/USD.KRAKEN-1-MINUTE-LAST-EXTERNAL");
        let price = Price::from("100.0");
        let volume = Quantity::from("1");
        (1..=n)
            .map(|i| {
                Bar::new(
                    bar_type,
                    price,
                    price,
                    price,
                    price,
                    volume,
                    UnixNanos::from(i),
                    UnixNanos::default(),
                )
            })
            .collect()
    }

    #[rstest]
    fn test_apply_bar_limit_no_limit_keeps_all() {
        let mut bars = ascending_bars(5);
        apply_bar_limit(&mut bars, None, None);
        assert_eq!(bars.len(), 5);
    }

    #[rstest]
    fn test_apply_bar_limit_count_only_keeps_most_recent() {
        let mut bars = ascending_bars(10);
        apply_bar_limit(&mut bars, None, Some(3));
        let ts: Vec<u64> = bars.iter().map(|b| b.ts_event.as_u64()).collect();
        assert_eq!(ts, vec![8, 9, 10]);
    }

    #[rstest]
    fn test_apply_bar_limit_with_start_keeps_oldest() {
        let mut bars = ascending_bars(10);
        let start = DateTime::<Utc>::from_timestamp(0, 0);
        apply_bar_limit(&mut bars, start, Some(3));
        let ts: Vec<u64> = bars.iter().map(|b| b.ts_event.as_u64()).collect();
        assert_eq!(ts, vec![1, 2, 3]);
    }

    #[rstest]
    fn test_apply_bar_limit_fewer_bars_than_limit() {
        let mut bars = ascending_bars(2);
        apply_bar_limit(&mut bars, None, Some(5));
        assert_eq!(bars.len(), 2);
    }
}
