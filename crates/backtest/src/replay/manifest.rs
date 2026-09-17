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

//! A reproducible description of one captured replay archive.
//!
//! A replay is only reproducible if the archive states what it holds, what it is missing, and what
//! the capture could not obtain. [`ReplayManifest`] records the source, capture time, schema
//! version, checksum, time range, and per-dataset limitations of one capture, and turns its
//! datasets into the catalog queries a `BacktestNode` replays.
//!
//! # Guarantees
//!
//! - The checksum covers every declared field except the checksum itself, so an edited window,
//!   row count, identifier list, or limitation fails to load.
//! - Every declared file must exist and match its recorded digest, so a truncated or replaced
//!   archive fails to load rather than replaying a shorter history silently.
//! - Declared limitations are never dropped. [`ReplayManifest::is_complete`] reports whether a
//!   replay of this archive is a full history or a scoped incomplete one.
//! - A dataset that names the wrong identifier shape for its data type fails to load, because such
//!   a manifest would query nothing and present an empty history as a complete one.

use std::{
    fs,
    path::{Path, PathBuf},
};

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarType, CatalogPathPrefix, FundingRateUpdate, IndexPriceUpdate, InstrumentClose,
        InstrumentStatus, MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDepth10,
        QuoteTick, TradeTick,
    },
    identifiers::InstrumentId,
    prediction::MarketResolution,
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use serde::{Deserialize, Serialize};

use crate::config::{BacktestDataConfig, NautilusDataType};

/// The schema version this build writes and accepts.
pub const REPLAY_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// The prefix of every checksum this module writes.
pub const REPLAY_CHECKSUM_PREFIX: &str = "blake3:";

/// Where a captured archive came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaySource {
    /// The trading venue the captured data belongs to.
    pub venue: String,
    /// The loader, endpoint, or capture tool that produced the archive.
    pub loader: String,
    /// UNIX timestamp (nanoseconds) the capture ran.
    pub captured_at: UnixNanos,
}

/// A declared limit on what a capture contains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReplayLimitation {
    /// The source holds no history for this dataset, so replaying it is unsupported.
    NotCaptured {
        /// Why the dataset is absent.
        reason: String,
    },
    /// The source paginates, and the capture stopped at the source's documented ceiling.
    PaginationCeiling {
        /// Rows the capture obtained before pagination stopped.
        captured: u64,
        /// What the source reported about the ceiling that stopped pagination.
        reason: String,
    },
    /// Captured rows are absent inside the declared window.
    Gap {
        /// UNIX timestamp (nanoseconds) the gap starts.
        start: UnixNanos,
        /// UNIX timestamp (nanoseconds) the gap ends.
        end: UnixNanos,
    },
}

impl ReplayLimitation {
    /// Returns a one-line description suitable for a replay report.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::NotCaptured { reason } => format!("not captured: {reason}"),
            Self::PaginationCeiling { captured, reason } => {
                format!(
                    "paginated capture stopped at the source's ceiling after {captured} rows: {reason}"
                )
            }
            Self::Gap { start, end } => format!("missing rows from {start} to {end}"),
        }
    }
}

/// One dataset of a captured archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayDataset {
    /// The [`NautilusDataType`] name, such as `TradeTick` or `MarketResolution`.
    pub data_type: String,
    /// The catalog the captured files are stored in.
    pub catalog_path: String,
    /// Instrument identifiers, or bar types for bar data. Empty for whole-catalog datasets.
    #[serde(default)]
    pub identifiers: Vec<String>,
    /// UNIX timestamp (nanoseconds) the captured window starts.
    pub start: UnixNanos,
    /// UNIX timestamp (nanoseconds) the captured window ends.
    pub end: UnixNanos,
    /// Rows the capture obtained, as the capture declared them.
    pub rows: u64,
    /// Captured catalog files, relative to `catalog_path`, in checksum order.
    #[serde(default)]
    pub files: Vec<String>,
    /// The `blake3:<hex>` digest of `files`, in order.
    pub checksum: String,
    /// What this dataset is missing. An empty list claims a complete dataset.
    #[serde(default)]
    pub limitations: Vec<ReplayLimitation>,
}

impl ReplayDataset {
    /// Returns the data type of this dataset.
    ///
    /// # Errors
    ///
    /// Returns an error if the declared name is not a [`NautilusDataType`].
    pub fn data_type(&self) -> anyhow::Result<NautilusDataType> {
        self.data_type
            .parse::<NautilusDataType>()
            .map_err(|_| anyhow::anyhow!("invalid replay dataset data type '{}'", self.data_type))
    }

    /// Returns whether the capture declared this dataset complete.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.limitations.is_empty()
    }

    /// Returns whether this dataset can be replayed from the catalog.
    ///
    /// A dataset the source never captured, or one that lists no captured files, has nothing to
    /// query, so it is reported instead of being replayed as an empty history.
    #[must_use]
    pub fn is_replayable(&self) -> bool {
        !self.files.is_empty()
            && !self
                .limitations
                .iter()
                .any(|limitation| matches!(limitation, ReplayLimitation::NotCaptured { .. }))
    }

    /// Declares the dataset a capture wrote into `catalog`.
    ///
    /// The dataset lists exactly the files the catalog holds for `identifiers` inside the window,
    /// bound to their digest, so a replay can verify that the archive it loads is the one the
    /// capture recorded. `rows` is the row count the capture declares, which is not the number of
    /// files: one file holds many rows.
    ///
    /// # Errors
    ///
    /// Returns an error if the catalog root is not a local path, the catalog cannot be listed, or
    /// a captured file cannot be read.
    pub fn capture(
        catalog: &ParquetDataCatalog,
        data_type: NautilusDataType,
        identifiers: Vec<String>,
        start: UnixNanos,
        end: UnixNanos,
        rows: u64,
        limitations: Vec<ReplayLimitation>,
    ) -> anyhow::Result<Self> {
        let catalog_path = catalog.original_uri.clone();
        let files = catalog.list_parquet_files_with_criteria(
            catalog_prefix(data_type),
            (!identifiers.is_empty()).then_some(identifiers.as_slice()),
            Some(start),
            Some(end),
        )?;
        let checksum = if files.is_empty() {
            String::new()
        } else {
            Self::checksum_files(&catalog_path, &files)?
        };

        Ok(Self {
            data_type: data_type.to_string(),
            catalog_path,
            identifiers,
            start,
            end,
            rows,
            files,
            checksum,
            limitations,
        })
    }

    /// Returns the `blake3:<hex>` digest of the given catalog files, in the order given.
    ///
    /// # Errors
    ///
    /// Returns an error if `catalog_path` is not a local path, or a file cannot be read.
    fn checksum_files(catalog_path: &str, files: &[String]) -> anyhow::Result<String> {
        let root = local_catalog_root(catalog_path)?;
        let mut hasher = blake3::Hasher::new();

        for file in files {
            let bytes = fs::read(root.join(file)).map_err(|e| {
                anyhow::anyhow!(
                    "cannot read replay file '{file}' under '{}': {e}",
                    root.display()
                )
            })?;
            hasher.update(&bytes);
        }

        Ok(format!(
            "{REPLAY_CHECKSUM_PREFIX}{}",
            hasher.finalize().to_hex()
        ))
    }

    /// Verifies every declared file exists and matches its recorded digest.
    ///
    /// # Errors
    ///
    /// Returns an error if `catalog_path` is not a local path, a file is missing, or a file no
    /// longer matches the captured digest.
    pub fn verify_files(&self) -> anyhow::Result<()> {
        if self.files.is_empty() {
            return Ok(());
        }

        let root = local_catalog_root(&self.catalog_path)?;

        for file in &self.files {
            let path = root.join(file);
            if !path.is_file() {
                anyhow::bail!(
                    "replay dataset '{}' is missing captured file '{file}' under '{}'",
                    self.data_type,
                    root.display()
                );
            }
        }

        let computed = Self::checksum_files(&self.catalog_path, &self.files)?;
        anyhow::ensure!(
            computed == self.checksum,
            "replay dataset '{}' checksum mismatch: captured {}, found {computed}",
            self.data_type,
            self.checksum
        );

        Ok(())
    }
}

/// A captured replay archive, reproducible from its catalog files alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayManifest {
    /// The manifest schema version, which must be [`REPLAY_MANIFEST_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Where the archive came from.
    pub source: ReplaySource,
    /// UNIX timestamp (nanoseconds) the declared replay window starts.
    pub start: UnixNanos,
    /// UNIX timestamp (nanoseconds) the declared replay window ends.
    pub end: UnixNanos,
    /// The `blake3:<hex>` digest of every declared field except this one.
    pub checksum: String,
    /// The captured datasets.
    pub datasets: Vec<ReplayDataset>,
}

impl ReplayManifest {
    /// Creates a manifest for `datasets`, computing its checksum.
    ///
    /// # Errors
    ///
    /// Returns an error if the declared window, dataset windows, identifiers, or files are
    /// inconsistent (see [`ReplayManifest::validate`]).
    pub fn new(
        source: ReplaySource,
        start: UnixNanos,
        end: UnixNanos,
        datasets: Vec<ReplayDataset>,
    ) -> anyhow::Result<Self> {
        let mut manifest = Self {
            schema_version: REPLAY_MANIFEST_SCHEMA_VERSION,
            source,
            start,
            end,
            checksum: String::new(),
            datasets,
        };
        manifest.validate()?;
        manifest.checksum = manifest.compute_checksum()?;

        Ok(manifest)
    }

    /// Returns `blake3:<hex>` over every declared field except the checksum.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be serialized.
    pub fn compute_checksum(&self) -> anyhow::Result<String> {
        #[derive(Serialize)]
        struct Body<'a> {
            schema_version: u32,
            source: &'a ReplaySource,
            start: UnixNanos,
            end: UnixNanos,
            datasets: &'a [ReplayDataset],
        }

        let body = Body {
            schema_version: self.schema_version,
            source: &self.source,
            start: self.start,
            end: self.end,
            datasets: &self.datasets,
        };
        let bytes = serde_json::to_vec(&body)?;

        Ok(format!(
            "{REPLAY_CHECKSUM_PREFIX}{}",
            blake3::hash(&bytes).to_hex()
        ))
    }

    /// Validates the declared schema version, window, checksum, and datasets.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema version is unsupported, the declared range is inverted, no
    /// dataset is declared, the checksum does not cover the declared fields, a dataset window lies
    /// outside the manifest range, a dataset repeats another, a dataset names the wrong identifier
    /// shape or an unparsable identifier, or a dataset declared complete lists no captured file.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == REPLAY_MANIFEST_SCHEMA_VERSION,
            "unsupported replay manifest schema version {}, expected {REPLAY_MANIFEST_SCHEMA_VERSION}",
            self.schema_version
        );
        anyhow::ensure!(
            self.start <= self.end,
            "replay manifest range start {} must be <= end {}",
            self.start,
            self.end
        );
        anyhow::ensure!(
            !self.datasets.is_empty(),
            "replay manifest declares no datasets"
        );

        if !self.checksum.is_empty() {
            let computed = self.compute_checksum()?;
            anyhow::ensure!(
                computed == self.checksum,
                "replay manifest checksum mismatch: declared {}, recomputed {computed}",
                self.checksum
            );
        }

        for dataset in &self.datasets {
            let data_type = dataset.data_type()?;
            anyhow::ensure!(
                dataset.start <= dataset.end,
                "replay dataset '{data_type}' window start {} must be <= end {}",
                dataset.start,
                dataset.end
            );
            anyhow::ensure!(
                dataset.start >= self.start && dataset.end <= self.end,
                "replay dataset '{data_type}' window {} to {} is outside the manifest range {} to {}",
                dataset.start,
                dataset.end,
                self.start,
                self.end
            );
            anyhow::ensure!(
                dataset.checksum.starts_with(REPLAY_CHECKSUM_PREFIX) || dataset.files.is_empty(),
                "replay dataset '{data_type}' declares files but no {} checksum",
                REPLAY_CHECKSUM_PREFIX.trim_end_matches(':')
            );
            anyhow::ensure!(
                !dataset.is_complete() || !dataset.files.is_empty(),
                "replay dataset '{data_type}' is declared complete but lists no captured files"
            );
            dataset_identifiers(dataset)?;
        }

        let mut seen: Vec<(NautilusDataType, &Vec<String>)> =
            Vec::with_capacity(self.datasets.len());
        for dataset in &self.datasets {
            let key = (dataset.data_type()?, &dataset.identifiers);
            anyhow::ensure!(
                !seen.contains(&key),
                "replay manifest declares '{}' for {:?} more than once",
                dataset.data_type,
                dataset.identifiers
            );
            seen.push(key);
        }

        Ok(())
    }

    /// Verifies every declared file of every dataset.
    ///
    /// # Errors
    ///
    /// Returns an error if a catalog path is not local, a file is missing, or a checksum differs.
    pub fn verify_files(&self) -> anyhow::Result<()> {
        for dataset in &self.datasets {
            dataset.verify_files()?;
        }

        Ok(())
    }

    /// Returns whether every dataset claims a complete capture.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.datasets.iter().all(ReplayDataset::is_complete)
    }

    /// Returns every declared limitation with the dataset that declared it.
    #[must_use]
    pub fn limitations(&self) -> Vec<(&ReplayDataset, &ReplayLimitation)> {
        self.datasets
            .iter()
            .flat_map(|dataset| {
                dataset
                    .limitations
                    .iter()
                    .map(move |limitation| (dataset, limitation))
            })
            .collect()
    }

    /// Returns the catalog queries that replay this archive.
    ///
    /// Datasets the source never captured are reported rather than queried, because there is no
    /// history for them to replay.
    ///
    /// # Errors
    ///
    /// Returns an error if a dataset declares an invalid identifier, or a query is rejected.
    pub fn data_configs(&self) -> anyhow::Result<Vec<BacktestDataConfig>> {
        let datasets = self
            .datasets
            .iter()
            .filter(|dataset| dataset.is_replayable());
        let mut configs = Vec::with_capacity(self.datasets.len());

        for dataset in datasets {
            let data_type = dataset.data_type()?;
            let builder = BacktestDataConfig::builder()
                .data_type(data_type)
                .catalog_path(dataset.catalog_path.clone())
                .start_time(dataset.start)
                .end_time(dataset.end);
            let config = match data_type {
                // Resolutions are addressed by outcome group, which is not an instrument identity.
                NautilusDataType::MarketResolution => builder.build()?,
                NautilusDataType::Bar => builder.bar_types(dataset.identifiers.clone()).build()?,
                _ => {
                    let ids = dataset
                        .identifiers
                        .iter()
                        .map(|identifier| {
                            identifier.parse::<InstrumentId>().map_err(|e| {
                                anyhow::anyhow!(
                                    "invalid instrument identifier '{identifier}' for replay dataset '{data_type}': {e}"
                                )
                            })
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    builder.instrument_ids(ids).build()?
                }
            };

            configs.push(config);
        }

        Ok(configs)
    }

    /// Decodes a manifest from JSON, verifying its schema version, checksum, and declarations.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is malformed, carries unknown fields, or fails validation.
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let manifest: Self = serde_json::from_str(json)?;
        manifest.validate()?;

        Ok(manifest)
    }

    /// Loads a manifest from a JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, or the manifest fails validation.
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let json = fs::read_to_string(path).map_err(|e| {
            anyhow::anyhow!("cannot read replay manifest '{}': {e}", path.display())
        })?;

        Self::from_json(&json)
    }

    /// Encodes the manifest as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be serialized.
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Writes the manifest to a JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be serialized or the file cannot be written.
    pub fn write(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = path.as_ref();
        fs::write(path, self.to_json()?)
            .map_err(|e| anyhow::anyhow!("cannot write replay manifest '{}': {e}", path.display()))
    }
}

/// Returns the catalog path prefix the repository stores `data_type` under.
fn catalog_prefix(data_type: NautilusDataType) -> &'static str {
    match data_type {
        NautilusDataType::QuoteTick => QuoteTick::path_prefix(),
        NautilusDataType::TradeTick => TradeTick::path_prefix(),
        NautilusDataType::Bar => Bar::path_prefix(),
        NautilusDataType::OrderBookDelta => OrderBookDelta::path_prefix(),
        NautilusDataType::OrderBookDepth10 => OrderBookDepth10::path_prefix(),
        NautilusDataType::MarkPriceUpdate => MarkPriceUpdate::path_prefix(),
        NautilusDataType::IndexPriceUpdate => IndexPriceUpdate::path_prefix(),
        NautilusDataType::FundingRateUpdate => FundingRateUpdate::path_prefix(),
        NautilusDataType::InstrumentStatus => InstrumentStatus::path_prefix(),
        NautilusDataType::OptionGreeks => OptionGreeks::path_prefix(),
        NautilusDataType::InstrumentClose => InstrumentClose::path_prefix(),
        NautilusDataType::MarketResolution => MarketResolution::path_prefix(),
    }
}

/// Returns the local root of a catalog path, rejecting remote schemes.
fn local_catalog_root(catalog_path: &str) -> anyhow::Result<PathBuf> {
    let stripped = catalog_path.strip_prefix("file://").unwrap_or(catalog_path);

    anyhow::ensure!(
        !stripped.contains("://"),
        "replay checksum verification requires a local catalog path, was '{catalog_path}'"
    );

    Ok(PathBuf::from(stripped))
}

/// Validates the identifier shape a dataset declares for its data type.
fn dataset_identifiers(dataset: &ReplayDataset) -> anyhow::Result<()> {
    let data_type = dataset.data_type()?;

    if data_type == NautilusDataType::MarketResolution {
        anyhow::ensure!(
            dataset.identifiers.is_empty(),
            "replay dataset '{data_type}' must not declare identifiers, because resolutions are \
             addressed by outcome group"
        );

        return Ok(());
    }

    for identifier in &dataset.identifiers {
        if data_type == NautilusDataType::Bar {
            identifier.parse::<BarType>().map_err(|e| {
                anyhow::anyhow!(
                    "invalid bar type '{identifier}' for replay dataset '{data_type}': {e}"
                )
            })?;
        } else {
            identifier.parse::<InstrumentId>().map_err(|e| {
                anyhow::anyhow!(
                    "invalid instrument identifier '{identifier}' for replay dataset '{data_type}': {e}"
                )
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nautilus_model::{
        data::QuoteTick,
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    const START: u64 = 1_700_000_000_000_000_000;
    const END: u64 = 1_700_000_600_000_000_000;

    fn source() -> ReplaySource {
        ReplaySource {
            venue: "POLYMARKET".to_string(),
            loader: "polymarket_data_api".to_string(),
            captured_at: UnixNanos::from(1_700_000_900_000_000_000u64),
        }
    }

    fn trades_dataset(catalog_path: &str, files: Vec<String>) -> ReplayDataset {
        ReplayDataset {
            data_type: "TradeTick".to_string(),
            catalog_path: catalog_path.to_string(),
            identifiers: vec!["0xCONDITION-YES.POLYMARKET".to_string()],
            start: UnixNanos::from(START),
            end: UnixNanos::from(END),
            rows: 2,
            checksum: if files.is_empty() {
                String::new()
            } else {
                ReplayDataset::checksum_files(catalog_path, &files).unwrap()
            },
            files,
            limitations: Vec::new(),
        }
    }

    fn resolutions_dataset(catalog_path: &str, files: Vec<String>) -> ReplayDataset {
        ReplayDataset {
            data_type: "MarketResolution".to_string(),
            catalog_path: catalog_path.to_string(),
            identifiers: Vec::new(),
            start: UnixNanos::from(START),
            end: UnixNanos::from(END),
            rows: 1,
            checksum: if files.is_empty() {
                String::new()
            } else {
                ReplayDataset::checksum_files(catalog_path, &files).unwrap()
            },
            files,
            limitations: Vec::new(),
        }
    }

    /// Writes a captured file into a fresh catalog directory and returns its root and relative path.
    fn captured_file(contents: &[u8]) -> (TempDir, String, String) {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_str().unwrap().to_string();
        let relative =
            "data/trades/0xCONDITION-YES.POLYMARKET/2025-01-01_2025-01-02.parquet".to_string();
        let path = temp.path().join(&relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();

        (temp, root, relative)
    }

    #[rstest]
    fn test_manifest_roundtrips_through_json() {
        let (_temp, root, file) = captured_file(b"captured trades");
        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![
                trades_dataset(&root, vec![file.clone()]),
                resolutions_dataset(&root, vec![file]),
            ],
        )
        .unwrap();

        let encoded = manifest.to_json().unwrap();
        let decoded = ReplayManifest::from_json(&encoded).unwrap();

        assert_eq!(decoded, manifest);
        assert!(decoded.is_complete());
        assert!(decoded.checksum.starts_with(REPLAY_CHECKSUM_PREFIX));
    }

    #[rstest]
    fn test_load_rejects_unsupported_schema_version() {
        let (_temp, root, file) = captured_file(b"captured trades");
        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![trades_dataset(&root, vec![file])],
        )
        .unwrap();
        let json = manifest
            .to_json()
            .unwrap()
            .replace("\"schema_version\": 1", "\"schema_version\": 2");

        let error = ReplayManifest::from_json(&json).unwrap_err();

        assert!(error.to_string().contains("schema version"), "{error}");
    }

    #[rstest]
    fn test_load_rejects_edited_dataset_declaration() {
        let (_temp, root, file) = captured_file(b"captured trades");
        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![trades_dataset(&root, vec![file])],
        )
        .unwrap();
        let json = manifest
            .to_json()
            .unwrap()
            .replace("\"rows\": 2", "\"rows\": 3");

        let error = ReplayManifest::from_json(&json).unwrap_err();

        assert!(error.to_string().contains("checksum mismatch"), "{error}");
    }

    #[rstest]
    fn test_load_rejects_dataset_outside_manifest_range() {
        let (_temp, root, file) = captured_file(b"captured trades");
        let mut dataset = trades_dataset(&root, vec![file]);
        dataset.start = UnixNanos::from(START - 1_000_000_000);
        // Built directly, because `new` rejects the inconsistent window it declares.
        let manifest = ReplayManifest {
            schema_version: REPLAY_MANIFEST_SCHEMA_VERSION,
            source: source(),
            start: UnixNanos::from(START),
            end: UnixNanos::from(END),
            checksum: String::new(),
            datasets: vec![dataset],
        };

        let error = manifest.validate().unwrap_err();

        assert!(
            error.to_string().contains("outside the manifest range"),
            "{error}"
        );
    }

    #[rstest]
    fn test_load_rejects_resolution_dataset_with_identifiers() {
        let (_temp, root, file) = captured_file(b"captured resolutions");
        let mut dataset = resolutions_dataset(&root, vec![file]);
        dataset.identifiers = vec!["0xCONDITION.POLYMARKET".to_string()];

        let error = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("addressed by outcome group"),
            "{error}"
        );
    }

    #[rstest]
    fn test_load_rejects_unknown_data_type() {
        let (_temp, root, file) = captured_file(b"captured resolutions");
        let mut dataset = resolutions_dataset(&root, vec![file]);
        dataset.data_type = "Resolution".to_string();

        let error = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("invalid replay dataset data type"),
            "{error}"
        );
    }

    #[rstest]
    fn test_load_rejects_duplicate_dataset() {
        let (_temp, root, file) = captured_file(b"captured resolutions");
        let dataset = resolutions_dataset(&root, vec![file]);

        let error = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset.clone(), dataset],
        )
        .unwrap_err();

        assert!(error.to_string().contains("more than once"), "{error}");
    }

    #[rstest]
    fn test_complete_dataset_must_list_captured_files() {
        let mut dataset = resolutions_dataset("/tmp/catalog", Vec::new());
        dataset.rows = 4;

        let error = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap_err();

        assert!(error.to_string().contains("no captured files"), "{error}");
    }

    #[rstest]
    fn test_verify_files_detects_missing_file() {
        let (_temp, root, file) = captured_file(b"captured trades");
        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![trades_dataset(&root, vec![file.clone()])],
        )
        .unwrap();
        fs::remove_file(Path::new(&root).join(&file)).unwrap();

        let error = manifest.verify_files().unwrap_err();

        assert!(
            error.to_string().contains("is missing captured file"),
            "{error}"
        );
    }

    #[rstest]
    fn test_verify_files_detects_edited_file() {
        let (_temp, root, file) = captured_file(b"captured trades");
        let dataset = trades_dataset(&root, vec![file.clone()]);
        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap();
        fs::write(Path::new(&root).join(&file), b"shorter").unwrap();

        let error = manifest.verify_files().unwrap_err();

        assert!(error.to_string().contains("checksum mismatch"), "{error}");
    }

    #[rstest]
    fn test_verify_files_rejects_remote_catalog() {
        // Built directly, because a remote catalog cannot be checksummed through the filesystem.
        let dataset = ReplayDataset {
            data_type: "OrderBookDelta".to_string(),
            catalog_path: "s3://bucket/catalog".to_string(),
            identifiers: vec!["0xCONDITION-YES.POLYMARKET".to_string()],
            start: UnixNanos::from(START),
            end: UnixNanos::from(END),
            rows: 1,
            files: vec!["data/order_book_deltas/x.parquet".to_string()],
            checksum: format!("{REPLAY_CHECKSUM_PREFIX}{}", "0".repeat(64)),
            limitations: Vec::new(),
        };
        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap();

        let error = manifest.verify_files().unwrap_err();

        assert!(
            error.to_string().contains("requires a local catalog path"),
            "{error}"
        );
    }

    #[rstest]
    fn test_data_configs_map_datasets_to_catalog_queries() {
        let (_temp, root, file) = captured_file(b"captured trades");
        let mut bar_dataset = trades_dataset(&root, vec![file.clone()]);
        bar_dataset.data_type = "Bar".to_string();
        bar_dataset.identifiers =
            vec!["0xCONDITION-YES.POLYMARKET-1-MINUTE-LAST-EXTERNAL".to_string()];
        let resolutions = resolutions_dataset(&root, vec![file.clone()]);
        let trades = trades_dataset(&root, vec![file]);
        let mut book = resolutions_dataset(&root, Vec::new());
        book.data_type = "OrderBookDelta".to_string();
        book.rows = 0;
        book.limitations = vec![ReplayLimitation::NotCaptured {
            reason: "no historical book".to_string(),
        }];
        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![trades, bar_dataset, resolutions, book],
        )
        .unwrap();

        let configs = manifest.data_configs().unwrap();

        assert!(!manifest.is_complete());
        assert_eq!(
            configs.len(),
            3,
            "an uncaptured dataset is reported, not queried"
        );
        assert_eq!(configs[0].data_type(), NautilusDataType::TradeTick);
        assert_eq!(
            configs[0].query_identifiers(),
            Some(vec!["0xCONDITION-YES.POLYMARKET".to_string()])
        );
        assert_eq!(configs[0].start_time(), Some(UnixNanos::from(START)));
        assert_eq!(
            configs[1].query_identifiers(),
            Some(vec![
                "0xCONDITION-YES.POLYMARKET-1-MINUTE-LAST-EXTERNAL".to_string()
            ])
        );
        assert_eq!(configs[2].data_type(), NautilusDataType::MarketResolution);
        assert_eq!(configs[2].query_identifiers(), None);
        assert_eq!(configs[2].catalog_path(), &root);
    }

    #[rstest]
    fn test_pagination_ceiling_marks_the_archive_incomplete() {
        let (_temp, root, file) = captured_file(b"captured trades");
        let mut dataset = trades_dataset(&root, vec![file]);
        dataset.limitations = vec![ReplayLimitation::PaginationCeiling {
            captured: 2,
            reason: "max historical activity offset reached".to_string(),
        }];
        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap();

        // The encode/decode round trip must preserve the declared limitation.
        let decoded = ReplayManifest::from_json(&manifest.to_json().unwrap()).unwrap();

        assert!(!decoded.is_complete());
        let limitations = decoded.limitations();

        assert_eq!(limitations.len(), 1);
        assert_eq!(
            limitations[0].1.describe(),
            "paginated capture stopped at the source's ceiling after 2 rows: max historical activity offset reached"
        );
        assert_eq!(
            limitations[0].0.data_type().unwrap(),
            NautilusDataType::TradeTick
        );
        assert_eq!(limitations[0].0.rows, 2);
    }

    #[rstest]
    fn test_not_captured_dataset_describes_an_unsupported_replay() {
        let mut dataset = resolutions_dataset("/tmp/catalog", Vec::new());
        dataset.limitations = vec![ReplayLimitation::NotCaptured {
            reason: "Polymarket exposes no historical book".to_string(),
        }];

        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap();

        assert!(!manifest.is_complete());
        assert_eq!(
            manifest.limitations()[0].1.describe(),
            "not captured: Polymarket exposes no historical book"
        );
    }

    #[rstest]
    fn test_identifier_must_parse_for_its_data_type() {
        let (_temp, root, file) = captured_file(b"captured quotes");
        let mut dataset = resolutions_dataset(&root, vec![file]);
        dataset.data_type = "QuoteTick".to_string();
        dataset.identifiers = vec!["not an instrument".to_string()];

        let error = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("invalid instrument identifier"),
            "{error}"
        );
    }

    #[rstest]
    fn test_instrument_identifier_round_trips_into_a_query() {
        let (_temp, root, file) = captured_file(b"captured quotes");
        let mut dataset = resolutions_dataset(&root, vec![file]);
        dataset.data_type = "QuoteTick".to_string();
        dataset.identifiers = vec![InstrumentId::from("0xCONDITION-YES.POLYMARKET").to_string()];

        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap();

        assert!(manifest.is_complete());
        assert_eq!(manifest.data_configs().unwrap().len(), 1);
    }

    #[rstest]
    fn test_capture_declares_the_files_the_catalog_holds() {
        let temp = TempDir::new().unwrap();
        let catalog = ParquetDataCatalog::new(temp.path(), None, None, None, None);
        let instrument_id = InstrumentId::from("0xCONDITION-YES.POLYMARKET");
        let quotes = (0..3u64)
            .map(|tick| {
                let ts = UnixNanos::from(START + tick * 1_000_000_000);
                QuoteTick::new(
                    instrument_id,
                    Price::from("0.340"),
                    Price::from("0.350"),
                    Quantity::from("1000"),
                    Quantity::from("1000"),
                    ts,
                    ts,
                )
            })
            .collect::<Vec<_>>();
        catalog.write_to_parquet(&quotes, None, None, None).unwrap();

        let dataset = ReplayDataset::capture(
            &catalog,
            NautilusDataType::QuoteTick,
            vec![instrument_id.to_string()],
            UnixNanos::from(START),
            UnixNanos::from(END),
            3,
            Vec::new(),
        )
        .unwrap();

        // The catalog path, the file list, and the digest all come from the capture itself.
        assert_eq!(dataset.catalog_path, catalog.original_uri);
        assert_eq!(dataset.files.len(), 1);
        assert_eq!(dataset.rows, 3);
        assert!(dataset.checksum.starts_with(REPLAY_CHECKSUM_PREFIX));
        assert!(dataset.is_replayable());
        dataset.verify_files().unwrap();

        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap();

        assert!(manifest.is_complete());
        assert_eq!(manifest.data_configs().unwrap().len(), 1);
    }

    #[rstest]
    fn test_capture_of_an_uncaptured_dataset_lists_no_files() {
        let temp = TempDir::new().unwrap();
        let catalog = ParquetDataCatalog::new(temp.path(), None, None, None, None);

        let dataset = ReplayDataset::capture(
            &catalog,
            NautilusDataType::OrderBookDelta,
            Vec::new(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            0,
            vec![ReplayLimitation::NotCaptured {
                reason: "no historical book".to_string(),
            }],
        )
        .unwrap();

        assert!(dataset.files.is_empty());
        assert!(dataset.checksum.is_empty());
        assert!(!dataset.is_replayable());

        let manifest = ReplayManifest::new(
            source(),
            UnixNanos::from(START),
            UnixNanos::from(END),
            vec![dataset],
        )
        .unwrap();

        assert!(!manifest.is_complete());
        assert!(manifest.data_configs().unwrap().is_empty());
    }
}
