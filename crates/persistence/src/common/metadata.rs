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
    let mut range: Option<(u64, u64)> = None;

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
            range = Some(range.map_or((value, value), |(start_ts, end_ts)| {
                (start_ts.min(value), end_ts.max(value))
            }));
        }
    }

    range.ok_or_else(|| anyhow::anyhow!("Record batches contain no non-null ts_init values"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{ArrayRef, UInt64Array},
        datatypes::{DataType, Field, Schema},
    };
    use rstest::rstest;

    use super::*;

    fn ts_init_batch(values: Vec<u64>) -> RecordBatch {
        let field = Field::new("ts_init", DataType::UInt64, false);
        let schema = Arc::new(Schema::new(vec![field]));
        let column: ArrayRef = Arc::new(UInt64Array::from(values));
        RecordBatch::try_new(schema, vec![column]).unwrap()
    }

    #[rstest]
    fn record_batch_ts_init_range_spans_unordered_batches() {
        let batches = [ts_init_batch(vec![5, 3]), ts_init_batch(vec![9, 1, 4])];

        let range = record_batch_ts_init_range(&batches).unwrap();

        assert_eq!(range, (1, 9));
    }

    #[rstest]
    fn record_batch_ts_init_range_rejects_batches_without_rows() {
        let error = record_batch_ts_init_range(&[ts_init_batch(Vec::new())]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Record batches contain no non-null ts_init values"
        );
    }
}
