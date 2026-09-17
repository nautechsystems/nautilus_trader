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
    collections::BTreeMap,
    fmt::{Debug, Display},
    sync::Arc,
};

use arrow::{
    array::UInt32Array,
    compute::take,
    datatypes::{DataType as ArrowDataType, Schema},
    record_batch::RecordBatch,
};
use futures::{StreamExt, TryStreamExt};
use nautilus_model::data::NautilusRecordType;
use nautilus_serialization::arrow::{
    KEY_IDENTIFIER, KEY_INSTRUMENT_ID, StringColumnRef,
    legacy::{
        LegacyArrowError, LegacySchemaResolution, LegacyTranscodeKind, LegacyTranscodeState,
        SchemaFingerprint, resolve_legacy_schema, schema_fingerprint,
        transcode_legacy_record_batch_with_state,
    },
    record_batch_with_identifier_column,
};
use object_store::{ObjectMeta, ObjectStore, ObjectStoreExt, path::Path as ObjectPath};
use parquet::{
    normalize_legacy_parquet_columns, normalize_legacy_parquet_schema,
    read_parquet_from_object_store, read_parquet_schema_from_object_store,
};
use serde::Serialize;
use strum::IntoEnumIterator;

use crate::{
    backend::parquet::io as parquet,
    catalog::types::{
        INSTRUMENT_PATH_PREFIXES, data_path_prefix, data_type_from_data_path_prefix,
        record_path_prefix,
    },
    common::{arrow::catalog_record_schema, storage::normalize_storage_location},
};

const SCHEMA_READ_CONCURRENCY: usize = 16;

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
        let normalized = path.replace('\\', "/");
        let base = self.base_path().trim_matches('/');
        let full = if base.is_empty() {
            normalized
        } else {
            format!("{base}/{}", normalized.trim_start_matches('/'))
        };
        ObjectPath::parse(full.trim_start_matches('/')).map_err(anyhow::Error::from)
    }
}

/// Default maximum number of source rows written in one open-catalog migration commit.
pub const DEFAULT_MIGRATION_COMMIT_ROWS: usize = 500_000;

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

#[derive(Clone, Debug)]
enum SourceClassification {
    Migratable {
        source_type_name: String,
        target_type_name: String,
    },
    Unmigrated(String),
}

#[derive(Clone, Debug)]
struct SchemaCandidate {
    object: ObjectMeta,
    relative_path: String,
    source_type_name: String,
    target_type_name: String,
    object_path: ObjectPath,
}

#[derive(Debug)]
struct ResolvedCandidate {
    candidate: SchemaCandidate,
    target_table: String,
    resolution: LegacySchemaResolution,
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
        let batch = normalize_legacy_parquet_columns(&batch)?;
        let result = transcode_legacy_record_batch_with_state(
            &file.target_type_name,
            &file.path,
            batch,
            &mut state,
        )?;
        transcoded.extend(result.batches);
    }
    let batches = transcoded;
    Ok(batches)
}

/// Resolves identifiers and groups batches from one source file.
///
/// # Errors
///
/// Returns an error when an identifier column is not string-like or a batch cannot be sliced or
/// rebuilt with current identifier metadata.
pub fn prepare_migration_parts(
    file: &PlannedMigrationFile,
    batches: Vec<RecordBatch>,
) -> anyhow::Result<Vec<PreparedMigrationPart>> {
    let mut grouped: BTreeMap<(Option<String>, IdentifierSource), Vec<RecordBatch>> =
        BTreeMap::new();

    for batch in batches {
        for (identifier, source, batch) in split_batch_by_identifier(file, batch)? {
            let batch =
                batch_with_identifier(&file.target_type_name, identifier.as_deref(), batch)?;
            grouped.entry((identifier, source)).or_default().push(batch);
        }
    }

    Ok(grouped
        .into_iter()
        .map(
            |((identifier, identifier_source), batches)| PreparedMigrationPart {
                row_count: batches.iter().map(RecordBatch::num_rows).sum(),
                identifier,
                identifier_source,
                batches,
            },
        )
        .collect())
}

pub(crate) fn feather_replay_identity(
    source_uri: &str,
    source_path: &str,
    content_hash: &str,
    identifiers: Option<&[String]>,
) -> String {
    let mut identifiers = identifiers.map(<[String]>::to_vec);
    if let Some(identifiers) = identifiers.as_mut() {
        identifiers.sort();
        identifiers.dedup();
    }
    let identity = serde_json::json!({
        "source_uri": source_uri,
        "source_path": source_path,
        "content_hash": content_hash,
        "identifiers": identifiers,
    });
    format!(
        "nautilus-feather:{}",
        blake3::hash(identity.to_string().as_bytes()).to_hex(),
    )
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
        .collect::<std::collections::HashSet<_>>();
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

    if let Ok(data_type) = data_type_from_data_path_prefix(source_type_name) {
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
        let schema = normalize_legacy_parquet_schema(&schema);
        if candidate.target_type_name == "custom" {
            let Some(type_name) = schema.metadata().get("type_name") else {
                unresolved.push(UnresolvedSchema {
                    path: candidate.relative_path.clone(),
                    message: format!(
                        "Parquet custom data file {} is missing type_name metadata",
                        candidate.relative_path
                    ),
                });
                continue;
            };
            candidate.target_type_name = format!("custom/{type_name}");
        }
        let target_table = if candidate.target_type_name == "instruments" {
            let Some(class) = schema.metadata().get("class") else {
                unresolved.push(UnresolvedSchema {
                    path: candidate.relative_path.clone(),
                    message: format!(
                        "Parquet instrument file {} is missing class metadata",
                        candidate.relative_path
                    ),
                });
                continue;
            };
            format!("instruments/{class}")
        } else {
            candidate.target_type_name.clone()
        };

        if let Ok(record_type) = candidate.target_type_name.parse::<NautilusRecordType>()
            && let Ok(current) = catalog_record_schema(&record_type)
        {
            let expected =
                nautilus_serialization::arrow::schema_without_identifier_column(&current);
            let actual = nautilus_serialization::arrow::schema_without_identifier_column(&schema);

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
                let batch = take_record_batch(&batch, &indices)?;
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
    let key = if type_name == "bars" {
        "bar_type"
    } else {
        KEY_INSTRUMENT_ID
    };
    batch.schema().metadata().get(key).cloned()
}

fn identifier_column<'a>(
    type_name: &str,
    batch: &'a RecordBatch,
) -> anyhow::Result<Option<StringColumnRef<'a>>> {
    let candidates = if type_name == "bars" {
        [KEY_IDENTIFIER, "bar_type", KEY_INSTRUMENT_ID, "id"]
    } else {
        [KEY_IDENTIFIER, KEY_INSTRUMENT_ID, "bar_type", "id"]
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

fn take_record_batch(batch: &RecordBatch, indices: &[u32]) -> anyhow::Result<RecordBatch> {
    let indices = UInt32Array::from(indices.to_vec());
    let columns = batch
        .columns()
        .iter()
        .map(|column| take(column.as_ref(), &indices, None))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RecordBatch::try_new(batch.schema(), columns)?)
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
    let metadata_key = if type_name == "bars" {
        "bar_type"
    } else if type_name.starts_with("custom/") {
        return Ok(batch);
    } else {
        KEY_INSTRUMENT_ID
    };
    let mut metadata = batch.schema().metadata().clone();
    metadata.insert(metadata_key.to_string(), identifier.to_string());
    let schema = Arc::new(Schema::new_with_metadata(
        batch.schema().fields().iter().cloned().collect::<Vec<_>>(),
        metadata,
    ));
    Ok(RecordBatch::try_new(schema, batch.columns().to_vec())?)
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

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use ::parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use arrow::{
        array::Int64Array,
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
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
            .join("../../test_data/nautilus")
            .join(precision_dir)
            .join(file_name);
        let file = std::fs::File::open(path).unwrap();
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
        let schema = normalize_legacy_parquet_schema(builder.schema().as_ref());

        let resolution = resolve_legacy_schema(type_name, file_name, &schema).unwrap();

        assert_eq!(resolution.kind, LegacyTranscodeKind::PassThrough);
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
            .join("../../test_data/nautilus")
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
}
