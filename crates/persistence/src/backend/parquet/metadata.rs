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

//! Parquet catalog metadata queries.

use std::collections::{BTreeMap, HashMap};

use datafusion::arrow::record_batch::RecordBatch;
use nautilus_core::UnixNanos;
use nautilus_serialization::arrow::U64ColumnRef;

use crate::{
    backend::parquet::{
        catalog::ParquetDataCatalog,
        io::read_parquet_schema_from_object_store,
        paths::{
            extract_identifier_from_path, extract_sql_safe_filename, make_sql_safe_identifier,
        },
    },
    catalog::traits::CatalogMetadata,
    common::{datafusion::build_query, metadata::arrow_metadata_to_params},
};

impl ParquetDataCatalog {
    /// Queries Arrow schema metadata and the first queried timestamp where each metadata is used.
    ///
    /// # Errors
    ///
    /// Returns an error if file discovery, Parquet metadata reading, or query execution fails.
    pub fn query_metadata(
        &mut self,
        data_type: &str,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
    ) -> anyhow::Result<Vec<CatalogMetadata>> {
        self.clear_session_tables();
        self.register_remote_object_store()?;

        let files_list = self.query_files(data_type, identifiers, start, end)?;
        let table_prefix = make_sql_safe_identifier(data_type);
        let mut metadata_by_key: BTreeMap<String, CatalogMetadata> = BTreeMap::new();

        for file_uri in &files_list {
            let object_path = self.to_object_path_parsed(file_uri)?;
            let metadata = self.execute_async(|| async {
                let schema =
                    read_parquet_schema_from_object_store(self.object_store.clone(), &object_path)
                        .await?;
                Ok::<HashMap<String, String>, anyhow::Error>(schema.metadata().clone())
            })?;

            let identifier = extract_identifier_from_path(file_uri).ok_or_else(|| {
                anyhow::anyhow!("Cannot extract identifier from path '{file_uri}'")
            })?;
            let safe_sql_identifier = make_sql_safe_identifier(identifier);
            let safe_filename = extract_sql_safe_filename(file_uri);
            let table_name = format!("{table_prefix}_{safe_sql_identifier}_{safe_filename}");
            let query = build_query(&table_name, start, end, where_clause);
            let resolved_path = self.resolve_path_for_datafusion(file_uri);
            let batches = self.session.collect_parquet_files_batches(
                &table_name,
                vec![resolved_path],
                Some(&query),
            )?;

            let Some(first_ts_init) = first_ts_init_from_batches(&batches)? else {
                continue;
            };

            let key = canonical_metadata_key(&metadata)?;
            let metadata = arrow_metadata_to_params(&metadata);

            match metadata_by_key.get_mut(&key) {
                Some(existing) => {
                    if first_ts_init < existing.first_ts_init {
                        existing.first_ts_init = first_ts_init;
                    }
                }
                None => {
                    metadata_by_key.insert(
                        key,
                        CatalogMetadata {
                            first_ts_init,
                            metadata,
                        },
                    );
                }
            }
        }

        let mut metadata = metadata_by_key.into_values().collect::<Vec<_>>();
        metadata.sort_by_key(|item| item.first_ts_init);
        Ok(metadata)
    }
}

fn first_ts_init_from_batches(batches: &[RecordBatch]) -> anyhow::Result<Option<UnixNanos>> {
    let mut first_ts_init: Option<u64> = None;

    for batch in batches {
        if batch.num_rows() == 0 {
            continue;
        }

        let column = batch
            .column_by_name("ts_init")
            .ok_or_else(|| anyhow::anyhow!("ts_init column not found"))?;
        let ts_init = U64ColumnRef::try_from_array(column.as_ref())
            .ok_or_else(|| anyhow::anyhow!("ts_init column has an unsupported type"))?;

        for row in 0..batch.num_rows() {
            if ts_init.is_null(row) {
                continue;
            }

            let value = ts_init
                .value(row)
                .ok_or_else(|| anyhow::anyhow!("ts_init value cannot be negative"))?;
            first_ts_init = Some(first_ts_init.map_or(value, |current| current.min(value)));
        }
    }

    Ok(first_ts_init.map(UnixNanos::from))
}

fn canonical_metadata_key(metadata: &HashMap<String, String>) -> anyhow::Result<String> {
    let ordered = metadata
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    Ok(serde_json::to_string(&ordered)?)
}
