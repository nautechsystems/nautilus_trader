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

use std::collections::HashMap;

use arrow::record_batch::RecordBatch;
use nautilus_core::Params;
use nautilus_serialization::arrow::U64ColumnRef;

/// Converts string Arrow schema metadata into generic catalog params.
#[must_use]
pub(crate) fn arrow_metadata_to_params(metadata: &HashMap<String, String>) -> Params {
    let mut params = Params::new();
    let mut entries = metadata.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(key, _)| *key);

    for (key, value) in entries {
        params.insert(key.clone(), serde_json::Value::String(value.clone()));
    }

    params
}

/// Returns the inclusive `ts_init` range across Arrow record batches.
pub(crate) fn record_batch_ts_init_range(batches: &[RecordBatch]) -> anyhow::Result<(u64, u64)> {
    let mut start_ts: Option<u64> = None;
    let mut end_ts: Option<u64> = None;

    for batch in batches {
        let ts_init = batch
            .column_by_name("ts_init")
            .ok_or_else(|| anyhow::anyhow!("ts_init column not found"))?;
        let ts_init = U64ColumnRef::try_from_array(ts_init.as_ref())
            .ok_or_else(|| anyhow::anyhow!("ts_init column has an unsupported type"))?;

        for row in 0..ts_init.len() {
            if ts_init.is_null(row) {
                anyhow::bail!("ts_init column contains null values");
            }

            let value = ts_init
                .value(row)
                .ok_or_else(|| anyhow::anyhow!("ts_init column contains a negative value"))?;
            start_ts = Some(start_ts.map_or(value, |current| current.min(value)));
            end_ts = Some(end_ts.map_or(value, |current| current.max(value)));
        }
    }

    match (start_ts, end_ts) {
        (Some(start_ts), Some(end_ts)) => Ok((start_ts, end_ts)),
        _ => anyhow::bail!("Record batches contain no non-null ts_init values"),
    }
}
