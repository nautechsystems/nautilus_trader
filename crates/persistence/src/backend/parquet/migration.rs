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

//! Legacy Parquet catalog rewrite into the current Parquet schema and layout.

use futures::StreamExt;
use nautilus_core::UnixNanos;
use nautilus_model::instruments::NautilusInstrumentType;
use nautilus_serialization::arrow::record_batch_without_identifier_column;
use object_store::{PutMode, PutOptions, path::Path as ObjectPath};

use super::{
    catalog::ParquetDataCatalog, io::write_batches_to_object_store_create,
    paths::timestamps_to_filename,
};
use crate::{
    backend::migration::{
        CatalogMigrationPlan, CatalogMigrationReport, IdentifierSource, ParquetCatalogSource,
        build_catalog_migration_plan, ensure_distinct_migration_locations,
        ensure_planned_file_unchanged, prepare_migration_parts, read_planned_migration_file,
    },
    catalog::types::instrument_path_prefix,
    common::metadata::record_batch_ts_init_range,
};

/// Settings for converting a legacy Parquet catalog into a separate native catalog.
#[derive(Debug)]
pub struct ParquetMigrationConfig {
    pub source_uri: String,
    pub target_uri: String,
    pub source_options: Vec<(String, String)>,
    pub target_options: Vec<(String, String)>,
    pub dry_run: bool,
}

/// Converts the current or legacy Arrow representation of a Parquet catalog.
///
/// The source remains unchanged. The destination must be empty, and preflight rejects
/// unsupported schemas before any destination files are written.
///
/// # Errors
///
/// Returns an error for overlapping locations, invalid schemas, a nonempty destination,
/// or an object-store read or write failure.
pub fn migrate_parquet_catalog(
    config: ParquetMigrationConfig,
) -> anyhow::Result<CatalogMigrationReport> {
    let source_uri = crate::common::storage::normalize_storage_location(&config.source_uri)?;
    let target_uri = crate::common::storage::normalize_storage_location(&config.target_uri)?;
    ensure_distinct_migration_locations(&source_uri, &target_uri)?;
    let source = ParquetDataCatalog::from_uri(
        &source_uri,
        Some(config.source_options.into_iter().collect()),
        None,
        None,
        None,
    )?;
    let plan = build_catalog_migration_plan(&source)?;
    plan.ensure_ready()?;
    if config.dry_run {
        return Ok(CatalogMigrationReport::from_plan(&plan, true));
    }
    let url = url::Url::parse(&target_uri)?;
    if url.scheme() == "file" {
        let path = url
            .to_file_path()
            .map_err(|()| anyhow::anyhow!("Invalid destination file URI"))?;
        std::fs::create_dir_all(path)?;
    }
    let target = ParquetDataCatalog::from_uri(
        &target_uri,
        Some(config.target_options.into_iter().collect()),
        None,
        None,
        None,
    )?;
    target.migrate_from_legacy_parquet_catalog_plan(&source, &plan)
}

impl ParquetCatalogSource for ParquetDataCatalog {
    fn object_store(&self) -> std::sync::Arc<dyn object_store::ObjectStore> {
        self.object_store.clone()
    }
    fn base_path(&self) -> &str {
        &self.base_path
    }
    fn original_uri(&self) -> &str {
        &self.original_uri
    }
}

impl ParquetDataCatalog {
    /// Rewrites a legacy Parquet catalog into this current Parquet catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if source preflight fails, this catalog contains any leaf object, or a
    /// source file cannot be read, converted, or written.
    pub fn migrate_from_legacy_parquet_catalog(
        &self,
        source: &Self,
    ) -> anyhow::Result<CatalogMigrationReport> {
        ensure_distinct_migration_locations(&source.original_uri, &self.original_uri)?;
        let plan = build_catalog_migration_plan(source)?;
        plan.ensure_ready()?;
        self.migrate_from_legacy_parquet_catalog_plan(source, &plan)
    }

    fn migrate_from_legacy_parquet_catalog_plan(
        &self,
        source: &Self,
        plan: &CatalogMigrationPlan,
    ) -> anyhow::Result<CatalogMigrationReport> {
        self.ensure_migration_target_empty()?;
        let mut report = CatalogMigrationReport::from_plan(plan, false);

        for file in &plan.files {
            if file.size == 0 {
                let source_path = source.to_object_path_parsed(&file.path)?;
                ensure_planned_file_unchanged(source, file, &source_path)?;
                let target_path =
                    self.to_object_path(&format!("{}/{}", self.base_path, file.relative_path))?;
                self.execute_async(|| async {
                    self.object_store
                        .put_opts(
                            &target_path,
                            Vec::new().into(),
                            PutOptions {
                                mode: PutMode::Create,
                                ..Default::default()
                            },
                        )
                        .await?;
                    Ok(())
                })?;
                report.record_migrated_file(file, 0, 0);
                continue;
            }
            let batches = read_planned_migration_file(source, file)?;
            let mut migrated_rows = 0;
            let mut path_identifier_rows = 0;

            for part in prepare_migration_parts(file, batches)? {
                if part.row_count == 0 {
                    continue;
                }
                let directory = if let Some(custom_type_name) =
                    file.target_type_name.strip_prefix("custom/")
                {
                    self.make_path_custom_data(custom_type_name, part.identifier.as_deref())?
                } else {
                    {
                        let prefix =
                            if let Some(class) = file.target_table.strip_prefix("instruments/") {
                                instrument_path_prefix(&class.parse::<NautilusInstrumentType>()?)
                            } else {
                                &file.target_type_name
                            };
                        self.make_path(prefix, part.identifier.as_deref())?
                    }
                };
                let (start_ts, end_ts) = record_batch_ts_init_range(&part.batches)?;
                let filename =
                    timestamps_to_filename(UnixNanos::from(start_ts), UnixNanos::from(end_ts));
                let object_path = self.to_object_path(&format!("{directory}/{filename}"))?;
                let batches = part
                    .batches
                    .into_iter()
                    .map(record_batch_without_identifier_column)
                    .collect::<Result<Vec<_>, _>>()?;

                self.execute_async(|| async {
                    write_batches_to_object_store_create(
                        &batches,
                        self.object_store.clone(),
                        &object_path,
                        Some(self.compression),
                        Some(self.max_row_group_size),
                        None,
                    )
                    .await
                })
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Parquet migration target object already exists or cannot be created: \
                         {object_path}: {e}"
                    )
                })?;
                migrated_rows += part.row_count;
                if part.identifier_source == IdentifierSource::Path {
                    path_identifier_rows += part.row_count;
                }
            }

            if migrated_rows == 0 {
                report.record_skipped_file(file);
            } else {
                report.record_migrated_file(file, migrated_rows, path_identifier_rows);
            }
        }

        Ok(report)
    }

    fn ensure_migration_target_empty(&self) -> anyhow::Result<()> {
        let prefix = (!self.base_path.is_empty()).then(|| ObjectPath::from(self.base_path.clone()));
        self.execute_async(|| async {
            let mut objects = self.object_store.list(prefix.as_ref());
            anyhow::ensure!(
                objects.next().await.transpose()?.is_none(),
                "Parquet migration target must be new or empty",
            );
            Ok(())
        })
    }
}
