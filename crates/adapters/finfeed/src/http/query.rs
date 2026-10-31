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

//! Typed queries with endpoint-specific bounds and exact provider time serialization.

use jiff::{Timestamp, civil::Date};
use serde::{Serialize, Serializer};

use super::error::Error;

/// Maximum listing, active-ID, activity, and historical trade/quote/book page size.
pub const PAGE_LIMIT_MAX: u32 = 10_000;
/// Maximum market OHLCV page size.
pub const OHLCV_LIMIT_MAX: u32 = 100_000;

pub(crate) const HOUR_SECONDS: i64 = 3600;

/// Full listings use an opaque cursor, not a page number.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        name = "FinFeedListingsQuery",
        module = "nautilus_trader.adapters.finfeed",
        frozen,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")
)]
pub struct ListingsQuery {
    /// Maximum records per page, in `[1, 10000]`.
    pub limit: u32,
    /// Nonempty continuation cursor; `None` starts at the first page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl ListingsQuery {
    pub(super) fn validate(&self) -> Result<(), Error> {
        validate_limit(self.limit, PAGE_LIMIT_MAX)?;

        if self.cursor.as_ref().is_some_and(|cursor| cursor.is_empty()) {
            return Err(Error::Validation("cursor must not be empty".into()));
        }

        Ok(())
    }
}

/// Active IDs use a one-based page number.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        name = "FinFeedActiveQuery",
        module = "nautilus_trader.adapters.finfeed",
        frozen,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")
)]
pub struct ActiveQuery {
    /// Maximum IDs per page, in `[1, 10000]`.
    pub limit: u32,
    /// One-based page number, in `[1, 2147483647]`.
    pub page: u32,
}

impl ActiveQuery {
    pub(super) fn validate(&self) -> Result<(), Error> {
        validate_limit(self.limit, PAGE_LIMIT_MAX)?;

        if self.page == 0 || self.page > i32::MAX as u32 {
            return Err(Error::Validation(
                "page must be a positive signed 32-bit integer".into(),
            ));
        }

        Ok(())
    }
}

/// Per-list record limit for latest activity.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        name = "FinFeedActivityQuery",
        module = "nautilus_trader.adapters.finfeed",
        frozen,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")
)]
pub struct ActivityQuery {
    /// Maximum records in each trade and quote list, independently, in `[1, 10000]`.
    pub limit: u32,
}

impl ActivityQuery {
    pub(super) fn validate(&self) -> Result<(), Error> {
        validate_limit(self.limit, PAGE_LIMIT_MAX)
    }
}

/// One historical response, selected by a date or a start and optional end time.
///
/// Date and time bounds are mutually exclusive. No filtering, bound inclusion, or completeness
/// guarantee is inferred from the provider's response.
/// Trade, quote, and book history reject bounded selectors within one UTC hour. Use covering
/// UTC-hour bounds and filter the raw records locally. Date and start-only selectors are accepted.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        name = "FinFeedHistoryQuery",
        module = "nautilus_trader.adapters.finfeed",
        frozen,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")
)]
pub struct HistoryQuery {
    /// UTC date selector; mutually exclusive with time bounds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<Date>,
    /// Start timestamp; required when `date` is absent.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_time"
    )]
    pub time_start: Option<Timestamp>,
    /// Optional end timestamp, strictly after `time_start` when both are present.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_time"
    )]
    pub time_end: Option<Timestamp>,
    /// Maximum response records, in `[1, 10000]`.
    pub limit: u32,
}

impl HistoryQuery {
    pub(super) fn validate(&self) -> Result<(), Error> {
        validate_limit(self.limit, PAGE_LIMIT_MAX)?;

        if self.date.is_some() {
            if self.time_start.is_some() || self.time_end.is_some() {
                return Err(Error::Validation(
                    "date and time bounds are mutually exclusive".into(),
                ));
            }
        } else if self.time_start.is_none() {
            return Err(Error::Validation("date or time_start is required".into()));
        }

        validate_times(self.time_start, self.time_end)
    }

    pub(super) fn validate_hourly(&self) -> Result<(), Error> {
        self.validate()?;

        if self
            .time_start
            .zip(self.time_end)
            .is_some_and(|(start, end)| {
                start.as_second().div_euclid(HOUR_SECONDS)
                    == end.as_second().div_euclid(HOUR_SECONDS)
            })
        {
            return Err(Error::Validation(
                "history time bounds must cross a UTC hour boundary; FinFeed can return empty data for ranges within one hour; use date or covering UTC-hour bounds and filter locally".into(),
            ));
        }

        Ok(())
    }
}

/// Flat-file OHLCV selected by a date or a start and optional end time.
///
/// The provider describes these bars as trade-generated. `period` accepts `1SEC`, `1MIN`,
/// or `1HRS`. The history limit is in `[1, 10000]`.
/// This query selects the activity flat-file source, independently of DB-backed OHLCV.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        name = "FinFeedFlatFileOhlcvQuery",
        module = "nautilus_trader.adapters.finfeed",
        frozen,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")
)]
pub struct FlatFileOhlcvQuery {
    /// Flat-file aggregation period: `1SEC`, `1MIN`, or `1HRS`.
    pub period: String,
    /// Date or time selectors and response limit for the flat-file source.
    #[serde(flatten)]
    pub history: HistoryQuery,
}

impl FlatFileOhlcvQuery {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if !matches!(self.period.as_str(), "1SEC" | "1MIN" | "1HRS") {
            return Err(Error::Validation(
                "flat-file period must be 1SEC, 1MIN, or 1HRS".into(),
            ));
        }

        self.history.validate()
    }
}

/// OHLCV query; `get_ohlcv_latest` rejects time bounds locally.
///
/// Exchange-wide history requires both time bounds, at most 24 hours apart, and a limit
/// in `[1, 10000]`. Per-market operations accept limits in `[1, 100000]`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        name = "FinFeedOhlcvQuery",
        module = "nautilus_trader.adapters.finfeed",
        frozen,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")
)]
pub struct OhlcvQuery {
    /// Nonblank identifier from the provider period catalog.
    pub period_id: String,
    /// Optional start timestamp; required for exchange-wide history and rejected for latest bars.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_time"
    )]
    pub time_start: Option<Timestamp>,
    /// Optional end timestamp; required for exchange-wide history and rejected for latest bars.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_time"
    )]
    pub time_end: Option<Timestamp>,
    /// Maximum rows: `[1, 100000]` per market or `[1, 10000]` for exchange-wide history.
    pub limit: u32,
}

impl OhlcvQuery {
    pub(super) fn validate(&self, latest: bool) -> Result<(), Error> {
        validate_limit(self.limit, OHLCV_LIMIT_MAX)?;

        if self.period_id.trim().is_empty()
            || (latest && (self.time_start.is_some() || self.time_end.is_some()))
        {
            return Err(Error::Validation(
                "period_id is required and latest cannot carry time bounds".into(),
            ));
        }

        validate_times(self.time_start, self.time_end)
    }

    pub(super) fn validate_exchange(&self) -> Result<(), Error> {
        validate_limit(self.limit, PAGE_LIMIT_MAX)?;
        self.validate(false)?;

        let (Some(start), Some(end)) = (self.time_start, self.time_end) else {
            return Err(Error::Validation(
                "exchange OHLCV requires time_start and time_end".into(),
            ));
        };

        if end.as_nanosecond() - start.as_nanosecond() > 86_400_000_000_000 {
            return Err(Error::Validation(
                "exchange OHLCV window must not exceed 24 hours".into(),
            ));
        }

        Ok(())
    }
}

/// Bounds on a complete discovery traversal; exceeding either returns an error.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        name = "FinFeedPagination",
        module = "nautilus_trader.adapters.finfeed",
        frozen,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")
)]
pub struct Pagination {
    /// Maximum requests in the traversal, including the terminal page; must be positive.
    pub max_pages: u32,
    /// Maximum total records retained across pages; must be positive.
    pub max_records: usize,
}

impl Pagination {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.max_pages == 0 || self.max_records == 0 {
            return Err(Error::Validation(
                "pagination bounds must be positive".into(),
            ));
        }

        Ok(())
    }
}

fn validate_limit(limit: u32, maximum: u32) -> Result<(), Error> {
    if !(1..=maximum).contains(&limit) {
        return Err(Error::Validation(format!(
            "limit must be in [1, {maximum}]"
        )));
    }

    Ok(())
}

fn validate_times(start: Option<Timestamp>, end: Option<Timestamp>) -> Result<(), Error> {
    if start.zip(end).is_some_and(|(start, end)| start >= end) {
        return Err(Error::Validation("time_start must precede time_end".into()));
    }

    if start
        .into_iter()
        .chain(end)
        .any(|time| time.subsec_nanosecond() % 100 != 0)
    {
        return Err(Error::Validation(
            "timestamps must fit the provider's 100 ns precision".into(),
        ));
    }

    Ok(())
}

fn serialize_time<S: Serializer>(
    time: &Option<Timestamp>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match time {
        Some(time) => serializer.collect_str(&format_args!("{time:.7}")),
        None => serializer.serialize_none(),
    }
}
