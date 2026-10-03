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

//! Target-neutral planning and reading for legacy Parquet catalog migration.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::{Debug, Display},
    sync::Arc,
};

use ahash::AHashSet;
use arrow::{
    array::{
        Array, ArrayRef, BinaryArray, Decimal128Array, FixedSizeListArray, ListArray,
        StringBuilder, StructArray, UInt32Array, UInt64Array,
    },
    buffer::{OffsetBuffer, ScalarBuffer},
    compute::{cast, take_record_batch},
    datatypes::{DataType as ArrowDataType, Field, Fields, Schema, TimeUnit},
    record_batch::RecordBatch,
};
use futures::{StreamExt, TryStreamExt};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, NautilusDataType, NautilusRecordType, OptionGreeks, OrderBookDelta,
        OrderBookDepth, QuoteTick, TradeTick, depth::DEPTH10_LEN,
    },
    instruments::{InstrumentAny, NautilusInstrumentType},
};
use nautilus_serialization::arrow::{
    ArrowSchemaProvider, EncodeToRecordBatch, KEY_BAR_TYPE, KEY_IDENTIFIER, KEY_INSTRUMENT_ID,
    KEY_TYPE_NAME, StringColumnRef,
    instrument::decode_instrument_any_batch,
    legacy::{
        LegacyArrowError, LegacySchemaResolution, LegacyTranscodeKind, LegacyTranscodeState,
        SchemaFingerprint, is_nautilus_legacy_schema, is_nautilus_timestamp_schema,
        normalize_legacy_fixed_columns,
        normalized_legacy_data_type as normalize_legacy_arrow_data_type, normalized_timestamp_type,
        resolve_legacy_schema, schema_fingerprint, transcode_legacy_record_batch_with_state,
    },
    record_batch_with_identifier_column, schema_without_identifier_column,
};
use object_store::{ObjectMeta, ObjectStore, ObjectStoreExt, path::Path as ObjectPath};
use parquet::{read_parquet_from_object_store, read_parquet_schema_from_object_store};
use serde::Serialize;
use strum::IntoEnumIterator;

use crate::{
    backend::parquet::io as parquet,
    catalog::types::{
        INSTRUMENT_PATH_PREFIXES, data_path_prefix, data_type_from_data_path_prefix,
        record_path_prefix,
    },
    common::{
        arrow::{catalog_record_schema, round_trip_batches, round_trip_catalog_record_batches},
        paths::normalize_path_separators,
        storage::normalize_storage_location,
    },
};

const SCHEMA_READ_CONCURRENCY: usize = 16;
const LEGACY_KEY_CLASS: &str = "class";

/// Default maximum number of source rows written in one open-catalog migration commit.
pub const DEFAULT_MIGRATION_COMMIT_ROWS: usize = 500_000;

/// Object-store source used to plan and read a legacy Parquet migration.
pub trait ParquetCatalogSource: Sync {
    /// Returns the object store containing the source catalog.
    fn object_store(&self) -> Arc<dyn ObjectStore>;
    /// Returns the catalog path within the object store.
    fn base_path(&self) -> &str;
    /// Returns the URI identifying the source catalog.
    fn original_uri(&self) -> &str;

    /// Resolves a plan-relative path within the source catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if the combined object-store path is invalid.
    fn to_object_path_parsed(&self, path: &str) -> anyhow::Result<ObjectPath> {
        let normalized = normalize_path_separators(path);
        let base = self.base_path().trim_matches('/');

        let full = if base.is_empty() {
            normalized
        } else {
            format!("{base}/{}", normalized.trim_start_matches('/'))
        };

        ObjectPath::parse(full.trim_start_matches('/')).map_err(anyhow::Error::from)
    }
}

/// Migration counters for one current target type.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CatalogMigrationTypeReport {
    pub planned_files: usize,
    pub migrated_files: usize,
    pub skipped_files: usize,
    pub migrated_rows: usize,
    pub transcoded_rows: usize,
    pub path_identifier_rows: usize,
}

/// Structured result shared by Parquet and Delta migration entry points.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CatalogMigrationReport {
    pub dry_run: bool,
    pub total_leaf_files: usize,
    pub migrated_files: usize,
    pub skipped_files: usize,
    pub migrated_rows: usize,
    pub transcoded_rows: usize,
    pub path_identifier_rows: usize,
    pub unmigrated: Vec<UnmigratedFile>,
    pub types: BTreeMap<String, CatalogMigrationTypeReport>,
}

impl CatalogMigrationReport {
    /// Builds a report with preflight file accounting and no writes.
    #[must_use]
    pub fn from_plan(plan: &CatalogMigrationPlan, dry_run: bool) -> Self {
        let mut types = BTreeMap::new();
        for file in &plan.files {
            types
                .entry(file.target_type_name.clone())
                .or_insert_with(CatalogMigrationTypeReport::default)
                .planned_files += 1;
        }

        Self {
            dry_run,
            total_leaf_files: plan.total_leaf_files,
            unmigrated: plan.unmigrated.clone(),
            types,
            ..Self::default()
        }
    }

    pub(crate) fn record_migrated_file(
        &mut self,
        file: &PlannedMigrationFile,
        rows: usize,
        path_identifier_rows: usize,
    ) {
        let transcoded_rows = if file.transcode_kind == LegacyTranscodeKind::PassThrough {
            0
        } else {
            rows
        };

        self.migrated_files += 1;
        self.migrated_rows += rows;
        self.transcoded_rows += transcoded_rows;
        self.path_identifier_rows += path_identifier_rows;
        let type_report = self.types.entry(file.target_type_name.clone()).or_default();
        type_report.migrated_files += 1;
        type_report.migrated_rows += rows;
        type_report.transcoded_rows += transcoded_rows;
        type_report.path_identifier_rows += path_identifier_rows;
    }

    pub(crate) fn record_skipped_file(&mut self, file: &PlannedMigrationFile) {
        self.skipped_files += 1;
        self.types
            .entry(file.target_type_name.clone())
            .or_default()
            .skipped_files += 1;
    }
}

impl Display for CatalogMigrationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{}: {} planned leaf files, {} migrated files, {} migrated rows, {} skipped files, {} \
             unmigrated files",
            if self.dry_run {
                "Migration dry-run report"
            } else {
                "Migration report"
            },
            self.total_leaf_files,
            self.migrated_files,
            self.migrated_rows,
            self.skipped_files,
            self.unmigrated.len(),
        )?;

        for (type_name, report) in &self.types {
            writeln!(
                f,
                "{type_name}: {} planned files, {} migrated files, {} rows, {} transcoded rows, {} \
                 path-derived identifier rows, {} skipped files",
                report.planned_files,
                report.migrated_files,
                report.migrated_rows,
                report.transcoded_rows,
                report.path_identifier_rows,
                report.skipped_files,
            )?;
        }

        let mut unmigrated_directories = BTreeMap::new();

        for file in &self.unmigrated {
            let directory = file
                .path
                .rsplit_once('/')
                .map_or(".", |(directory, _)| directory);
            *unmigrated_directories.entry(directory).or_insert(0_usize) += 1;
        }

        for (directory, file_count) in unmigrated_directories {
            writeln!(f, "Unmigrated directory {directory}: {file_count} files")?;
        }

        for file in &self.unmigrated {
            writeln!(f, "Unmigrated {}: {}", file.path, file.reason)?;
        }

        Ok(())
    }
}

/// One source file accepted by migration preflight.
#[derive(Clone, Debug)]
pub struct PlannedMigrationFile {
    pub path: String,
    pub relative_path: String,
    pub source_type_name: String,
    pub target_type_name: String,
    pub target_table: String,
    pub size: u64,
    pub e_tag: Option<String>,
    pub version: Option<String>,
    pub last_modified: String,
    pub source_fingerprint: SchemaFingerprint,
    pub target_fingerprint: SchemaFingerprint,
    pub transcode_kind: LegacyTranscodeKind,
}

/// One source path excluded from migration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnmigratedFile {
    pub path: String,
    pub reason: String,
}

/// One schema and example source path in a target-table conflict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaConflictExample {
    pub fingerprint: SchemaFingerprint,
    pub path: String,
}

/// Incompatible final schemas targeting one output table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaConflict {
    pub target_table: String,
    pub examples: Vec<SchemaConflictExample>,
}

/// One source file whose schema needs an unregistered transcoder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedSchema {
    pub path: String,
    pub message: String,
}

/// Complete read-only result of source discovery and schema preflight.
#[derive(Clone, Debug, Default)]
pub struct CatalogMigrationPlan {
    pub files: Vec<PlannedMigrationFile>,
    pub unmigrated: Vec<UnmigratedFile>,
    pub conflicts: Vec<SchemaConflict>,
    pub unresolved_schemas: Vec<UnresolvedSchema>,
    pub total_leaf_files: usize,
}

impl CatalogMigrationPlan {
    /// Returns whether preflight found a condition that prevents writing.
    #[must_use]
    pub const fn has_errors(&self) -> bool {
        !self.conflicts.is_empty() || !self.unresolved_schemas.is_empty()
    }

    /// Rejects a plan that cannot be written safely.
    ///
    /// # Errors
    ///
    /// Returns one summary error containing every schema conflict and unresolved schema.
    pub fn ensure_ready(&self) -> anyhow::Result<()> {
        if !self.has_errors() {
            return Ok(());
        }

        let mut messages = self
            .conflicts
            .iter()
            .map(|conflict| {
                let examples = conflict
                    .examples
                    .iter()
                    .map(|example| format!("{} ({})", example.path, example.fingerprint))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "target table {} has conflicting schemas: {examples}",
                    conflict.target_table
                )
            })
            .collect::<Vec<_>>();

        messages.extend(
            self.unresolved_schemas
                .iter()
                .map(|schema| schema.message.clone()),
        );
        anyhow::bail!(
            "Catalog migration preflight failed:\n{}",
            messages.join("\n")
        );
    }
}

/// How a migrated identifier was resolved.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum IdentifierSource {
    Metadata,
    Row,
    Path,
    Absent,
}

/// Batches from one source file that share one resolved identifier.
#[derive(Debug)]
pub struct PreparedMigrationPart {
    pub identifier: Option<String>,
    pub identifier_source: IdentifierSource,
    pub batches: Vec<RecordBatch>,
    pub row_count: usize,
}

/// Enumerates and resolves every source leaf file without writing a target.
///
/// # Errors
///
/// Returns an error if source listing or Parquet schema reading fails.
pub fn build_catalog_migration_plan(
    source: &dyn ParquetCatalogSource,
) -> anyhow::Result<CatalogMigrationPlan> {
    let objects = list_source_objects(source)?;
    let total_leaf_files = objects.len();
    let mut candidates = Vec::new();
    let mut unmigrated = Vec::new();

    for object in objects {
        let relative_path = relative_object_path(source, &object.location);
        match classify_source_path(&relative_path) {
            SourceClassification::Migratable {
                source_type_name,
                target_type_name,
            } if relative_path.ends_with(".parquet") => {
                candidates.push(SchemaCandidate {
                    object_path: object.location.clone(),
                    object,
                    relative_path,
                    source_type_name,
                    target_type_name,
                });
            }
            SourceClassification::Migratable { .. } => {
                unmigrated.push(UnmigratedFile {
                    path: relative_path,
                    reason: "recognized catalog directory contains a non-Parquet leaf".to_string(),
                });
            }
            SourceClassification::Unmigrated(reason) => {
                unmigrated.push(UnmigratedFile {
                    path: relative_path,
                    reason,
                });
            }
        }
    }

    let (mut files, unresolved_schemas) = resolve_candidate_schemas(source, candidates)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    unmigrated.sort_by(|left, right| left.path.cmp(&right.path));
    let conflicts = schema_conflicts(&files);

    Ok(CatalogMigrationPlan {
        files,
        unmigrated,
        conflicts,
        unresolved_schemas,
        total_leaf_files,
    })
}

/// Reads, normalizes, and transcodes one planned source file.
///
/// # Errors
///
/// Returns an error if the source file cannot be read, normalized, or transcoded.
pub fn read_planned_migration_file(
    source: &dyn ParquetCatalogSource,
    file: &PlannedMigrationFile,
) -> anyhow::Result<Vec<RecordBatch>> {
    let object_path = source.to_object_path_parsed(&file.path)?;

    let (batches, schema) = execute_async(|| async {
        read_parquet_from_object_store(source.object_store(), &object_path).await
    })?;

    ensure_planned_file_unchanged(source, file, &object_path)?;
    let mut state = LegacyTranscodeState::default();
    let mut transcoded = Vec::new();

    for batch in record_batches_with_schema(batches, &schema)? {
        let batch = with_inferred_custom_type_name(file, batch)?;
        let batch = normalize_legacy_parquet_columns(&batch)?;
        let result = transcode_legacy_record_batch_with_state(
            &file.target_type_name,
            &file.path,
            batch,
            &mut state,
        )?;
        transcoded.extend(result.batches);
    }

    Ok(transcoded)
}

/// Resolves identifiers, re-encodes built-in batches with the current encoders, and groups
/// batches from one source file.
///
/// # Errors
///
/// Returns an error when an identifier column is not string-like, a batch cannot be sliced or
/// rebuilt with current identifier metadata, or a built-in batch fails its current decoder.
pub fn prepare_migration_parts(
    file: &PlannedMigrationFile,
    batches: Vec<RecordBatch>,
) -> anyhow::Result<Vec<PreparedMigrationPart>> {
    let mut grouped: BTreeMap<(Option<String>, IdentifierSource), Vec<RecordBatch>> =
        BTreeMap::new();

    for batch in batches {
        for (identifier, source, batch) in split_batch_by_identifier(file, batch)? {
            if batch.num_rows() == 0 {
                continue;
            }

            let batch =
                batch_with_identifier(&file.target_type_name, identifier.as_deref(), batch)?;
            grouped.entry((identifier, source)).or_default().push(batch);
        }
    }

    grouped
        .into_iter()
        .map(|((identifier, identifier_source), batches)| {
            let row_count = batches.iter().map(RecordBatch::num_rows).sum();
            let batches = round_trip_migrated_batches(&file.target_type_name, batches)?;
            Ok(PreparedMigrationPart {
                identifier,
                identifier_source,
                batches,
                row_count,
            })
        })
        .collect()
}

/// Parses a storage option expressed as `key=value`.
///
/// # Errors
///
/// Returns an error when the separator or either side is absent.
pub fn parse_storage_option(option: &str) -> Result<(String, String), String> {
    let (key, value) = option
        .split_once('=')
        .ok_or_else(|| format!("Storage option must use key=value: {option}"))?;

    if key.is_empty() || value.is_empty() {
        return Err(format!(
            "Storage option must use non-empty key=value: {option}"
        ));
    }

    Ok((key.to_string(), value.to_string()))
}

pub(crate) fn ensure_distinct_migration_locations(
    source_uri: &str,
    target_uri: &str,
) -> anyhow::Result<()> {
    let source_uri = normalize_storage_location(source_uri)?;
    let target_uri = normalize_storage_location(target_uri)?;
    let source_uri = source_uri.trim_end_matches('/');
    let target_uri = target_uri.trim_end_matches('/');
    anyhow::ensure!(
        source_uri != target_uri
            && !target_uri.starts_with(&format!("{source_uri}/"))
            && !source_uri.starts_with(&format!("{target_uri}/")),
        "Migration source and target must be distinct, non-overlapping locations",
    );
    Ok(())
}

fn list_source_objects(source: &dyn ParquetCatalogSource) -> anyhow::Result<Vec<ObjectMeta>> {
    let prefix =
        (!source.base_path().is_empty()).then(|| ObjectPath::from(source.base_path().to_string()));

    let mut objects = execute_async(|| async {
        Ok(source
            .object_store()
            .list(prefix.as_ref())
            .try_collect::<Vec<_>>()
            .await?)
    })?;

    // The OpenDAL filesystem adapter lists directory entries alongside leaves; an entry
    // that is the parent of another listed entry is a directory, not a migratable leaf.
    objects.sort_by(|left, right| left.location.as_ref().cmp(right.location.as_ref()));

    let parents = objects
        .windows(2)
        .filter(|pair| {
            pair[1]
                .location
                .as_ref()
                .starts_with(&format!("{}/", pair[0].location.as_ref()))
        })
        .map(|pair| pair[0].location.clone())
        .collect::<AHashSet<_>>();

    objects.retain(|object| {
        !parents.contains(&object.location)
            && !relative_object_path(source, &object.location).is_empty()
    });

    Ok(objects)
}

fn execute_async<C, F, R>(create_future: C) -> anyhow::Result<R>
where
    C: FnOnce() -> F + Send,
    F: std::future::Future<Output = anyhow::Result<R>>,
    R: Send,
{
    nautilus_common::live::block_on_nautilus_with(create_future)
}

fn relative_object_path(source: &dyn ParquetCatalogSource, path: &ObjectPath) -> String {
    let path = path.as_ref();
    let base = source.base_path().trim_matches('/');
    if base.is_empty() {
        return path.to_string();
    }

    path.strip_prefix(&format!("{base}/"))
        .unwrap_or(path)
        .to_string()
}

#[derive(Clone, Debug)]
enum SourceClassification {
    Migratable {
        source_type_name: String,
        target_type_name: String,
    },
    Unmigrated(String),
}

fn classify_source_path(path: &str) -> SourceClassification {
    let parts = path.split('/').collect::<Vec<_>>();

    let Some(root) = parts.first().copied() else {
        return SourceClassification::Unmigrated("empty source path".to_string());
    };

    if matches!(root, "backtest" | "live") {
        return SourceClassification::Unmigrated(format!(
            "{root} Feather trees are outside catalog migration scope"
        ));
    }

    if root != "data" || parts.len() < 2 {
        return SourceClassification::Unmigrated(
            "path is outside the recognized data catalog tree".to_string(),
        );
    }

    let source_type_name = parts[1];
    if INSTRUMENT_PATH_PREFIXES.contains(&source_type_name) {
        return SourceClassification::Migratable {
            source_type_name: source_type_name.to_string(),
            target_type_name: "instruments".to_string(),
        };
    }

    if source_type_name == "custom" || source_type_name.starts_with("custom_") {
        return SourceClassification::Migratable {
            source_type_name: source_type_name.to_string(),
            target_type_name: "custom".to_string(),
        };
    }

    // v1 catalogs named the depth directory after the removed `OrderBookDepth10` type
    let data_type = match source_type_name {
        "order_book_depth10" => Ok(NautilusDataType::OrderBookDepth),
        _ => data_type_from_data_path_prefix(source_type_name),
    };

    if let Ok(data_type) = data_type {
        return SourceClassification::Migratable {
            source_type_name: source_type_name.to_string(),
            target_type_name: data_path_prefix(&data_type).into_owned(),
        };
    }

    if NautilusRecordType::iter()
        .any(|record_type| record_path_prefix(&record_type).as_ref() == source_type_name)
    {
        return SourceClassification::Migratable {
            source_type_name: source_type_name.to_string(),
            target_type_name: source_type_name.to_string(),
        };
    }

    let reason = if source_type_name == "portfolio_snapshot" {
        "no NautilusRecordType maps to data/portfolio_snapshot"
    } else {
        "unrecognized catalog data directory"
    };

    SourceClassification::Unmigrated(reason.to_string())
}

#[derive(Clone, Debug)]
struct SchemaCandidate {
    object: ObjectMeta,
    relative_path: String,
    source_type_name: String,
    target_type_name: String,
    object_path: ObjectPath,
}

#[expect(
    clippy::too_many_lines,
    reason = "Preflight resolves every candidate before assembling the migration plan"
)]
fn resolve_candidate_schemas(
    source: &dyn ParquetCatalogSource,
    candidates: Vec<SchemaCandidate>,
) -> anyhow::Result<(Vec<PlannedMigrationFile>, Vec<UnresolvedSchema>)> {
    let object_store = source.object_store();

    let resolved = execute_async(|| async move {
        futures::stream::iter(candidates)
            .map(|candidate| {
                let object_store = object_store.clone();
                async move {
                    let schema =
                        read_parquet_schema_from_object_store(object_store, &candidate.object_path)
                            .await?;
                    Ok::<_, anyhow::Error>((candidate, schema))
                }
            })
            .buffer_unordered(SCHEMA_READ_CONCURRENCY)
            .try_collect::<Vec<_>>()
            .await
    })?;

    let mut files = Vec::new();
    let mut unresolved = Vec::new();

    for (mut candidate, schema) in resolved {
        if candidate.object.size == 0 {
            // Mirror the data-file inference so markers land beside their data files
            if candidate.target_type_name == "custom"
                && let Some(inferred) = legacy_custom_type_name(&candidate.source_type_name)
            {
                candidate.target_type_name = format!("custom/{inferred}");
            }

            let fingerprint = schema_fingerprint(&Schema::empty());
            files.push(ResolvedCandidate {
                target_table: candidate.target_type_name.clone(),
                candidate,
                resolution: LegacySchemaResolution {
                    kind: LegacyTranscodeKind::PassThrough,
                    source_fingerprint: fingerprint.clone(),
                    target_fingerprint: fingerprint,
                },
            });

            continue;
        }

        // Resolve the custom target before normalization, so the preflight fingerprint
        // reflects the same type_name the execution path injects.
        if candidate.target_type_name == "custom" {
            if let Some(type_name) = schema.metadata().get("type_name") {
                candidate.target_type_name = format!("custom/{type_name}");
            } else if let Some(inferred) = legacy_custom_type_name(&candidate.source_type_name) {
                candidate.target_type_name = format!("custom/{inferred}");
            } else {
                unresolved.push(UnresolvedSchema {
                    path: candidate.relative_path.clone(),
                    message: format!(
                        "Parquet custom data file {} is missing type_name metadata",
                        candidate.relative_path
                    ),
                });

                continue;
            }
        }

        // Custom targets skip fingerprint validation, so reject unconvertible
        // timestamps here instead of migrating to an unreadable destination.
        if candidate.target_type_name.starts_with("custom/")
            && !custom_timestamps_convertible(&schema)
        {
            let detail = if schema.metadata().contains_key("type_name") {
                "has non-UInt64 timestamps with no transcoder"
            } else {
                "is missing type_name metadata and has non-UInt64 timestamps with no transcoder"
            };

            unresolved.push(UnresolvedSchema {
                path: candidate.relative_path.clone(),
                message: format!(
                    "Parquet custom data file {} {detail}",
                    candidate.relative_path
                ),
            });

            continue;
        }

        let schema = with_target_custom_type_name(&candidate, &schema);
        let schema = normalize_legacy_parquet_schema(&schema);

        let target_table = if candidate.target_type_name == "instruments" {
            let Some(instrument_type) = schema.metadata().get(KEY_TYPE_NAME) else {
                unresolved.push(UnresolvedSchema {
                    path: candidate.relative_path.clone(),
                    message: format!(
                        "Parquet instrument file {} is missing type_name metadata",
                        candidate.relative_path
                    ),
                });

                continue;
            };

            format!("instruments/{instrument_type}")
        } else {
            candidate.target_type_name.clone()
        };

        if let Ok(record_type) = candidate.target_type_name.parse::<NautilusRecordType>()
            && let Ok(current) = catalog_record_schema(record_type)
        {
            let expected = schema_without_identifier_column(&current);
            let actual = schema_without_identifier_column(&schema);

            if schema_fingerprint(&actual) != schema_fingerprint(&expected) {
                unresolved.push(UnresolvedSchema {
                    path: candidate.relative_path.clone(),
                    message: format!(
                        "Record file {} does not match the registered Arrow schema",
                        candidate.relative_path
                    ),
                });

                continue;
            }
        }

        if schema
            .fields()
            .iter()
            .any(|field| contains_legacy_fixed_binary(field.data_type()))
        {
            unresolved.push(UnresolvedSchema {
                path: candidate.relative_path.clone(),
                message: format!(
                    "No final-format transcoder is registered for fixed binary columns in {}",
                    candidate.relative_path
                ),
            });

            continue;
        }

        match resolve_legacy_schema(
            &candidate.target_type_name,
            &candidate.relative_path,
            &schema,
        ) {
            Ok(resolution) => files.push(ResolvedCandidate {
                candidate,
                target_table,
                resolution,
            }),
            Err(error @ LegacyArrowError::UnknownSchema { .. }) => {
                unresolved.push(UnresolvedSchema {
                    path: candidate.relative_path,
                    message: error.to_string(),
                });
            }
            Err(e) => return Err(e.into()),
        }
    }

    let files = files
        .into_iter()
        .map(|resolved| PlannedMigrationFile {
            path: resolved.candidate.object.location.to_string(),
            relative_path: resolved.candidate.relative_path,
            source_type_name: resolved.candidate.source_type_name,
            target_type_name: resolved.candidate.target_type_name,
            target_table: resolved.target_table,
            size: resolved.candidate.object.size,
            e_tag: resolved.candidate.object.e_tag,
            version: resolved.candidate.object.version,
            last_modified: resolved.candidate.object.last_modified.to_rfc3339(),
            source_fingerprint: resolved.resolution.source_fingerprint,
            target_fingerprint: resolved.resolution.target_fingerprint,
            transcode_kind: resolved.resolution.kind,
        })
        .collect();

    Ok((files, unresolved))
}

#[derive(Debug)]
struct ResolvedCandidate {
    candidate: SchemaCandidate,
    target_table: String,
    resolution: LegacySchemaResolution,
}

/// Infers a custom type name from a legacy `custom_<snake_case>` directory.
///
/// Old Python-written catalogs stored custom data under `data/custom_<snake_case>` without
/// `type_name` schema metadata. Best-effort reversal to PascalCase; acronyms do not survive
/// the round trip, but the known legacy layouts (e.g. `custom_binance_bar` -> `BinanceBar`)
/// map exactly. Returns `None` for the canonical `custom` directory, which carries no name.
fn legacy_custom_type_name(source_type_name: &str) -> Option<String> {
    let legacy = source_type_name.strip_prefix("custom_")?;
    let mut pascal = String::with_capacity(legacy.len());
    for part in legacy.split('_') {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            pascal.extend(first.to_uppercase());
            pascal.extend(chars);
        }
    }

    (!pascal.is_empty()).then_some(pascal)
}

/// Returns true when a custom schema can migrate: timestamp normalization converts
/// `UInt64` `ts_event`/`ts_init` and passes timestamps through, so any other
/// physical type (notably legacy `Int64`) has no transcoder.
fn custom_timestamps_convertible(schema: &Schema) -> bool {
    ["ts_event", "ts_init"].iter().all(|name| {
        schema.field_with_name(name).is_ok_and(|field| {
            matches!(
                field.data_type(),
                ArrowDataType::UInt64 | ArrowDataType::Timestamp(TimeUnit::Nanosecond, _)
            )
        })
    })
}

/// Attaches the planned custom `type_name` to a preflight schema that lacks it,
/// mirroring the execution-time injection so fingerprints match written output.
fn with_target_custom_type_name(candidate: &SchemaCandidate, schema: &Schema) -> Schema {
    let Some(type_name) = candidate.target_type_name.strip_prefix("custom/") else {
        return schema.clone();
    };

    inject_type_name_metadata(schema, type_name)
}

/// Attaches a custom `type_name` to a schema that lacks it, leaving other
/// schemas untouched.
fn inject_type_name_metadata(schema: &Schema, type_name: &str) -> Schema {
    let mut schema = schema.clone();
    schema
        .metadata
        .entry("type_name".to_string())
        .or_insert_with(|| type_name.to_string());
    schema
}

fn contains_legacy_fixed_binary(data_type: &ArrowDataType) -> bool {
    match data_type {
        ArrowDataType::FixedSizeBinary(_) => true,
        ArrowDataType::List(field)
        | ArrowDataType::LargeList(field)
        | ArrowDataType::FixedSizeList(field, _)
        | ArrowDataType::Map(field, _) => contains_legacy_fixed_binary(field.data_type()),
        ArrowDataType::Struct(fields) => fields
            .iter()
            .any(|field| contains_legacy_fixed_binary(field.data_type())),
        ArrowDataType::Dictionary(_, value) => contains_legacy_fixed_binary(value),
        _ => false,
    }
}

pub(crate) fn ensure_planned_file_unchanged(
    source: &dyn ParquetCatalogSource,
    file: &PlannedMigrationFile,
    object_path: &ObjectPath,
) -> anyhow::Result<()> {
    let object = execute_async(|| async { Ok(source.object_store().head(object_path).await?) })?;
    anyhow::ensure!(
        object.size == file.size
            && object.e_tag == file.e_tag
            && object.version == file.version
            && object.last_modified.to_rfc3339() == file.last_modified,
        "Source file changed after migration preflight: {}",
        file.relative_path,
    );
    Ok(())
}

fn schema_conflicts(files: &[PlannedMigrationFile]) -> Vec<SchemaConflict> {
    let mut by_table: BTreeMap<String, BTreeMap<String, SchemaConflictExample>> = BTreeMap::new();
    for file in files.iter().filter(|file| file.size != 0) {
        by_table
            .entry(file.target_table.clone())
            .or_default()
            .entry(file.target_fingerprint.to_string())
            .or_insert_with(|| SchemaConflictExample {
                fingerprint: file.target_fingerprint.clone(),
                path: file.relative_path.clone(),
            });
    }

    by_table
        .into_iter()
        .filter_map(|(target_table, examples)| {
            (examples.len() > 1).then(|| SchemaConflict {
                target_table,
                examples: examples.into_values().collect(),
            })
        })
        .collect()
}

fn split_batch_by_identifier(
    file: &PlannedMigrationFile,
    batch: RecordBatch,
) -> anyhow::Result<Vec<(Option<String>, IdentifierSource, RecordBatch)>> {
    if let Some(identifier) = metadata_identifier(&file.target_type_name, &batch) {
        return Ok(vec![(Some(identifier), IdentifierSource::Metadata, batch)]);
    }

    if let Some(column) = identifier_column(&file.target_type_name, &batch)? {
        let groups = row_identifier_groups(&column, batch.num_rows())?;
        return groups
            .into_iter()
            .map(|(identifier, indices)| {
                let batch = take_record_batch(&batch, &UInt32Array::from(indices))?;
                Ok((identifier, IdentifierSource::Row, batch))
            })
            .collect();
    }

    if let Some(identifier) =
        record_identifier_from_path(&file.relative_path, &file.source_type_name)
    {
        return Ok(vec![(Some(identifier), IdentifierSource::Path, batch)]);
    }

    Ok(vec![(None, IdentifierSource::Absent, batch)])
}

fn metadata_identifier(type_name: &str, batch: &RecordBatch) -> Option<String> {
    batch
        .schema()
        .metadata()
        .get(identifier_metadata_key(type_name))
        .cloned()
}

fn identifier_metadata_key(type_name: &str) -> &'static str {
    if type_name == "bars" {
        KEY_BAR_TYPE
    } else {
        KEY_INSTRUMENT_ID
    }
}

fn identifier_column<'a>(
    type_name: &str,
    batch: &'a RecordBatch,
) -> anyhow::Result<Option<StringColumnRef<'a>>> {
    let candidates = if type_name == "bars" {
        [KEY_IDENTIFIER, KEY_BAR_TYPE, KEY_INSTRUMENT_ID, "id"]
    } else {
        [KEY_IDENTIFIER, KEY_INSTRUMENT_ID, KEY_BAR_TYPE, "id"]
    };

    for name in candidates {
        if let Some(column) = batch.column_by_name(name) {
            return StringColumnRef::try_from_array(column.as_ref())
                .map(Some)
                .ok_or_else(|| anyhow::anyhow!("Identifier column {name} is not string-like"));
        }
    }

    Ok(None)
}

fn row_identifier_groups(
    column: &StringColumnRef<'_>,
    row_count: usize,
) -> anyhow::Result<BTreeMap<Option<String>, Vec<u32>>> {
    let mut groups: BTreeMap<Option<String>, Vec<u32>> = BTreeMap::new();
    for row in 0..row_count {
        groups
            .entry(column.value_opt(row).map(ToString::to_string))
            .or_default()
            .push(u32::try_from(row)?);
    }

    Ok(groups)
}

fn batch_with_identifier(
    type_name: &str,
    identifier: Option<&str>,
    batch: RecordBatch,
) -> anyhow::Result<RecordBatch> {
    let batch = record_batch_with_identifier_column(batch, identifier)?;

    let Some(identifier) = identifier else {
        return Ok(batch);
    };

    if type_name.starts_with("custom/") {
        return Ok(batch);
    }

    let mut schema = batch.schema().as_ref().clone();
    schema.metadata.insert(
        identifier_metadata_key(type_name).to_string(),
        identifier.to_string(),
    );
    Ok(RecordBatch::try_new(
        Arc::new(schema),
        batch.columns().to_vec(),
    )?)
}

// Built-in batches of one output file take the current encoder's exact schema and metadata, so
// migrated files consolidate with later catalog writes
fn round_trip_migrated_batches(
    type_name: &str,
    batches: Vec<RecordBatch>,
) -> anyhow::Result<Vec<RecordBatch>> {
    if let Ok(record_type) = type_name.parse::<NautilusRecordType>() {
        return round_trip_catalog_record_batches(record_type, batches);
    }

    let data_type = data_type_from_data_path_prefix(type_name)?;

    macro_rules! round_trip_data_type {
        (
            ($data_type:ident, $batches:ident);
            (Instrument, InstrumentAny, Instrument, Instrument, $instrument_prefix:literal),
            $(($variant:ident, $type:ident, $data:ident, $batch_variant:ident, $prefix:literal)),+ $(,)?
        ) => {
            match $data_type {
                NautilusDataType::Instrument => {
                    let mut instruments = Vec::new();

                    for batch in &$batches {
                        let metadata = batch.schema().metadata().clone();
                        instruments.extend(decode_instrument_any_batch(&metadata, batch)?);
                    }

                    let metadata = InstrumentAny::chunk_metadata(&instruments);
                    Ok(vec![InstrumentAny::encode_batch(&metadata, &instruments)?])
                }
                $(
                    NautilusDataType::$variant => {
                        let batches = $batches
                            .iter()
                            .map(project_current_columns::<$type>)
                            .collect::<anyhow::Result<Vec<_>>>()?;
                        round_trip_batches::<$type>(batches)
                    }
                )+
                NautilusDataType::Custom { .. } => Ok($batches),
                #[cfg(feature = "defi")]
                NautilusDataType::Defi => {
                    anyhow::bail!("Parquet migration does not support DeFi data")
                }
                #[cfg(not(feature = "defi"))]
                #[allow(
                    unreachable_patterns,
                    reason = "DeFi variants can exist without this crate's defi feature"
                )]
                _ => anyhow::bail!("Parquet migration does not support DeFi data"),
            }
        };
    }

    nautilus_model::for_each_data_type!(round_trip_data_type, data_type, batches)
}

// Positional decoders read the current column order, while sources may order columns differently
// or carry identity columns that current schemas keep in metadata. Current nullability applies so
// a null in a required column fails here instead of decoding as a default value.
fn project_current_columns<T: ArrowSchemaProvider>(
    batch: &RecordBatch,
) -> anyhow::Result<RecordBatch> {
    let schema = batch.schema();
    let mut fields = Vec::new();
    let mut columns = Vec::new();

    for expected in T::get_schema(None).fields() {
        if expected.name() == KEY_IDENTIFIER {
            continue;
        }

        let index = schema.index_of(expected.name())?;
        fields.push(
            schema
                .field(index)
                .clone()
                .with_nullable(expected.is_nullable()),
        );
        columns.push(batch.column(index).clone());
    }

    Ok(RecordBatch::try_new(
        Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone())),
        columns,
    )?)
}

fn record_identifier_from_path(file_path: &str, type_name: &str) -> Option<String> {
    let path_parts = file_path.split('/').collect::<Vec<_>>();
    let type_parts = type_name.split('/').collect::<Vec<_>>();

    for start in 0..path_parts.len() {
        if path_parts.get(start) != Some(&"data") {
            continue;
        }

        let type_start = start + 1;
        let type_end = type_start + type_parts.len();
        if path_parts.get(type_start..type_end) != Some(type_parts.as_slice()) {
            continue;
        }

        let remaining = &path_parts[type_end..];
        if remaining.len() <= 1 {
            return None;
        }

        return Some(remaining[0].to_string());
    }

    None
}

fn record_batches_with_schema(
    batches: Vec<RecordBatch>,
    schema: &Arc<Schema>,
) -> anyhow::Result<Vec<RecordBatch>> {
    batches
        .into_iter()
        .map(|batch| {
            RecordBatch::try_new(schema.clone(), batch.columns().to_vec()).map_err(Into::into)
        })
        .collect()
}

/// Attaches the planned custom `type_name` to batches that lack it.
///
/// Legacy Python-written custom files predate `type_name` schema metadata. Timestamp
/// normalization keys off that metadata, so without it `uint64` timestamps would pass
/// through unconverted. Files that already carry `type_name` are returned unchanged.
fn with_inferred_custom_type_name(
    file: &PlannedMigrationFile,
    batch: RecordBatch,
) -> anyhow::Result<RecordBatch> {
    let Some(type_name) = file.target_type_name.strip_prefix("custom/") else {
        return Ok(batch);
    };

    let schema = Arc::new(inject_type_name_metadata(&batch.schema(), type_name));
    Ok(RecordBatch::try_new(schema, batch.columns().to_vec())?)
}

/// Normalizes supported legacy Parquet physical encodings for explicit migration.
///
/// Older Python/v1 catalog writers can emit low-cardinality strings as Arrow dictionary
/// arrays and instrument `info` values as the JSON bytes `null` rather than Arrow nulls.
fn normalize_legacy_parquet_columns(batch: &RecordBatch) -> anyhow::Result<RecordBatch> {
    if let Some(schema) = normalize_legacy_record_schema(batch.schema_ref()) {
        let batch = normalize_legacy_info_column(batch)?;
        let batch = normalize_legacy_fixed_columns(&batch)?;
        return Ok(nautilus_serialization::arrow::record_batch_with_timestamps(
            Arc::new(schema),
            batch.columns().to_vec(),
        )?);
    }

    if is_legacy_instrument_schema(batch.schema_ref()) {
        let metadata = legacy_instrument_metadata(batch.schema().metadata());
        if batch.num_rows() == 0 {
            return Ok(RecordBatch::new_empty(Arc::new(InstrumentAny::get_schema(
                Some(metadata),
            ))));
        }

        let batch = normalize_legacy_info_column(batch)?;
        let instruments = decode_instrument_any_batch(&metadata, &batch)?;
        return Ok(InstrumentAny::encode_batch(&metadata, &instruments)?);
    }

    let normalize_legacy = is_nautilus_legacy_schema(batch.schema_ref());

    let batch = if normalize_legacy {
        normalize_dictionary_string_columns(batch)?
    } else {
        batch.clone()
    };

    let batch = normalize_legacy_fixed_columns(&batch)?;
    let batch = normalize_legacy_info_column(&batch)?;
    normalize_legacy_depth_columns(&batch)
}

/// Normalizes the Arrow schema changes made by [`normalize_legacy_parquet_columns`].
#[must_use]
fn normalize_legacy_parquet_schema(schema: &Schema) -> Schema {
    if let Some(schema) = normalize_legacy_record_schema(schema) {
        return schema;
    }

    if is_legacy_instrument_schema(schema) {
        return InstrumentAny::get_schema(Some(legacy_instrument_metadata(schema.metadata())));
    }

    let normalize_fixed = is_nautilus_legacy_schema(schema);
    let normalize_timestamps = is_nautilus_timestamp_schema(schema);

    let fields = schema
        .fields()
        .iter()
        .map(|field| {
            let data_type =
                normalized_legacy_data_type(field.name(), field.data_type(), normalize_fixed);

            let data_type = if schema.metadata().contains_key("type_name")
                && matches!(field.name().as_str(), "ts_event" | "ts_init")
                && data_type == ArrowDataType::UInt64
            {
                nautilus_serialization::arrow::timestamp_data_type()
            } else if normalize_timestamps {
                normalized_timestamp_type(&data_type)
            } else {
                data_type
            };

            let nullable = field.is_nullable()
                || (normalize_fixed
                    && matches!(field.data_type(), ArrowDataType::FixedSizeBinary(8 | 16)))
                || (field.name() == "info" && field.data_type() == &ArrowDataType::Binary);

            Arc::new(
                field
                    .as_ref()
                    .clone()
                    .with_data_type(data_type)
                    .with_nullable(nullable),
            )
        })
        .collect::<Vec<_>>();

    normalize_legacy_depth_schema(Schema::new_with_metadata(fields, schema.metadata().clone()))
}

fn normalize_legacy_record_schema(schema: &Schema) -> Option<Schema> {
    let record_type = schema
        .metadata()
        .get("type")?
        .parse::<NautilusRecordType>()
        .ok()?;
    let current = catalog_record_schema(record_type).ok()?;

    let fields = schema
        .fields()
        .iter()
        .map(|field| {
            let Ok(expected) = current.field_with_name(field.name()) else {
                return field.clone();
            };

            if field.data_type() == expected.data_type()
                || (expected.data_type() == &nautilus_serialization::arrow::timestamp_data_type()
                    && (field.data_type() == &ArrowDataType::UInt64
                        || normalized_timestamp_type(field.data_type()) == *expected.data_type()))
                || (field.name() == "info" && field.data_type() == &ArrowDataType::Binary)
            {
                Arc::new(expected.clone())
            } else {
                field.clone()
            }
        })
        .collect::<Vec<_>>();

    Some(Schema::new_with_metadata(fields, schema.metadata().clone()))
}

fn is_legacy_instrument_schema(schema: &Schema) -> bool {
    schema
        .metadata()
        .get(LEGACY_KEY_CLASS)
        .is_some_and(|type_name| type_name.parse::<NautilusInstrumentType>().is_ok())
        && schema.field_with_name("ts_init").is_ok_and(|field| {
            field.data_type() == &ArrowDataType::UInt64
                || field.data_type() == &nautilus_serialization::arrow::timestamp_data_type()
        })
}

// Legacy catalogs name the instrument type under `class`; current schemas use `type_name`
fn legacy_instrument_metadata(metadata: &HashMap<String, String>) -> HashMap<String, String> {
    let mut metadata = metadata.clone();
    if let Some(instrument_type) = metadata.remove(LEGACY_KEY_CLASS) {
        metadata.insert(KEY_TYPE_NAME.to_string(), instrument_type);
    }

    metadata
}

/// Casts dictionary-encoded string columns from legacy Parquet files to plain UTF-8 columns.
///
/// Older Python/v1 catalog writers can emit low-cardinality strings as Arrow dictionary
/// arrays. Rust decoders generally expect concrete `Utf8` columns, so Parquet reads normalize
/// this physical encoding before typed decoding.
fn normalize_dictionary_string_columns(batch: &RecordBatch) -> anyhow::Result<RecordBatch> {
    let schema = batch.schema();
    let mut changed = false;
    let mut fields = Vec::with_capacity(schema.fields().len());
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(batch.num_columns());

    for (field, column) in schema.fields().iter().zip(batch.columns()) {
        let data_type = normalized_dictionary_data_type(field.data_type());

        changed |= &data_type != field.data_type();
        fields.push(Arc::new(
            field.as_ref().clone().with_data_type(data_type.clone()),
        ));

        if column.data_type() == &data_type {
            columns.push(column.clone());
        } else {
            columns.push(cast(column.as_ref(), &data_type)?);
        }
    }

    if !changed {
        return Ok(batch.clone());
    }

    let schema = Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone()));
    Ok(RecordBatch::try_new(schema, columns)?)
}

fn normalized_legacy_data_type(
    name: &str,
    data_type: &ArrowDataType,
    normalize_fixed: bool,
) -> ArrowDataType {
    if normalize_fixed
        && let ArrowDataType::FixedSizeList(item, length) = data_type
        && matches!(item.data_type(), ArrowDataType::FixedSizeBinary(8 | 16))
    {
        return ArrowDataType::FixedSizeList(
            Arc::new(
                item.as_ref()
                    .clone()
                    .with_data_type(normalize_legacy_arrow_data_type(name, item.data_type()))
                    .with_nullable(true),
            ),
            *length,
        );
    }

    if normalize_fixed {
        let normalized = normalize_legacy_arrow_data_type(name, data_type);
        if &normalized != data_type {
            return normalized;
        }
    }

    match data_type {
        ArrowDataType::Binary if name == "info" => ArrowDataType::Utf8,
        _ if normalize_fixed => normalized_dictionary_data_type(data_type),
        _ => data_type.clone(),
    }
}

fn normalized_dictionary_data_type(data_type: &ArrowDataType) -> ArrowDataType {
    match data_type {
        ArrowDataType::Dictionary(_, value_type)
            if matches!(value_type.as_ref(), ArrowDataType::Utf8) =>
        {
            ArrowDataType::Utf8
        }
        _ => data_type.clone(),
    }
}

fn normalize_legacy_info_column(batch: &RecordBatch) -> anyhow::Result<RecordBatch> {
    let Some(info_index) = batch.schema().index_of("info").ok() else {
        return Ok(batch.clone());
    };

    let column = batch.column(info_index);
    if column.data_type() != &ArrowDataType::Binary {
        return Ok(batch.clone());
    }

    let info = column
        .as_any()
        .downcast_ref::<BinaryArray>()
        .expect("Binary column should downcast to BinaryArray");

    let mut builder = StringBuilder::new();

    for row in 0..info.len() {
        if info.is_null(row) || info.value(row) == b"null" {
            builder.append_null();
        } else {
            builder.append_value(std::str::from_utf8(info.value(row))?);
        }
    }

    let mut fields = batch.schema().fields().iter().cloned().collect::<Vec<_>>();

    fields[info_index] = Arc::new(
        fields[info_index]
            .as_ref()
            .clone()
            .with_data_type(ArrowDataType::Utf8)
            .with_nullable(true),
    );
    let mut columns = batch.columns().to_vec();
    columns[info_index] = Arc::new(builder.finish());

    let schema = Arc::new(Schema::new_with_metadata(
        fields,
        batch.schema().metadata().clone(),
    ));
    Ok(RecordBatch::try_new(schema, columns)?)
}

fn normalize_legacy_depth_schema(schema: Schema) -> Schema {
    if !has_legacy_depth_columns(&schema) {
        return schema;
    }

    let mut fields = vec![depth_side_field("bids"), depth_side_field("asks")];
    fields.extend(
        schema
            .fields()
            .iter()
            .filter(|field| !is_legacy_depth_column(field.name()))
            .cloned(),
    );
    Schema::new_with_metadata(fields, schema.metadata().clone())
}

fn normalize_legacy_depth_columns(batch: &RecordBatch) -> anyhow::Result<RecordBatch> {
    if !has_legacy_depth_columns(batch.schema().as_ref()) {
        return Ok(batch.clone());
    }

    let flat = batch.schema().index_of("bid_price_0").is_ok();

    let side_values = |side: &str, value: &str| {
        let name = format!("{side}_{value}");

        if flat {
            return match value {
                "price" | "size" => decimal_depth_list(batch, &name),
                "count" => u32_depth_list(batch, &name),
                "order_id" => u64_depth_list(batch, &name),
                _ => unreachable!("depth field inventory is fixed"),
            }
            .and_then(|list| depth_list_values(&list));
        }

        if let Some(column) = batch.column_by_name(&name) {
            return depth_list_values(column);
        }

        anyhow::ensure!(
            matches!(value, "count" | "order_id"),
            "Missing legacy depth column '{name}'"
        );
        let width = legacy_fixed_list_width(batch, side)?;

        let len = batch
            .num_rows()
            .checked_mul(width)
            .ok_or_else(|| anyhow::anyhow!("Legacy depth column '{name}' length overflow"))?;

        match value {
            "count" => Ok(Arc::new(UInt32Array::from(vec![0; len])) as ArrayRef),
            "order_id" => Ok(Arc::new(UInt64Array::from(vec![0; len])) as ArrayRef),
            _ => unreachable!("missing legacy depth defaults are fixed"),
        }
    };

    let mut columns = vec![
        depth_side_array(
            &side_values("bid", "price")?,
            &side_values("bid", "size")?,
            &side_values("bid", "count")?,
            &side_values("bid", "order_id")?,
            batch.num_rows(),
        )?,
        depth_side_array(
            &side_values("ask", "price")?,
            &side_values("ask", "size")?,
            &side_values("ask", "count")?,
            &side_values("ask", "order_id")?,
            batch.num_rows(),
        )?,
    ];
    columns.extend(
        batch
            .schema()
            .fields()
            .iter()
            .zip(batch.columns())
            .filter(|(field, _)| !is_legacy_depth_column(field.name()))
            .map(|(_, column)| column.clone()),
    );

    let schema = Arc::new(normalize_legacy_depth_schema(
        batch.schema().as_ref().clone(),
    ));
    Ok(RecordBatch::try_new(schema, columns)?)
}

fn legacy_fixed_list_width(batch: &RecordBatch, side: &str) -> anyhow::Result<usize> {
    let schema = batch.schema();
    ["price", "size", "count", "order_id"]
        .iter()
        .filter_map(|value| schema.field_with_name(&format!("{side}_{value}")).ok())
        .find_map(|field| match field.data_type() {
            ArrowDataType::FixedSizeList(_, width) => usize::try_from(*width).ok(),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("Missing legacy depth FixedSizeList width for '{side}'"))
}

fn has_legacy_depth_columns(schema: &Schema) -> bool {
    if schema.index_of("bids").is_ok() {
        return false;
    }

    let has_fixed_lists = ["bid_price", "ask_price", "bid_size", "ask_size"]
        .iter()
        .all(|name| {
            schema
                .field_with_name(name)
                .is_ok_and(|field| matches!(field.data_type(), ArrowDataType::FixedSizeList(_, _)))
        });

    let has_flat_levels = ["bid_price_0", "ask_price_0", "bid_size_0", "ask_size_0"]
        .iter()
        .all(|name| schema.index_of(name).is_ok());

    has_fixed_lists || has_flat_levels
}

fn depth_level_fields() -> Fields {
    vec![
        Field::new("price", ArrowDataType::Decimal128(38, 16), false),
        Field::new("size", ArrowDataType::Decimal128(38, 16), false),
        Field::new("count", ArrowDataType::UInt32, false),
        Field::new("order_id", ArrowDataType::UInt64, false),
    ]
    .into()
}

fn depth_side_field(name: &str) -> Arc<Field> {
    let fields = depth_level_fields();

    Arc::new(Field::new(
        name,
        ArrowDataType::List(Arc::new(Field::new(
            "item",
            ArrowDataType::Struct(fields),
            false,
        ))),
        false,
    ))
}

fn decimal_depth_list(batch: &RecordBatch, prefix: &str) -> anyhow::Result<ArrayRef> {
    let arrays = (0..DEPTH10_LEN)
        .map(|level| {
            let name = format!("{prefix}_{level}");
            batch
                .column_by_name(&name)
                .and_then(|column| column.as_any().downcast_ref::<Decimal128Array>())
                .ok_or_else(|| anyhow::anyhow!("Legacy depth column '{name}' must be Decimal128"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut values = Vec::with_capacity(batch.num_rows() * DEPTH10_LEN);
    for row in 0..batch.num_rows() {
        for array in &arrays {
            values.push((!array.is_null(row)).then(|| array.value(row)));
        }
    }

    let values = Decimal128Array::from(values).with_precision_and_scale(38, 16)?;
    Ok(depth_list_array(Arc::new(values), true))
}

fn u64_depth_list(batch: &RecordBatch, prefix: &str) -> anyhow::Result<ArrayRef> {
    let arrays = (0..DEPTH10_LEN)
        .map(|level| {
            let name = format!("{prefix}_{level}");
            batch
                .column_by_name(&name)
                .map(|column| {
                    column
                        .as_any()
                        .downcast_ref::<UInt64Array>()
                        .ok_or_else(|| {
                            anyhow::anyhow!("Legacy depth column '{name}' must be UInt64")
                        })
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut values = Vec::with_capacity(batch.num_rows() * DEPTH10_LEN);
    for row in 0..batch.num_rows() {
        for array in &arrays {
            values.push(array.map_or(0, |array| array.value(row)));
        }
    }

    Ok(depth_list_array(Arc::new(UInt64Array::from(values)), false))
}

fn u32_depth_list(batch: &RecordBatch, prefix: &str) -> anyhow::Result<ArrayRef> {
    let arrays = (0..DEPTH10_LEN)
        .map(|level| {
            let name = format!("{prefix}_{level}");
            batch
                .column_by_name(&name)
                .and_then(|column| column.as_any().downcast_ref::<UInt32Array>())
                .ok_or_else(|| anyhow::anyhow!("Legacy depth column '{name}' must be UInt32"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut values = Vec::with_capacity(batch.num_rows() * DEPTH10_LEN);
    for row in 0..batch.num_rows() {
        for array in &arrays {
            values.push(array.value(row));
        }
    }

    Ok(depth_list_array(Arc::new(UInt32Array::from(values)), false))
}

fn depth_list_array(values: ArrayRef, values_nullable: bool) -> ArrayRef {
    Arc::new(FixedSizeListArray::new(
        Arc::new(Field::new(
            "item",
            values.data_type().clone(),
            values_nullable,
        )),
        i32::try_from(DEPTH10_LEN).expect("depth-10 length fits i32"),
        values,
        None,
    ))
}

fn depth_list_values(list: &ArrayRef) -> anyhow::Result<ArrayRef> {
    list.as_any()
        .downcast_ref::<FixedSizeListArray>()
        .map(|list| list.values().clone())
        .ok_or_else(|| anyhow::anyhow!("Legacy depth column must be FixedSizeList"))
}

fn depth_side_array(
    prices: &ArrayRef,
    sizes: &ArrayRef,
    counts: &ArrayRef,
    order_ids: &ArrayRef,
    rows: usize,
) -> anyhow::Result<ArrayRef> {
    let prices = prices
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .ok_or_else(|| anyhow::anyhow!("Legacy depth prices must be Decimal128"))?;
    let sizes = sizes
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .ok_or_else(|| anyhow::anyhow!("Legacy depth sizes must be Decimal128"))?;
    let counts = counts
        .as_any()
        .downcast_ref::<UInt32Array>()
        .ok_or_else(|| anyhow::anyhow!("Legacy depth counts must be UInt32"))?;
    let order_ids = order_ids
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or_else(|| anyhow::anyhow!("Legacy depth order IDs must be UInt64"))?;
    let expected_len = prices.len();
    anyhow::ensure!(
        sizes.len() == expected_len
            && counts.len() == expected_len
            && order_ids.len() == expected_len,
        "Legacy depth columns must contain the same number of values"
    );

    let width = if rows == 0 {
        0
    } else {
        anyhow::ensure!(
            expected_len.is_multiple_of(rows),
            "Legacy depth column length {expected_len} is not divisible by row count {rows}"
        );
        expected_len / rows
    };

    let mut open_prices = Vec::with_capacity(expected_len);
    let mut open_sizes = Vec::with_capacity(expected_len);
    let mut open_counts = Vec::with_capacity(expected_len);
    let mut open_order_ids = Vec::with_capacity(expected_len);
    let mut offsets = Vec::with_capacity(rows + 1);
    offsets.push(0);

    for row in 0..rows {
        for level in 0..width {
            let index = row * width + level;
            if prices.is_null(index) || sizes.is_null(index) {
                continue;
            }

            open_prices.push(prices.value(index));
            open_sizes.push(sizes.value(index));
            open_counts.push(counts.value(index));
            open_order_ids.push(order_ids.value(index));
        }

        offsets.push(i32::try_from(open_prices.len())?);
    }

    let fields = depth_level_fields();
    let values = StructArray::try_new(
        fields.clone(),
        vec![
            Arc::new(Decimal128Array::from(open_prices).with_precision_and_scale(38, 16)?),
            Arc::new(Decimal128Array::from(open_sizes).with_precision_and_scale(38, 16)?),
            Arc::new(UInt32Array::from(open_counts)),
            Arc::new(UInt64Array::from(open_order_ids)),
        ],
        None,
    )?;
    Ok(Arc::new(ListArray::try_new(
        Arc::new(Field::new("item", ArrowDataType::Struct(fields), false)),
        OffsetBuffer::new(ScalarBuffer::from(offsets)),
        Arc::new(values),
        None,
    )?))
}

fn is_legacy_depth_column(name: &str) -> bool {
    const LIST_COLUMNS: &[&str] = &[
        "bid_price",
        "ask_price",
        "bid_size",
        "ask_size",
        "bid_order_id",
        "ask_order_id",
        "bid_count",
        "ask_count",
    ];
    LIST_COLUMNS.iter().any(|column| {
        name.strip_prefix(column).is_some_and(|suffix| {
            suffix.is_empty()
                || suffix
                    .strip_prefix('_')
                    .and_then(|level| level.parse::<usize>().ok())
                    .is_some_and(|level| level < DEPTH10_LEN)
        })
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs, sync::Arc};

    use ::parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use arrow::{
        array::{
            BooleanArray, FixedSizeBinaryArray, Float64Array, Int64Array, StringArray,
            StringDictionaryBuilder, TimestampNanosecondArray, UInt8Array, UInt64Array,
        },
        datatypes::{DataType, Field, Int8Type, Schema},
        record_batch::RecordBatch,
    };
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{greeks::OptionGreekValues, stubs::stub_depth10},
        enums::{GreeksConvention, MarketStatusAction},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use nautilus_serialization::arrow::{
        DecodeFromRecordBatch, KEY_PRICE_PRECISION, KEY_TYPE_NAME,
        record_batch_without_identifier_column, timestamp_array, timestamp_data_type,
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    fn storage_options_require_non_empty_key_and_value() {
        assert_eq!(
            parse_storage_option("region=us-east-1"),
            Ok(("region".to_string(), "us-east-1".to_string()))
        );
        assert_eq!(
            parse_storage_option("region").unwrap_err(),
            "Storage option must use key=value: region"
        );
        assert_eq!(
            parse_storage_option("=us-east-1").unwrap_err(),
            "Storage option must use non-empty key=value: =us-east-1"
        );
        assert_eq!(
            parse_storage_option("region=").unwrap_err(),
            "Storage option must use non-empty key=value: region="
        );
    }

    #[rstest]
    fn dry_run_report_lists_unmigrated_files_and_reasons() {
        let plan = CatalogMigrationPlan {
            total_leaf_files: 1,
            unmigrated: vec![UnmigratedFile {
                path: "backtest/run/data.feather".to_string(),
                reason: "Feather trees are outside catalog migration scope".to_string(),
            }],
            ..CatalogMigrationPlan::default()
        };

        let report = CatalogMigrationReport::from_plan(&plan, true).to_string();

        assert!(report.contains("Migration dry-run report: 1 planned leaf files"));
        assert!(report.contains("Unmigrated directory backtest/run: 1 files"));
        assert!(report.contains(
            "Unmigrated backtest/run/data.feather: Feather trees are outside catalog migration scope"
        ));
    }

    #[rstest]
    #[case("/tmp/source", "/tmp/source")]
    #[case("/tmp/source/", "/tmp/source")]
    #[case("/tmp/source", "/tmp/source/target")]
    #[case("/tmp/source/child", "/tmp/source")]
    fn migration_locations_must_not_overlap(#[case] source: &str, #[case] target: &str) {
        assert_eq!(
            ensure_distinct_migration_locations(source, target)
                .unwrap_err()
                .to_string(),
            "Migration source and target must be distinct, non-overlapping locations"
        );
    }

    #[rstest]
    fn migration_locations_normalize_local_paths() {
        let source = std::env::current_dir().unwrap().join("catalog");
        let source_uri = normalize_storage_location(source.to_str().unwrap()).unwrap();

        assert!(ensure_distinct_migration_locations("catalog", &source_uri).is_err());
        assert!(ensure_distinct_migration_locations("catalog-a", "catalog-b").is_ok());
    }

    #[rstest]
    #[case("depths.parquet", "order_book_depths")]
    #[case("quotes.parquet", "quotes")]
    #[case("trades.parquet", "trades")]
    #[case("bars.parquet", "bars")]
    #[case("deltas.parquet", "order_book_deltas")]
    fn committed_legacy_fixture_plans_as_pass_through(
        #[case] file_name: &str,
        #[case] type_name: &str,
    ) {
        let precision_dir = if cfg!(feature = "high-precision") {
            "128-bit"
        } else {
            "64-bit"
        };

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data/nautilus/legacy")
            .join(precision_dir)
            .join(file_name);
        let file = std::fs::File::open(path).unwrap();
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
        let schema = normalize_legacy_parquet_schema(builder.schema().as_ref());

        let resolution = resolve_legacy_schema(type_name, file_name, &schema).unwrap();

        assert_eq!(resolution.kind, LegacyTranscodeKind::PassThrough);
    }

    #[rstest]
    fn classify_source_path_maps_legacy_depth_directory() {
        let classification =
            classify_source_path("data/order_book_depth10/AAPL.XNAS/part-0.parquet");

        assert!(matches!(
            classification,
            SourceClassification::Migratable { source_type_name, target_type_name }
                if source_type_name == "order_book_depth10"
                    && target_type_name == "order_book_depths"
        ));
    }

    #[rstest]
    fn migration_plan_discovers_bar_file_under_bare_instrument_folder() {
        let temp = TempDir::new().unwrap();
        let source_path = temp.path().join("source");
        let bar_dir = source_path.join("data").join("bars").join("AUDUSD.SIM");
        fs::create_dir_all(&bar_dir).unwrap();

        let precision_dir = if cfg!(feature = "high-precision") {
            "128-bit"
        } else {
            "64-bit"
        };

        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data/nautilus/legacy")
            .join(precision_dir)
            .join("bars.parquet");
        fs::copy(fixture, bar_dir.join("bars.parquet")).unwrap();
        let source = crate::backend::parquet::catalog::ParquetDataCatalog::from_uri(
            source_path.to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let plan = build_catalog_migration_plan(&source).unwrap();

        assert_eq!(plan.total_leaf_files, 1);
        assert_eq!(plan.files.len(), 1);
        assert_eq!(
            plan.files[0].relative_path,
            "data/bars/AUDUSD.SIM/bars.parquet",
        );
        assert_eq!(plan.files[0].source_type_name, "bars");
        assert_eq!(plan.files[0].target_type_name, "bars");
        assert!(plan.unmigrated.is_empty());
        assert!(plan.unresolved_schemas.is_empty());
        plan.ensure_ready().unwrap();
    }

    #[rstest]
    fn migration_plan_rejects_unrecognized_schema_during_preflight() {
        let temp = TempDir::new().unwrap();
        let source_path = temp.path().join("source");
        let quote_dir = source_path.join("data").join("quotes").join("AUDUSD.SIM");
        fs::create_dir_all(&quote_dir).unwrap();

        let schema = Arc::new(Schema::new(vec![Field::new(
            "unknown",
            DataType::Int64,
            false,
        )]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Int64Array::from(vec![1_i64]))],
        )
        .unwrap();
        let file = fs::File::create(quote_dir.join("unknown.parquet")).unwrap();
        let mut writer = ::parquet::arrow::ArrowWriter::try_new(file, schema, None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
        let source = crate::backend::parquet::catalog::ParquetDataCatalog::from_uri(
            source_path.to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let plan = build_catalog_migration_plan(&source).unwrap();
        let error = plan.ensure_ready().unwrap_err();

        assert_eq!(plan.total_leaf_files, 1);
        assert!(plan.files.is_empty());
        assert_eq!(plan.unresolved_schemas.len(), 1);
        assert_eq!(
            plan.unresolved_schemas[0].path,
            "data/quotes/AUDUSD.SIM/unknown.parquet",
        );
        assert_eq!(
            error.to_string(),
            format!(
                "Catalog migration preflight failed:\n{}",
                plan.unresolved_schemas[0].message,
            ),
        );
    }

    #[rstest]
    #[case::bars(
        "bars",
        Some("AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"),
        Some("AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"),
        None
    )]
    #[case::quotes("quotes", Some("AUD/USD.SIM"), None, Some("AUD/USD.SIM"))]
    #[case::custom("custom/RustTestCustomData", Some("AUD/USD.SIM"), None, None)]
    #[case::no_identifier("quotes", None, None, None)]
    fn batch_with_identifier_writes_the_type_specific_metadata_key(
        #[case] type_name: &str,
        #[case] identifier: Option<&str>,
        #[case] expected_bar_type: Option<&str>,
        #[case] expected_instrument_id: Option<&str>,
    ) {
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "ts_init",
                DataType::Int64,
                false,
            )])),
            vec![Arc::new(Int64Array::from(vec![1]))],
        )
        .unwrap();

        let batch = batch_with_identifier(type_name, identifier, batch).unwrap();

        let schema = batch.schema();
        let metadata = schema.metadata();
        assert_eq!(
            metadata.get(KEY_BAR_TYPE).map(String::as_str),
            expected_bar_type
        );
        assert_eq!(
            metadata.get(KEY_INSTRUMENT_ID).map(String::as_str),
            expected_instrument_id
        );
        assert_eq!(
            metadata_identifier(type_name, &batch).as_deref(),
            expected_bar_type.or(expected_instrument_id)
        );
    }

    #[rstest]
    #[case::absent(None, "Planned")]
    #[case::present(Some("Existing"), "Existing")]
    fn inject_type_name_metadata_keeps_an_existing_type_name(
        #[case] existing: Option<&str>,
        #[case] expected: &str,
    ) {
        let metadata = existing
            .map(|name| HashMap::from([("type_name".to_string(), name.to_string())]))
            .unwrap_or_default();
        let schema = Schema::new_with_metadata(
            vec![Field::new("ts_init", DataType::Int64, false)],
            metadata,
        );

        let injected = inject_type_name_metadata(&schema, "Planned");

        assert_eq!(
            injected.metadata().get("type_name").map(String::as_str),
            Some(expected)
        );
        assert_eq!(injected.fields(), schema.fields());
    }

    #[rstest]
    fn prepare_migration_parts_splits_legacy_status_and_matches_fresh_writes() {
        let expected = [
            InstrumentStatus::new(
                InstrumentId::from("AAA.XNAS"),
                MarketStatusAction::Trading,
                UnixNanos::from(1),
                UnixNanos::from(2),
                Some("Normal trading".into()),
                None,
                Some(true),
                None,
                Some(false),
            ),
            InstrumentStatus::new(
                InstrumentId::from("BBB.XNAS"),
                MarketStatusAction::Halt,
                UnixNanos::from(3),
                UnixNanos::from(4),
                None,
                Some("MARKET_HALT".into()),
                Some(false),
                Some(false),
                None,
            ),
        ];

        // Legacy status files carried the instrument per row, with no identity metadata
        let source = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("instrument_id", DataType::Utf8, true),
                Field::new("action", DataType::Utf8, true),
                Field::new("reason", DataType::Utf8, true),
                Field::new("trading_event", DataType::Utf8, true),
                Field::new("is_trading", DataType::Boolean, true),
                Field::new("is_quoting", DataType::Boolean, true),
                Field::new("is_short_sell_restricted", DataType::Boolean, true),
                Field::new("ts_event", DataType::UInt64, true),
                Field::new("ts_init", DataType::UInt64, true),
            ])),
            vec![
                Arc::new(StringArray::from(vec!["AAA.XNAS", "BBB.XNAS"])),
                Arc::new(StringArray::from(vec!["TRADING", "HALT"])),
                Arc::new(StringArray::from(vec![Some("Normal trading"), None])),
                Arc::new(StringArray::from(vec![None, Some("MARKET_HALT")])),
                Arc::new(BooleanArray::from(vec![Some(true), Some(false)])),
                Arc::new(BooleanArray::from(vec![None, Some(false)])),
                Arc::new(BooleanArray::from(vec![Some(false), None])),
                Arc::new(UInt64Array::from(vec![1, 3])),
                Arc::new(UInt64Array::from(vec![2, 4])),
            ],
        )
        .unwrap();

        let parts = migrate_source_batches("instrument_status", &[source]);

        assert_eq!(
            parts
                .iter()
                .map(|part| (part.identifier.as_deref(), part.identifier_source))
                .collect::<Vec<_>>(),
            vec![
                (Some("AAA.XNAS"), IdentifierSource::Row),
                (Some("BBB.XNAS"), IdentifierSource::Row),
            ]
        );

        for (part, expected) in parts.into_iter().zip(expected) {
            assert_part_matches_fresh_write(part, &[expected]);
        }
    }

    #[rstest]
    fn prepare_migration_parts_reorders_legacy_greeks_and_matches_fresh_writes() {
        let instrument_id = InstrumentId::from("BTC-20260529-100000-C.OKX");

        let expected = OptionGreeks {
            instrument_id,
            convention: GreeksConvention::PriceAdjusted,
            greeks: OptionGreekValues {
                delta: 0.55,
                gamma: 0.012,
                vega: 3.4,
                theta: -1.2,
                rho: 0.01,
            },
            mark_iv: Some(0.64),
            bid_iv: None,
            ask_iv: Some(0.66),
            underlying_price: Some(100_000.0),
            open_interest: None,
            ts_event: UnixNanos::from(5),
            ts_init: UnixNanos::from(6),
        };

        // Legacy Greeks files led with an instrument column and ended with the convention
        let float_field = |name| Field::new(name, DataType::Float64, false);
        let optional_float_field = |name| Field::new(name, DataType::Float64, true);
        let source = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(
                vec![
                    Field::new("instrument_id", DataType::Utf8, false),
                    float_field("delta"),
                    float_field("gamma"),
                    float_field("vega"),
                    float_field("theta"),
                    float_field("rho"),
                    optional_float_field("mark_iv"),
                    optional_float_field("bid_iv"),
                    optional_float_field("ask_iv"),
                    optional_float_field("underlying_price"),
                    optional_float_field("open_interest"),
                    Field::new("ts_event", DataType::UInt64, false),
                    Field::new("ts_init", DataType::UInt64, false),
                    Field::new("convention", DataType::Utf8, false),
                ],
                HashMap::from([("type".to_string(), "OptionGreeks".to_string())]),
            )),
            vec![
                Arc::new(StringArray::from(vec![instrument_id.to_string()])),
                Arc::new(Float64Array::from(vec![0.55])),
                Arc::new(Float64Array::from(vec![0.012])),
                Arc::new(Float64Array::from(vec![3.4])),
                Arc::new(Float64Array::from(vec![-1.2])),
                Arc::new(Float64Array::from(vec![0.01])),
                Arc::new(Float64Array::from(vec![Some(0.64)])),
                Arc::new(Float64Array::from(vec![None])),
                Arc::new(Float64Array::from(vec![Some(0.66)])),
                Arc::new(Float64Array::from(vec![Some(100_000.0)])),
                Arc::new(Float64Array::from(vec![None])),
                Arc::new(UInt64Array::from(vec![5])),
                Arc::new(UInt64Array::from(vec![6])),
                Arc::new(StringArray::from(vec!["PRICE_ADJUSTED"])),
            ],
        )
        .unwrap();

        let parts = migrate_source_batches("option_greeks", &[source]);

        assert_eq!(parts.len(), 1);
        assert_eq!(
            parts[0].identifier.as_deref(),
            Some("BTC-20260529-100000-C.OKX")
        );
        assert_eq!(
            parts[0].batches[0].schema().metadata().get(KEY_TYPE_NAME),
            Some(&"OptionGreeks".to_string())
        );
        assert_part_matches_fresh_write(parts.into_iter().next().unwrap(), &[expected]);
    }

    #[rstest]
    fn prepare_migration_parts_rejects_null_in_required_column() {
        let source = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(
                vec![
                    Field::new("instrument_id", DataType::Utf8, false),
                    Field::new("convention", DataType::Utf8, false),
                    Field::new("delta", DataType::Float64, true),
                    Field::new("gamma", DataType::Float64, false),
                    Field::new("vega", DataType::Float64, false),
                    Field::new("theta", DataType::Float64, false),
                    Field::new("rho", DataType::Float64, false),
                    Field::new("mark_iv", DataType::Float64, true),
                    Field::new("bid_iv", DataType::Float64, true),
                    Field::new("ask_iv", DataType::Float64, true),
                    Field::new("underlying_price", DataType::Float64, true),
                    Field::new("open_interest", DataType::Float64, true),
                    Field::new("ts_event", timestamp_data_type(), false),
                    Field::new("ts_init", timestamp_data_type(), false),
                ],
                HashMap::from([("type".to_string(), "OptionGreeks".to_string())]),
            )),
            vec![
                Arc::new(StringArray::from(vec!["BTC-20260529-100000-C.OKX"])),
                Arc::new(StringArray::from(vec!["BLACK_SCHOLES"])),
                Arc::new(Float64Array::from(vec![None])),
                Arc::new(Float64Array::from(vec![0.012])),
                Arc::new(Float64Array::from(vec![3.4])),
                Arc::new(Float64Array::from(vec![-1.2])),
                Arc::new(Float64Array::from(vec![0.01])),
                Arc::new(Float64Array::from(vec![None])),
                Arc::new(Float64Array::from(vec![None])),
                Arc::new(Float64Array::from(vec![None])),
                Arc::new(Float64Array::from(vec![None])),
                Arc::new(Float64Array::from(vec![None])),
                Arc::new(timestamp_array([5]).unwrap()),
                Arc::new(timestamp_array([6]).unwrap()),
            ],
        )
        .unwrap();

        let error = try_migrate_source_batches("option_greeks", &[source]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Invalid argument error: Column 'delta' is declared as non-nullable but contains null values"
        );
    }

    #[rstest]
    fn prepare_migration_parts_selects_metadata_across_every_batch_of_a_file() {
        let populated = stub_depth10();
        let mut empty = populated.clone();
        empty.bids.clear();
        empty.asks.clear();
        empty.bid_counts.clear();
        empty.ask_counts.clear();
        let expected = [empty, populated];

        // A reader splits one file into batches that share the file's metadata, and a batch of
        // empty snapshots alone would select zero precision
        let file_metadata = OrderBookDepth::chunk_metadata(&expected);

        let sources = expected
            .iter()
            .map(|depth| {
                let batch =
                    OrderBookDepth::encode_batch(&file_metadata, std::slice::from_ref(depth))
                        .unwrap();
                record_batch_without_identifier_column(batch).unwrap()
            })
            .collect::<Vec<_>>();

        let parts = migrate_source_batches("order_book_depths", &sources);

        assert_eq!(parts.len(), 1);
        assert_eq!(
            parts[0]
                .batches
                .iter()
                .map(|batch| batch.schema().metadata()[KEY_PRICE_PRECISION].clone())
                .collect::<Vec<_>>(),
            vec!["2", "2"]
        );
        assert_part_matches_fresh_write(parts.into_iter().next().unwrap(), &expected);
    }

    // Runs the per-batch steps of `read_planned_migration_file` without an object store
    fn migrate_source_batches(
        type_name: &str,
        sources: &[RecordBatch],
    ) -> Vec<PreparedMigrationPart> {
        try_migrate_source_batches(type_name, sources).unwrap()
    }

    fn try_migrate_source_batches(
        type_name: &str,
        sources: &[RecordBatch],
    ) -> anyhow::Result<Vec<PreparedMigrationPart>> {
        let mut state = LegacyTranscodeState::default();
        let mut batches = Vec::new();
        let mut transcode_kind = LegacyTranscodeKind::PassThrough;

        for source in sources {
            let normalized = normalize_legacy_parquet_columns(source).unwrap();
            let transcoded = transcode_legacy_record_batch_with_state(
                type_name,
                "legacy.parquet",
                normalized,
                &mut state,
            )
            .unwrap();
            transcode_kind = transcoded.kind;
            batches.extend(transcoded.batches);
        }

        let fingerprint = schema_fingerprint(sources[0].schema_ref());

        let file = PlannedMigrationFile {
            path: format!("data/{type_name}/legacy.parquet"),
            relative_path: format!("data/{type_name}/legacy.parquet"),
            source_type_name: type_name.to_string(),
            target_type_name: type_name.to_string(),
            target_table: type_name.to_string(),
            size: 1,
            e_tag: None,
            version: None,
            last_modified: String::new(),
            source_fingerprint: fingerprint.clone(),
            target_fingerprint: fingerprint,
            transcode_kind,
        };

        prepare_migration_parts(&file, batches)
    }

    fn assert_part_matches_fresh_write<T>(part: PreparedMigrationPart, expected: &[T])
    where
        T: Clone + DecodeFromRecordBatch + EncodeToRecordBatch + PartialEq + std::fmt::Debug,
    {
        let fresh = T::encode_batch(&T::chunk_metadata(expected), expected).unwrap();

        let mut decoded = Vec::new();

        for batch in part.batches {
            let schema = batch.schema();
            assert_eq!(schema, fresh.schema());
            decoded.extend(T::decode_batch(schema.metadata(), batch).unwrap());
        }

        assert_eq!(part.row_count, expected.len());
        assert_eq!(decoded, expected);
    }

    #[rstest]
    #[case::instrument_u64("CryptoPerpetual", DataType::UInt64, true)]
    #[case::instrument_utc("CryptoPerpetual", timestamp_data_type(), true)]
    #[case::instrument_no_timezone(
        "CryptoPerpetual",
        DataType::Timestamp(TimeUnit::Nanosecond, None),
        false
    )]
    #[case::instrument_other_timezone("CryptoPerpetual", DataType::Timestamp(TimeUnit::Nanosecond, Some("America/New_York".into())), false)]
    #[case::quote_u64("QuoteTick", DataType::UInt64, false)]
    #[case::quote_utc("QuoteTick", timestamp_data_type(), false)]
    fn legacy_instrument_detection_requires_known_class_and_supported_timestamps(
        #[case] class: &str,
        #[case] timestamp_type: DataType,
        #[case] expected: bool,
    ) {
        let schema = Schema::new_with_metadata(
            vec![Field::new("ts_init", timestamp_type, false)],
            HashMap::from([(LEGACY_KEY_CLASS.to_string(), class.to_string())]),
        );

        assert_eq!(is_legacy_instrument_schema(&schema), expected);
    }

    #[rstest]
    #[case::utc(Some("UTC"))]
    #[case::canonical(None)]
    fn v2_timestamp_normalization_matches_preflight_and_preserves_values(
        #[case] timezone: Option<&str>,
    ) {
        let first = QuoteTick {
            instrument_id: InstrumentId::from("AAPL.XNAS"),
            bid_price: Price::from("123.45"),
            ask_price: Price::from("123.67"),
            bid_size: Quantity::from(17),
            ask_size: Quantity::from(29),
            ts_event: 1_788_652_800_123_456_789_u64.into(),
            ts_init: 1_788_652_800_123_456_799_u64.into(),
        };

        let values = vec![
            first,
            QuoteTick {
                ts_event: 1_788_652_800_123_456_801_u64.into(),
                ts_init: 1_788_652_800_123_456_899_u64.into(),
                ..first
            },
        ];
        let metadata = QuoteTick::get_metadata(&first.instrument_id, 2, 0);
        let expected = QuoteTick::encode_batch(&metadata, &values).unwrap();
        let source_type = DataType::Timestamp(TimeUnit::Nanosecond, timezone.map(Into::into));

        let fields = expected
            .schema()
            .fields()
            .iter()
            .map(|field| {
                let field = field.as_ref().clone();

                if matches!(field.data_type(), DataType::Timestamp(_, _)) {
                    field.with_data_type(source_type.clone())
                } else {
                    field
                }
            })
            .collect::<Vec<_>>();

        let columns = expected
            .columns()
            .iter()
            .map(|column| {
                if let Some(timestamps) = column.as_any().downcast_ref::<TimestampNanosecondArray>()
                {
                    Arc::new(
                        timestamps
                            .clone()
                            .with_timezone_opt(timezone.map(Arc::<str>::from)),
                    ) as ArrayRef
                } else {
                    column.clone()
                }
            })
            .collect::<Vec<_>>();

        let source = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(
                fields,
                expected.schema().metadata().clone(),
            )),
            columns,
        )
        .unwrap();
        let preflight = normalize_legacy_parquet_schema(source.schema_ref());
        let normalized = normalize_legacy_parquet_columns(&source).unwrap();
        assert_eq!(&preflight, expected.schema_ref().as_ref());
        assert_eq!(normalized, expected);
        assert_eq!(
            QuoteTick::decode_batch(&metadata, normalized).unwrap(),
            values
        );
    }

    #[rstest]
    fn timestamp_normalization_preserves_custom_nulls_and_unrelated_numeric_fields() {
        let schema = Arc::new(Schema::new_with_metadata(
            vec![
                Field::new(
                    "ts_event",
                    DataType::Timestamp(TimeUnit::Nanosecond, None),
                    true,
                ),
                Field::new("ts_count", DataType::UInt64, false),
            ],
            HashMap::from([("type_name".to_string(), "TimestampSample".to_string())]),
        ));
        let timestamps =
            TimestampNanosecondArray::from(vec![Some(1_788_652_800_123_456_789), None]);
        let source = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(timestamps),
                Arc::new(UInt64Array::from(vec![17, 29])),
            ],
        )
        .unwrap();
        let normalized = normalize_legacy_parquet_columns(&source).unwrap();
        assert_eq!(
            &normalize_legacy_parquet_schema(source.schema_ref()),
            normalized.schema_ref().as_ref()
        );
        assert_eq!(
            normalized.schema().field(0).data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into()))
        );
        assert_eq!(
            normalized
                .column(0)
                .as_any()
                .downcast_ref::<TimestampNanosecondArray>()
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            vec![Some(1_788_652_800_123_456_789), None]
        );
        assert_eq!(normalized.column(1), source.column(1));
    }

    #[rstest]
    fn normalize_legacy_info_schema_makes_binary_info_nullable() {
        let schema = Schema::new(vec![Field::new("info", DataType::Binary, false)]);

        let normalized = normalize_legacy_parquet_schema(&schema);

        let info = normalized.field_with_name("info").unwrap();
        assert_eq!(info.data_type(), &DataType::Utf8);
        assert!(info.is_nullable());
    }

    #[rstest]
    fn normalize_dictionary_string_columns_casts_string_dictionaries_to_utf8() {
        let mut builder = StringDictionaryBuilder::<Int8Type>::new();
        builder.append("AUD/USD.SIM").unwrap();
        builder.append("EUR/USD.SIM").unwrap();
        let dictionary = Arc::new(builder.finish()) as ArrayRef;

        let schema = Arc::new(Schema::new(vec![
            Field::new("instrument_id", dictionary.data_type().clone(), false),
            Field::new("ts_init", DataType::UInt64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                dictionary,
                Arc::new(UInt64Array::from(vec![1_u64, 2])) as ArrayRef,
            ],
        )
        .unwrap();

        let normalized = normalize_dictionary_string_columns(&batch).unwrap();

        assert_eq!(
            normalized
                .schema()
                .field_with_name("instrument_id")
                .unwrap()
                .data_type(),
            &DataType::Utf8,
        );
        let values = normalized
            .column_by_name("instrument_id")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .iter()
            .collect::<Vec<_>>();
        assert_eq!(values, vec![Some("AUD/USD.SIM"), Some("EUR/USD.SIM")]);
    }

    #[rstest]
    fn normalize_legacy_parquet_columns_preserves_unrecognized_dictionary() {
        let mut builder = StringDictionaryBuilder::<Int8Type>::new();
        builder.append("alpha").unwrap();
        let dictionary = Arc::new(builder.finish()) as ArrayRef;

        let schema = Arc::new(Schema::new(vec![Field::new(
            "label",
            dictionary.data_type().clone(),
            false,
        )]));
        let batch = RecordBatch::try_new(schema, vec![dictionary]).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized, batch);
    }

    #[rstest]
    fn normalize_open_custom_columns_preserves_dictionary_with_type_metadata() {
        let mut builder = StringDictionaryBuilder::<Int8Type>::new();
        builder.append("alpha").unwrap();
        let dictionary = Arc::new(builder.finish()) as ArrayRef;
        let decimal = Arc::new(
            Decimal128Array::from(vec![Some(123_i128)])
                .with_precision_and_scale(38, 16)
                .unwrap(),
        ) as ArrayRef;

        let schema = Arc::new(Schema::new_with_metadata(
            vec![
                Field::new("label", dictionary.data_type().clone(), false),
                Field::new("price", decimal.data_type().clone(), false),
                Field::new("ts_recv", DataType::UInt64, false),
            ],
            HashMap::from([("type_name".to_string(), "CustomData".to_string())]),
        ));
        let batch = RecordBatch::try_new(
            schema,
            vec![dictionary, decimal, Arc::new(UInt64Array::from(vec![7]))],
        )
        .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();
        let normalized_schema = normalize_legacy_parquet_schema(batch.schema_ref());

        assert_eq!(normalized, batch);
        assert_eq!(
            normalized_schema
                .field_with_name("label")
                .unwrap()
                .data_type(),
            batch.schema().field_with_name("label").unwrap().data_type(),
        );
    }

    #[rstest]
    fn normalize_legacy_parquet_schema_preserves_unrecognized_fixed_binary() {
        let schema = Schema::new(vec![Field::new(
            "price",
            DataType::FixedSizeBinary(8),
            false,
        )]);

        let normalized = normalize_legacy_parquet_schema(&schema);

        assert_eq!(normalized, schema);
    }

    #[rstest]
    fn normalize_legacy_parquet_columns_converts_binary_info_null_to_arrow_null() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Binary, true),
            Field::new("ts_init", DataType::UInt64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(BinaryArray::from_vec(vec![b"null".as_slice()])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![1_u64])) as ArrayRef,
            ],
        )
        .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();
        let info = normalized
            .column_by_name("info")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert_eq!(
            normalized
                .schema()
                .field_with_name("info")
                .unwrap()
                .data_type(),
            &DataType::Utf8,
        );
        assert!(info.is_null(0));
    }

    #[rstest]
    fn normalize_legacy_parquet_columns_preserves_quote_price_columns() {
        let decimal = DataType::Decimal128(38, 16);

        let schema = Arc::new(Schema::new(vec![
            Field::new("bid_price", decimal.clone(), false),
            Field::new("ask_price", decimal.clone(), false),
            Field::new("bid_size", decimal.clone(), false),
            Field::new("ask_size", decimal, false),
        ]));

        let values = || {
            Arc::new(
                Decimal128Array::from(vec![1_i128])
                    .with_precision_and_scale(38, 16)
                    .unwrap(),
            ) as ArrayRef
        };

        let batch =
            RecordBatch::try_new(schema.clone(), vec![values(), values(), values(), values()])
                .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized.schema(), schema);
        assert_eq!(normalized, batch);
    }

    #[rstest]
    fn normalize_legacy_depth_flat_columns_builds_structured_sides() {
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for level in 0..DEPTH10_LEN {
                for (name, value) in [("price", 11_i128), ("size", 22_i128)] {
                    fields.push(Field::new(
                        format!("{side}_{name}_{level}"),
                        DataType::Decimal128(38, 16),
                        true,
                    ));
                    let value = (level == 0).then_some(value);
                    columns.push(Arc::new(
                        Decimal128Array::from(vec![value])
                            .with_precision_and_scale(38, 16)
                            .unwrap(),
                    ) as ArrayRef);
                }

                fields.push(Field::new(
                    format!("{side}_count_{level}"),
                    DataType::UInt32,
                    false,
                ));
                columns.push(Arc::new(UInt32Array::from(vec![33])) as ArrayRef);
                fields.push(Field::new(
                    format!("{side}_order_id_{level}"),
                    DataType::UInt64,
                    false,
                ));
                columns.push(Arc::new(UInt64Array::from(vec![44])) as ArrayRef);
            }
        }

        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized.num_columns(), 2);
        assert_normalized_depth(&normalized, 1, 11, 22, 33, 44);
    }

    #[rstest]
    fn normalize_legacy_depth_fixed_lists_builds_structured_sides() {
        let decimal_values = |value| {
            Arc::new(
                Decimal128Array::from(
                    (0..DEPTH10_LEN)
                        .map(|level| (level == 0).then_some(value))
                        .collect::<Vec<_>>(),
                )
                .with_precision_and_scale(38, 16)
                .unwrap(),
            ) as ArrayRef
        };

        let counts = Arc::new(UInt32Array::from(vec![33; DEPTH10_LEN])) as ArrayRef;
        let order_ids = Arc::new(UInt64Array::from(vec![44; DEPTH10_LEN])) as ArrayRef;
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for (name, column) in [
                ("price", depth_list_array(decimal_values(11), true)),
                ("size", depth_list_array(decimal_values(22), true)),
                ("count", depth_list_array(counts.clone(), false)),
                ("order_id", depth_list_array(order_ids.clone(), false)),
            ] {
                fields.push(Field::new(
                    format!("{side}_{name}"),
                    column.data_type().clone(),
                    false,
                ));
                columns.push(column);
            }
        }

        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized.num_columns(), 2);
        assert_normalized_depth(&normalized, 1, 11, 22, 33, 44);
    }

    #[rstest]
    fn normalize_legacy_depth_missing_counts_and_order_ids_uses_list_width() {
        const WIDTH: i32 = 3;

        let decimal_values = |value| {
            Arc::new(
                Decimal128Array::from(vec![value; WIDTH as usize])
                    .with_precision_and_scale(38, 16)
                    .unwrap(),
            ) as ArrayRef
        };

        let list = |values: ArrayRef| {
            Arc::new(FixedSizeListArray::new(
                Arc::new(Field::new("item", values.data_type().clone(), false)),
                WIDTH,
                values,
                None,
            )) as ArrayRef
        };

        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for (name, column) in [
                ("price", list(decimal_values(11))),
                ("size", list(decimal_values(22))),
            ] {
                fields.push(Field::new(
                    format!("{side}_{name}"),
                    column.data_type().clone(),
                    false,
                ));
                columns.push(column);
            }
        }

        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized.num_columns(), 2);
        assert_normalized_depth(&normalized, WIDTH as usize, 11, 22, 0, 0);
    }

    #[rstest]
    #[case::with_order_ids(true, 44)]
    #[case::without_order_ids(false, 0)]
    fn normalize_legacy_depth_fixed_binary_lists_matches_schema(
        #[case] include_order_ids: bool,
        #[case] expected_order_id: u64,
    ) {
        let fixed_values = |value: [u8; 8]| {
            let values = (0..DEPTH10_LEN)
                .map(|level| (level == 0).then_some(value))
                .collect::<Vec<_>>();
            Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    values
                        .iter()
                        .map(Option::as_ref)
                        .map(|value| value.map(<[u8; 8]>::as_slice)),
                    8,
                )
                .unwrap(),
            ) as ArrayRef
        };

        let counts = Arc::new(UInt32Array::from(vec![33; DEPTH10_LEN])) as ArrayRef;
        let order_ids = Arc::new(UInt64Array::from(vec![44; DEPTH10_LEN])) as ArrayRef;
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            let mut side_columns = vec![
                (
                    "price",
                    depth_list_array(fixed_values(11_i64.to_le_bytes()), true),
                ),
                (
                    "size",
                    depth_list_array(fixed_values(22_u64.to_le_bytes()), true),
                ),
                ("count", depth_list_array(counts.clone(), false)),
            ];

            if include_order_ids {
                side_columns.push(("order_id", depth_list_array(order_ids.clone(), false)));
            }

            for (name, column) in side_columns {
                fields.push(Field::new(
                    format!("{side}_{name}"),
                    column.data_type().clone(),
                    false,
                ));
                columns.push(column);
            }
        }

        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        assert!(is_nautilus_legacy_schema(batch.schema_ref()));
        let normalized_schema = normalize_legacy_parquet_schema(batch.schema_ref());
        let normalized_batch = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(
            normalized_schema,
            normalized_batch.schema().as_ref().clone()
        );
        assert_normalized_depth(
            &normalized_batch,
            1,
            110_000_000,
            220_000_000,
            33,
            expected_order_id,
        );
    }

    #[rstest]
    fn normalize_legacy_depth_flat_fixed_columns_preserves_order_ids() {
        let fixed_price = || {
            let bytes = 11_i64.to_le_bytes();
            Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    [Some(bytes.as_slice())].into_iter(),
                    8,
                )
                .unwrap(),
            ) as ArrayRef
        };

        let fixed_size = || {
            let bytes = 22_u64.to_le_bytes();
            Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    [Some(bytes.as_slice())].into_iter(),
                    8,
                )
                .unwrap(),
            ) as ArrayRef
        };

        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for level in 0..DEPTH10_LEN {
                fields.push(Field::new(
                    format!("{side}_price_{level}"),
                    DataType::FixedSizeBinary(8),
                    false,
                ));
                columns.push(fixed_price());
                fields.push(Field::new(
                    format!("{side}_size_{level}"),
                    DataType::FixedSizeBinary(8),
                    false,
                ));
                columns.push(fixed_size());
                fields.push(Field::new(
                    format!("{side}_count_{level}"),
                    DataType::UInt32,
                    false,
                ));
                columns.push(Arc::new(UInt32Array::from(vec![33])) as ArrayRef);
                fields.push(Field::new(
                    format!("{side}_order_id_{level}"),
                    DataType::UInt64,
                    false,
                ));
                columns.push(Arc::new(UInt64Array::from(vec![44])) as ArrayRef);
            }
        }

        for (field, column) in [
            (
                Field::new("flags", DataType::UInt8, false),
                Arc::new(UInt8Array::from(vec![0])) as ArrayRef,
            ),
            (
                Field::new("sequence", DataType::UInt64, false),
                Arc::new(UInt64Array::from(vec![1])) as ArrayRef,
            ),
            (
                Field::new("ts_event", DataType::UInt64, false),
                Arc::new(UInt64Array::from(vec![2])) as ArrayRef,
            ),
            (
                Field::new("ts_init", DataType::UInt64, false),
                Arc::new(UInt64Array::from(vec![3])) as ArrayRef,
            ),
        ] {
            fields.push(field);
            columns.push(column);
        }

        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();
        assert!(is_nautilus_legacy_schema(batch.schema_ref()));

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        for side in ["bids", "asks"] {
            let list = normalized
                .column_by_name(side)
                .unwrap()
                .as_any()
                .downcast_ref::<ListArray>()
                .unwrap();
            let levels = list.value(0);
            let levels = levels.as_any().downcast_ref::<StructArray>().unwrap();
            let order_ids = levels
                .column_by_name("order_id")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();
            assert_eq!(order_ids.values(), &[44; DEPTH10_LEN]);
        }
    }

    #[rstest]
    fn normalize_legacy_depth_fixture_matches_open_shape() {
        let precision_dir = if cfg!(feature = "high-precision") {
            "128-bit"
        } else {
            "64-bit"
        };

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data/nautilus/legacy")
            .join(precision_dir)
            .join("depths.parquet");
        let file = std::fs::File::open(path).unwrap();
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
        let metadata = builder
            .metadata()
            .file_metadata()
            .key_value_metadata()
            .unwrap();

        for (key, value) in [
            ("instrument_id", "AAPL.XNAS"),
            ("price_precision", "4"),
            ("size_precision", "1"),
        ] {
            assert_eq!(
                metadata
                    .iter()
                    .find(|entry| entry.key == key)
                    .and_then(|entry| entry.value.as_deref()),
                Some(value),
            );
        }

        let batch = builder.build().unwrap().next().unwrap().unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_normalized_depth(
            &normalized,
            DEPTH10_LEN,
            12_345_000_000_000_000,
            25_000_000_000_000_000,
            3,
            0,
        );
        assert_eq!(normalized.num_columns(), 6);
        assert_eq!(
            normalized
                .column_by_name("flags")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt8Array>()
                .unwrap()
                .value(0),
            32
        );
        assert_eq!(
            normalized
                .column_by_name("sequence")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
                .value(0),
            7
        );

        for name in ["ts_event", "ts_init"] {
            assert_eq!(
                normalized
                    .schema()
                    .field_with_name(name)
                    .unwrap()
                    .data_type(),
                &DataType::Timestamp(arrow::datatypes::TimeUnit::Nanosecond, Some("UTC".into())),
            );
        }
    }

    #[rstest]
    #[case("quotes.parquet", "bid_price", None)]
    #[case("trades.parquet", "price", Some("aggressor_side"))]
    #[case("bars.parquet", "open", None)]
    #[case("deltas.parquet", "price", Some("action"))]
    fn legacy_market_fixture_matches_open_types(
        #[case] file_name: &str,
        #[case] fixed_field: &str,
        #[case] enum_field: Option<&str>,
    ) {
        let precision_dir = if cfg!(feature = "high-precision") {
            "128-bit"
        } else {
            "64-bit"
        };

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data/nautilus/legacy")
            .join(precision_dir)
            .join(file_name);
        let file = std::fs::File::open(path).unwrap();
        let batch = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap()
            .next()
            .unwrap()
            .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(
            normalized
                .schema()
                .field_with_name(fixed_field)
                .unwrap()
                .data_type(),
            &DataType::Decimal128(38, 16),
        );
        assert_eq!(
            normalized
                .schema()
                .field_with_name("ts_init")
                .unwrap()
                .data_type(),
            &DataType::Timestamp(arrow::datatypes::TimeUnit::Nanosecond, Some("UTC".into())),
        );

        if let Some(enum_field) = enum_field {
            assert!(matches!(
                normalized
                    .schema()
                    .field_with_name(enum_field)
                    .unwrap()
                    .data_type(),
                DataType::Dictionary(_, value) if value.as_ref() == &DataType::Utf8
            ));
        }
    }

    #[rstest]
    #[case("depths.parquet")]
    #[case("quotes.parquet")]
    #[case("trades.parquet")]
    #[case("bars.parquet")]
    #[case("deltas.parquet")]
    #[case("dictionary-trade")]
    fn legacy_fixture_schema_normalization_matches_batch(#[case] file_name: &str) {
        let (schema, batch) = if file_name == "dictionary-trade" {
            let dictionary = |value: &str| {
                let mut builder = StringDictionaryBuilder::<Int8Type>::new();
                builder.append(value).unwrap();
                Arc::new(builder.finish()) as ArrayRef
            };

            let price = 11_i64.to_le_bytes();
            let size = 22_u64.to_le_bytes();
            let trade_ids = dictionary("trade-1");
            let identifiers = dictionary("AAPL.XNAS");

            let schema = Arc::new(Schema::new_with_metadata(
                vec![
                    Field::new("price", DataType::FixedSizeBinary(8), false),
                    Field::new("size", DataType::FixedSizeBinary(8), false),
                    Field::new("aggressor_side", DataType::UInt8, false),
                    Field::new("trade_id", trade_ids.data_type().clone(), false),
                    Field::new("ts_event", DataType::UInt64, false),
                    Field::new("ts_init", DataType::UInt64, false),
                    Field::new(KEY_IDENTIFIER, identifiers.data_type().clone(), false),
                ],
                HashMap::from([("type".to_string(), "TradeTick".to_string())]),
            ));
            let batch = RecordBatch::try_new(
                Arc::clone(&schema),
                vec![
                    Arc::new(
                        FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                            [Some(price.as_slice())].into_iter(),
                            8,
                        )
                        .unwrap(),
                    ),
                    Arc::new(
                        FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                            [Some(size.as_slice())].into_iter(),
                            8,
                        )
                        .unwrap(),
                    ),
                    Arc::new(UInt8Array::from(vec![1])),
                    trade_ids,
                    Arc::new(UInt64Array::from(vec![1])),
                    Arc::new(UInt64Array::from(vec![2])),
                    identifiers,
                ],
            )
            .unwrap();
            (schema, batch)
        } else {
            let precision_dir = if cfg!(feature = "high-precision") {
                "128-bit"
            } else {
                "64-bit"
            };

            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test_data/nautilus/legacy")
                .join(precision_dir)
                .join(file_name);
            let file = std::fs::File::open(path).unwrap();
            let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
            let schema = builder.schema().clone();
            let batch = builder.build().unwrap().next().unwrap().unwrap();
            let batch =
                RecordBatch::try_new(Arc::clone(&schema), batch.columns().to_vec()).unwrap();
            (schema, batch)
        };

        let normalized_schema = normalize_legacy_parquet_schema(schema.as_ref());
        let normalized_batch = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(
            normalized_schema,
            normalized_batch.schema().as_ref().clone()
        );

        if file_name == "dictionary-trade" {
            assert_eq!(
                normalized_batch
                    .schema()
                    .field_with_name("trade_id")
                    .unwrap()
                    .data_type(),
                &DataType::Utf8,
            );
        }
    }

    fn assert_normalized_depth(
        batch: &RecordBatch,
        level_count: usize,
        price: i128,
        size: i128,
        count: u32,
        order_id: u64,
    ) {
        let schema = batch.schema();
        assert_eq!(schema.field(0).name(), "bids");
        assert_eq!(schema.field(1).name(), "asks");

        for side in ["bids", "asks"] {
            let list = batch
                .column_by_name(side)
                .unwrap()
                .as_any()
                .downcast_ref::<ListArray>()
                .unwrap();
            let levels = list.value(0);
            let levels = levels.as_any().downcast_ref::<StructArray>().unwrap();
            let prices = levels
                .column_by_name("price")
                .unwrap()
                .as_any()
                .downcast_ref::<Decimal128Array>()
                .unwrap();
            let sizes = levels
                .column_by_name("size")
                .unwrap()
                .as_any()
                .downcast_ref::<Decimal128Array>()
                .unwrap();
            let counts = levels
                .column_by_name("count")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt32Array>()
                .unwrap();
            let order_ids = levels
                .column_by_name("order_id")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();

            assert_eq!(levels.len(), level_count);
            assert_eq!(prices.value(0), price);
            assert_eq!(sizes.value(0), size);
            assert_eq!(counts.value(0), count);
            assert_eq!(order_ids.value(0), order_id);
        }
    }
}
