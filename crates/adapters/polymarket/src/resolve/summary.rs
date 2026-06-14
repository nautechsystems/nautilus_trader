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

use std::sync::Arc;

use nautilus_core::UnixNanos;
use nautilus_model::data::{HasTsInit, custom::CustomDataTrait};
#[cfg(feature = "python")]
use pyo3::types::PyDictMethods;
use serde::{Deserialize, Serialize};

pub(crate) const RESOLVE_REQUEST_TYPE_NAME: &str = "PolymarketResolveRequest";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResolveRequestSummary {
    pub(crate) requested_condition_ids: Vec<String>,
    pub(crate) fetched_markets: usize,
    pub(crate) resolved_markets: usize,
    pub(crate) skipped_non_binary_markets: usize,
    pub(crate) clob_fallback_successes: usize,
    pub(crate) emitted_condition_ids: Vec<String>,
    pub(crate) failed_condition_ids: Vec<String>,
    pub(crate) used_watchlist_fallback: bool,
    pub(crate) timed_out_watchlist: usize,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PolymarketResolveRequestSummaryData {
    pub(crate) requested_condition_ids: Vec<String>,
    pub(crate) fetched_markets: usize,
    pub(crate) resolved_markets: usize,
    pub(crate) skipped_non_binary_markets: usize,
    pub(crate) clob_fallback_successes: usize,
    pub(crate) emitted_condition_ids: Vec<String>,
    pub(crate) failed_condition_ids: Vec<String>,
    pub(crate) used_watchlist_fallback: bool,
    pub(crate) timed_out_watchlist: usize,
    pub(crate) error: Option<String>,
    pub(crate) ts_event: UnixNanos,
    pub(crate) ts_init: UnixNanos,
}

impl PolymarketResolveRequestSummaryData {
    pub(crate) fn from_summary(summary: ResolveRequestSummary, ts_now: UnixNanos) -> Self {
        Self {
            requested_condition_ids: summary.requested_condition_ids,
            fetched_markets: summary.fetched_markets,
            resolved_markets: summary.resolved_markets,
            skipped_non_binary_markets: summary.skipped_non_binary_markets,
            clob_fallback_successes: summary.clob_fallback_successes,
            emitted_condition_ids: summary.emitted_condition_ids,
            failed_condition_ids: summary.failed_condition_ids,
            used_watchlist_fallback: summary.used_watchlist_fallback,
            timed_out_watchlist: summary.timed_out_watchlist,
            error: summary.error,
            ts_event: ts_now,
            ts_init: ts_now,
        }
    }
}

impl HasTsInit for PolymarketResolveRequestSummaryData {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for PolymarketResolveRequestSummaryData {
    fn type_name(&self) -> &'static str {
        RESOLVE_REQUEST_TYPE_NAME
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(other) = other.as_any().downcast_ref::<Self>() {
            self == other
        } else {
            false
        }
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        let dict = pyo3::types::PyDict::new(py);
        dict.set_item(
            "requested_condition_ids",
            self.requested_condition_ids.clone(),
        )?;
        dict.set_item("fetched_markets", self.fetched_markets)?;
        dict.set_item("resolved_markets", self.resolved_markets)?;
        dict.set_item(
            "skipped_non_binary_markets",
            self.skipped_non_binary_markets,
        )?;
        dict.set_item("clob_fallback_successes", self.clob_fallback_successes)?;
        dict.set_item("emitted_condition_ids", self.emitted_condition_ids.clone())?;
        dict.set_item("failed_condition_ids", self.failed_condition_ids.clone())?;
        dict.set_item("used_watchlist_fallback", self.used_watchlist_fallback)?;
        dict.set_item("timed_out_watchlist", self.timed_out_watchlist)?;
        dict.set_item("error", self.error.clone())?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        Ok(dict.unbind().into())
    }

    fn type_name_static() -> &'static str {
        RESOLVE_REQUEST_TYPE_NAME
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let parsed: Self = serde_json::from_value(value)?;
        Ok(Arc::new(parsed))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "python")]
    use nautilus_core::UnixNanos;
    #[cfg(feature = "python")]
    use nautilus_model::data::custom::CustomDataTrait;
    #[cfg(feature = "python")]
    use pyo3::types::PyAnyMethods;
    #[cfg(feature = "python")]
    use rstest::rstest;

    #[cfg(feature = "python")]
    use super::*;

    #[cfg(feature = "python")]
    #[rstest]
    fn resolve_request_summary_to_pyobject_returns_dict_payload() {
        let summary = ResolveRequestSummary {
            requested_condition_ids: vec!["0xCOND-A".to_string()],
            fetched_markets: 1,
            resolved_markets: 1,
            skipped_non_binary_markets: 0,
            clob_fallback_successes: 0,
            emitted_condition_ids: vec!["0xCOND-A".to_string()],
            failed_condition_ids: Vec::new(),
            used_watchlist_fallback: false,
            timed_out_watchlist: 0,
            error: None,
        };
        let payload =
            PolymarketResolveRequestSummaryData::from_summary(summary, UnixNanos::from(123_u64));

        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let obj = payload
                .to_pyobject(py)
                .expect("expected summary conversion to Python object");
            let bound = obj.bind(py);
            let dict = bound
                .cast::<pyo3::types::PyDict>()
                .expect("expected Python dict payload");

            let requested = dict
                .get_item("requested_condition_ids")
                .expect("failed to read requested_condition_ids")
                .expect("expected requested_condition_ids");
            let requested_vec: Vec<String> = requested
                .extract()
                .expect("expected requested_condition_ids as list[str]");
            assert_eq!(requested_vec, vec!["0xCOND-A".to_string()]);

            let resolved = dict
                .get_item("resolved_markets")
                .expect("failed to read resolved_markets")
                .expect("expected resolved_markets");
            let resolved_count: usize = resolved
                .extract()
                .expect("expected resolved_markets as integer");
            assert_eq!(resolved_count, 1);

            let skipped = dict
                .get_item("skipped_non_binary_markets")
                .expect("failed to read skipped_non_binary_markets")
                .expect("expected skipped_non_binary_markets");
            let skipped_count: usize = skipped
                .extract()
                .expect("expected skipped_non_binary_markets as integer");
            assert_eq!(skipped_count, 0);

            let clob_successes = dict
                .get_item("clob_fallback_successes")
                .expect("failed to read clob_fallback_successes")
                .expect("expected clob_fallback_successes");
            let clob_success_count: usize = clob_successes
                .extract()
                .expect("expected clob_fallback_successes");
            assert_eq!(clob_success_count, 0);
        });
    }
}
