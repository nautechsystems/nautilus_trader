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

use nautilus_model::identifiers::InstrumentId;

pub(crate) struct ReportRead<T> {
    pub(crate) reports: Vec<T>,
    pub(crate) historical_incomplete: bool,
}

impl<T> ReportRead<T> {
    pub(crate) fn new() -> Self {
        Self {
            reports: Vec::new(),
            historical_incomplete: false,
        }
    }

    pub(crate) fn record_historical_miss(&mut self, instrument_id: InstrumentId) {
        log::warn!(
            "Instrument {instrument_id} missing from reconciliation cache, history is incomplete"
        );
        self.historical_incomplete = true;
    }

    pub(crate) fn into_reports(self) -> anyhow::Result<Vec<T>> {
        anyhow::ensure!(
            !self.historical_incomplete,
            "in-scope historical instruments missing from reconciliation cache"
        );
        Ok(self.reports)
    }
}

pub(crate) fn instrument_in_scope(
    instrument_id: InstrumentId,
    load_ids: Option<&[InstrumentId]>,
) -> bool {
    load_ids.is_none_or(|ids| ids.is_empty() || ids.contains(&instrument_id))
}

pub(crate) fn millis_in_window(
    value: &str,
    start: Option<i64>,
    end: Option<i64>,
) -> anyhow::Result<bool> {
    if start.is_none() && end.is_none() {
        return Ok(true);
    }

    let millis = value.parse::<i64>()?;
    Ok(start.is_none_or(|start| millis >= start) && end.is_none_or(|end| millis <= end))
}
