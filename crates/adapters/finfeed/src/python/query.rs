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

//! Python constructors for the bounded REST queries.

use jiff::Timestamp;
use nautilus_core::python::to_pyvalue_err;
use pyo3::prelude::*;

use crate::http::query::{
    ActiveQuery, ActivityQuery, FlatFileOhlcvQuery, HistoryQuery, ListingsQuery, OhlcvQuery,
    Pagination,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ListingsQuery {
    /// Full listings use an opaque cursor, not a page number.
    #[new]
    #[pyo3(signature = (limit, cursor=None))]
    fn py_new(limit: u32, cursor: Option<String>) -> Self {
        Self { limit, cursor }
    }

    /// Maximum records per page, in `[1, 10000]`.
    #[getter(limit)]
    fn py_limit(&self) -> u32 {
        self.limit
    }

    /// Nonempty continuation cursor; `None` starts at the first page.
    #[getter(cursor)]
    fn py_cursor(&self) -> Option<String> {
        self.cursor.clone()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ActiveQuery {
    /// Active IDs use a one-based page number.
    #[new]
    fn py_new(limit: u32, page: u32) -> Self {
        Self { limit, page }
    }

    /// Maximum IDs per page, in `[1, 10000]`.
    #[getter(limit)]
    fn py_limit(&self) -> u32 {
        self.limit
    }

    /// One-based page number, in `[1, 2147483647]`.
    #[getter(page)]
    fn py_page(&self) -> u32 {
        self.page
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ActivityQuery {
    /// Per-list record limit for latest activity.
    #[new]
    fn py_new(limit: u32) -> Self {
        Self { limit }
    }

    /// Maximum records in each trade and quote list, independently, in `[1, 10000]`.
    #[getter(limit)]
    fn py_limit(&self) -> u32 {
        self.limit
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HistoryQuery {
    /// One historical response, selected by a date or a start and optional end time.
    ///
    /// Date and time bounds are mutually exclusive. No filtering, bound inclusion, or completeness
    /// guarantee is inferred from the provider's response.
    /// Trade, quote, and book history reject bounded selectors within one UTC hour. Use covering
    /// UTC-hour bounds and filter the raw records locally. Date and start-only selectors are accepted.
    #[new]
    #[pyo3(signature = (limit, date=None, time_start=None, time_end=None))]
    fn py_new(
        limit: u32,
        date: Option<&str>,
        time_start: Option<&str>,
        time_end: Option<&str>,
    ) -> PyResult<Self> {
        let date = date
            .map(str::parse)
            .transpose()
            .map_err(|_| to_pyvalue_err("invalid date"))?;
        let (time_start, time_end) = parse_times(time_start, time_end)?;
        Ok(Self {
            date,
            time_start,
            time_end,
            limit,
        })
    }

    /// Maximum response records, in `[1, 10000]`.
    #[getter(limit)]
    fn py_limit(&self) -> u32 {
        self.limit
    }

    /// UTC date selector; mutually exclusive with time bounds.
    #[getter(date)]
    fn py_date(&self) -> Option<String> {
        self.date.map(|date| date.to_string())
    }

    /// Start timestamp; required when `date` is absent.
    #[getter(time_start)]
    fn py_time_start(&self) -> Option<String> {
        self.time_start.map(|time| time.to_string())
    }

    /// Optional end timestamp, strictly after `time_start` when both are present.
    #[getter(time_end)]
    fn py_time_end(&self) -> Option<String> {
        self.time_end.map(|time| time.to_string())
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl FlatFileOhlcvQuery {
    /// Flat-file OHLCV selected by a date or a start and optional end time.
    ///
    /// The provider describes these bars as trade-generated. `period` accepts `1SEC`, `1MIN`,
    /// or `1HRS`. The history limit is in `[1, 10000]`.
    /// This query selects the activity flat-file source, independently of DB-backed OHLCV.
    #[new]
    #[pyo3(signature = (period, limit, date=None, time_start=None, time_end=None))]
    fn py_new(
        period: String,
        limit: u32,
        date: Option<&str>,
        time_start: Option<&str>,
        time_end: Option<&str>,
    ) -> PyResult<Self> {
        let history = HistoryQuery::py_new(limit, date, time_start, time_end)?;
        Ok(Self { period, history })
    }

    /// Flat-file aggregation period: `1SEC`, `1MIN`, or `1HRS`.
    #[getter(period)]
    fn py_period(&self) -> String {
        self.period.clone()
    }

    /// Maximum response records, in `[1, 10000]`.
    #[getter(limit)]
    fn py_limit(&self) -> u32 {
        self.history.limit
    }

    /// UTC date selector; mutually exclusive with time bounds.
    #[getter(date)]
    fn py_date(&self) -> Option<String> {
        self.history.py_date()
    }

    /// Start timestamp; required when `date` is absent.
    #[getter(time_start)]
    fn py_time_start(&self) -> Option<String> {
        self.history.py_time_start()
    }

    /// Optional end timestamp, strictly after `time_start` when both are present.
    #[getter(time_end)]
    fn py_time_end(&self) -> Option<String> {
        self.history.py_time_end()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OhlcvQuery {
    /// OHLCV query; `get_ohlcv_latest` rejects time bounds locally.
    ///
    /// Exchange-wide history requires both time bounds, at most 24 hours apart, and a limit
    /// in `[1, 10000]`. Per-market operations accept limits in `[1, 100000]`.
    #[new]
    #[pyo3(signature = (period_id, limit, time_start=None, time_end=None))]
    fn py_new(
        period_id: String,
        limit: u32,
        time_start: Option<&str>,
        time_end: Option<&str>,
    ) -> PyResult<Self> {
        let (time_start, time_end) = parse_times(time_start, time_end)?;
        Ok(Self {
            period_id,
            time_start,
            time_end,
            limit,
        })
    }

    /// Nonblank identifier from the provider period catalog.
    #[getter(period_id)]
    fn py_period_id(&self) -> String {
        self.period_id.clone()
    }

    /// Maximum rows: `[1, 100000]` per market or `[1, 10000]` for exchange-wide history.
    #[getter(limit)]
    fn py_limit(&self) -> u32 {
        self.limit
    }

    /// Optional start timestamp; required for exchange-wide history and rejected for latest bars.
    #[getter(time_start)]
    fn py_time_start(&self) -> Option<String> {
        self.time_start.map(|time| time.to_string())
    }

    /// Optional end timestamp; required for exchange-wide history and rejected for latest bars.
    #[getter(time_end)]
    fn py_time_end(&self) -> Option<String> {
        self.time_end.map(|time| time.to_string())
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Pagination {
    /// Bounds on a complete discovery traversal; exceeding either returns an error.
    #[new]
    fn py_new(max_pages: u32, max_records: usize) -> Self {
        Self {
            max_pages,
            max_records,
        }
    }

    /// Maximum requests in the traversal, including the terminal page; must be positive.
    #[getter(max_pages)]
    fn py_max_pages(&self) -> u32 {
        self.max_pages
    }

    /// Maximum total records retained across pages; must be positive.
    #[getter(max_records)]
    fn py_max_records(&self) -> usize {
        self.max_records
    }
}

pub(super) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ListingsQuery>()?;
    m.add_class::<ActiveQuery>()?;
    m.add_class::<ActivityQuery>()?;
    m.add_class::<HistoryQuery>()?;
    m.add_class::<FlatFileOhlcvQuery>()?;
    m.add_class::<OhlcvQuery>()?;
    m.add_class::<Pagination>()?;
    Ok(())
}

fn parse_times(
    start: Option<&str>,
    end: Option<&str>,
) -> PyResult<(Option<Timestamp>, Option<Timestamp>)> {
    let start = start
        .map(str::parse)
        .transpose()
        .map_err(|_| to_pyvalue_err("invalid time_start"))?;
    let end = end
        .map(str::parse)
        .transpose()
        .map_err(|_| to_pyvalue_err("invalid time_end"))?;
    Ok((start, end))
}
