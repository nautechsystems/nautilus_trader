// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautilustrader.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Feather-staged promoting writer for the Parquet catalog.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use nautilus_common::live::block_on_nautilus_with;
use object_store::{
    Error as ObjectStoreError, ObjectStoreExt, PutMode, PutOptions, path::Path as ObjectPath,
};
use serde::{Deserialize, Serialize};

use super::catalog::ParquetDataCatalog;
use crate::{
    catalog::factory::CatalogConnectConfig,
    common::{
        conversion::FeatherConversionSummary, datafusion::identifiers_from_record_batches,
        paths::create_local_directory, storage::create_storage_backend_from_path,
    },
    writer::{
        factory::{PARQUET_WRITER_FACTORY_NAME, WriterConnectConfig, WriterFactoryRegistry},
        feather::WriterClock,
        materializer::read_feather_record_batches_with_identity,
        promotion::{
            PromotionBackend, PromotionResult, PromotionSession, PromotionWork,
            StagedPromotionBackend,
        },
        run::{FeatherSessionSource, RunStatus},
        staged::{StagedFeatherWriter, StagedWriter},
        traits::StreamingDataSink,
    },
};

pub(crate) fn register_factory(registry: &mut WriterFactoryRegistry) {
    registry.insert(
        PARQUET_WRITER_FACTORY_NAME.to_string(),
        Arc::new(parquet_writer_factory),
    );
}

fn parquet_writer_factory(
    config: &WriterConnectConfig,
    clock: WriterClock,
) -> anyhow::Result<StreamingDataSink> {
    Ok(Box::new(ParquetWriter::new(config, clock)?))
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "the booleans are independent writer policy and lifecycle flags"
)]
struct ParquetWriter {
    core: StagedFeatherWriter<ParquetPromotionBackend>,
    session: PromotionSession,
    source: FeatherSessionSource,
    catalog: CatalogConnectConfig,
    interval_ms: Option<u64>,
    promote_on_close: bool,
    delete_feather_after_commit: bool,
    use_ts_event_for_ts_init: bool,
    legacy_manifest_missing: Arc<AtomicBool>,
    has_data: bool,
    run_status: RunStatus,
}

impl Debug for ParquetWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ParquetWriter))
            .field("staging_uri", &self.core.storage.original_uri)
            .field("catalog_uri", &self.catalog.uri)
            .field("interval_ms", &self.interval_ms)
            .finish_non_exhaustive()
    }
}

impl ParquetWriter {
    fn new(config: &WriterConnectConfig, clock: WriterClock) -> anyhow::Result<Self> {
        let catalog = config
            .required_catalog(PARQUET_WRITER_FACTORY_NAME)?
            .clone();
        let storage = create_storage_backend_from_path(&config.uri, None)?;
        let session = StagedFeatherWriter::<ParquetPromotionBackend>::required_session(
            &storage.original_uri,
            "Parquet writer URI",
        )?;
        let source_storage = create_storage_backend_from_path(&session.root_uri, None)?;

        let source = FeatherSessionSource::new(
            source_storage,
            session.environment,
            session.instance_id.clone(),
        );

        // A Parquet catalog opens only an existing local directory, so the writer creates the one
        // it promotes into, then opens it now rather than failing at the first promotion
        create_local_directory(&catalog.uri)?;

        let legacy_manifest_missing = Arc::new(AtomicBool::new(false));

        ParquetPromotionBackend::new(&catalog, Arc::clone(&legacy_manifest_missing))?;

        let mut core = StagedFeatherWriter::new(
            storage.clone(),
            &source,
            clock,
            config.rotation_config.clone(),
            None,
            config.flush_interval_ms,
            config.record_filter.clone(),
        )?;

        block_on_nautilus_with(|| {
            storage.write_current_run_manifest(
                session.environment,
                &session.instance_id,
                RunStatus::InProgress,
                true,
            )
        })?;

        let timer_catalog = catalog.clone();
        core.warn_if_orphan_feather_present("Parquet");
        let timer_legacy_manifest_missing = Arc::clone(&legacy_manifest_missing);
        core.start_promotion_timer(
            config.promotion_interval_ms,
            source.clone(),
            "parquet-promotion",
            move || {
                ParquetPromotionBackend::new(
                    &timer_catalog,
                    Arc::clone(&timer_legacy_manifest_missing),
                )
            },
            config.use_ts_event_for_ts_init,
            config.delete_feather_after_promotion,
        )?;

        Ok(Self {
            core,
            catalog,
            session,
            source,
            interval_ms: config.promotion_interval_ms,
            promote_on_close: config.promote_on_close,
            delete_feather_after_commit: config.delete_feather_after_promotion,
            use_ts_event_for_ts_init: config.use_ts_event_for_ts_init,
            legacy_manifest_missing,
            has_data: false,
            run_status: RunStatus::InProgress,
        })
    }

    fn record_status(&mut self, status: RunStatus) -> anyhow::Result<()> {
        if self.run_status == status {
            return Ok(());
        }

        block_on_nautilus_with(|| {
            self.core.storage.write_current_run_manifest(
                self.session.environment,
                &self.session.instance_id,
                status,
                !self.has_data,
            )
        })?;

        self.run_status = status;
        Ok(())
    }

    fn mark_non_empty(&mut self) -> anyhow::Result<()> {
        if self.has_data {
            return Ok(());
        }

        block_on_nautilus_with(|| {
            self.core.storage.write_current_run_manifest(
                self.session.environment,
                &self.session.instance_id,
                RunStatus::InProgress,
                false,
            )
        })?;

        self.has_data = true;
        Ok(())
    }

    fn interval_ns(&self) -> Option<u64> {
        self.interval_ms
            .filter(|interval_ms| *interval_ms > 0)
            .map(|interval_ms| interval_ms.saturating_mul(1_000_000))
    }

    fn prepare_promotion(&self) -> anyhow::Result<Option<PromotionWork<ParquetPromotionBackend>>> {
        let backend =
            ParquetPromotionBackend::new(&self.catalog, Arc::clone(&self.legacy_manifest_missing))?;
        self.core.prepare_promotion(
            backend,
            self.source.clone(),
            self.use_ts_event_for_ts_init,
            self.delete_feather_after_commit,
        )
    }

    fn finalize(
        &mut self,
        result: PromotionResult,
    ) -> anyhow::Result<Vec<FeatherConversionSummary>> {
        if result.converted.is_err() {
            if result.run_state_recorded {
                self.run_status = RunStatus::Failed;
            } else if let Err(e) = self.record_status(RunStatus::Failed) {
                log::warn!("Failed to record Parquet run Failed state: {e}");
            }
        } else if result.run_state_recorded {
            self.run_status = RunStatus::Promoted;
        }

        self.core.finalize_promotion(result)
    }

    fn drain_completed(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>> {
        let mut converted = Vec::new();
        for result in self.core.promotion_driver.drain_completed()? {
            converted.extend(self.finalize(result)?);
        }

        Ok(converted)
    }

    fn wait(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>> {
        let mut converted = Vec::new();
        for result in self.core.promotion_driver.wait()? {
            converted.extend(self.finalize(result)?);
        }

        Ok(converted)
    }

    fn promote_now(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>> {
        let mut converted = self.wait()?;
        if let Some(work) = self.prepare_promotion()? {
            converted.extend(self.finalize(work.execute())?);
        }

        Ok(converted)
    }
}

impl StagedWriter for ParquetWriter {
    type Backend = ParquetPromotionBackend;

    fn staged(&mut self) -> &mut StagedFeatherWriter<Self::Backend> {
        &mut self.core
    }

    fn mark_non_empty(&mut self) -> anyhow::Result<()> {
        Self::mark_non_empty(self)
    }

    fn maybe_promote(&mut self) -> anyhow::Result<()> {
        self.drain_completed()?;

        let Some(interval_ns) = self.interval_ns() else {
            return Ok(());
        };

        if self
            .core
            .promotion_driver
            .is_due(self.core.clock.timestamp_ns(), interval_ns)
            && let Some(work) = self.prepare_promotion()?
        {
            self.core
                .promotion_driver
                .submit(work, "parquet-promotion")?;
            self.core
                .promotion_driver
                .mark_committed_at(self.core.clock.timestamp_ns());
        }

        Ok(())
    }

    fn wait_for_promotions(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>> {
        self.wait()
    }

    fn promote_on_flush(&self) -> bool {
        self.interval_ns().is_some_and(|interval_ns| {
            self.core
                .promotion_driver
                .is_due(self.core.clock.timestamp_ns(), interval_ns)
        })
    }

    fn promote_on_close(&self) -> bool {
        self.promote_on_close
    }

    fn promote_now(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>> {
        Self::promote_now(self)
    }

    fn record_completed(&mut self) {
        if self.run_status == RunStatus::InProgress
            && let Err(e) = self.record_status(RunStatus::Completed)
        {
            log::warn!("Failed to complete Parquet run manifest: {e}");
        }
    }
}

impl Drop for ParquetWriter {
    fn drop(&mut self) {
        self.core.stop_promotion_timer();

        let promotion_failed = if let Err(e) = self.wait() {
            log::warn!("ParquetWriter dropped with pending promotion error: {e}");
            true
        } else {
            false
        };

        if self.run_status == RunStatus::InProgress {
            let status = if promotion_failed || !self.core.is_closed().unwrap_or(false) {
                RunStatus::Failed
            } else {
                RunStatus::Completed
            };

            if let Err(e) = self.record_status(status) {
                log::warn!(
                    "Failed to record Parquet run {} state during drop: {e}",
                    status.as_str()
                );
            }
        }
    }
}

const PROMOTION_MANIFEST: &str = "_nautilus_promotions.json";
const PROMOTION_MARKERS: &str = "_nautilus_promotions";

struct ParquetPromotionBackend {
    catalog: ParquetDataCatalog,
    legacy_manifest_missing: Arc<AtomicBool>,
}

impl ParquetPromotionBackend {
    fn new(
        catalog: &CatalogConnectConfig,
        legacy_manifest_missing: Arc<AtomicBool>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            catalog: super::open_catalog(catalog)?,
            legacy_manifest_missing,
        })
    }

    fn manifest_path(&self) -> ObjectPath {
        self.catalog_path(PROMOTION_MANIFEST)
    }

    fn manifest(&self) -> anyhow::Result<ParquetPromotionManifest> {
        if self.legacy_manifest_missing.load(Ordering::Relaxed) {
            return Ok(ParquetPromotionManifest::default());
        }

        let path = self.manifest_path();
        block_on_nautilus_with(|| async {
            let result = match self.catalog.object_store.get(&path).await {
                Ok(result) => result,
                Err(ObjectStoreError::NotFound { .. }) => {
                    self.legacy_manifest_missing.store(true, Ordering::Relaxed);
                    return Ok(ParquetPromotionManifest::default());
                }
                Err(e) => return Err(e.into()),
            };

            Ok(serde_json::from_slice(&result.bytes().await?)?)
        })
    }

    fn marker_path(&self, identity: &str) -> ObjectPath {
        let digest = blake3::hash(identity.as_bytes()).to_hex();
        self.catalog_path(&format!("{PROMOTION_MARKERS}/{digest}.json"))
    }

    fn catalog_path(&self, path: &str) -> ObjectPath {
        let base = self.catalog.base_path.trim_matches('/');
        if base.is_empty() {
            ObjectPath::from(path)
        } else {
            ObjectPath::from(format!("{base}/{path}"))
        }
    }

    fn identity_recorded(&self, identity: &str) -> anyhow::Result<bool> {
        let path = self.marker_path(identity);

        let marker_exists = block_on_nautilus_with(|| async {
            match self.catalog.object_store.head(&path).await {
                Ok(_) => Ok::<bool, anyhow::Error>(true),
                Err(ObjectStoreError::NotFound { .. }) => Ok(false),
                Err(e) => Err(anyhow::Error::from(e)),
            }
        })?;

        Ok(marker_exists
            || self
                .manifest()?
                .identities
                .iter()
                .any(|item| item == identity))
    }

    fn record_identity(&self, identity: &str) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(identity)?;
        let path = self.marker_path(identity);
        block_on_nautilus_with(|| async {
            match self
                .catalog
                .object_store
                .put_opts(
                    &path,
                    bytes.into(),
                    PutOptions {
                        mode: PutMode::Create,
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(_) | Err(ObjectStoreError::AlreadyExists { .. }) => Ok(()),
                Err(e) => Err(e.into()),
            }
        })
    }
}

impl PromotionBackend for ParquetPromotionBackend {
    type Source = FeatherSessionSource;

    const NAME: &'static str = "Parquet";

    fn convert_file(
        &mut self,
        source: &Self::Source,
        file: &str,
        use_ts_event_for_ts_init: bool,
        record_promoted: bool,
    ) -> anyhow::Result<Option<FeatherConversionSummary>> {
        let object_path = ObjectPath::parse(file)?;

        let read = block_on_nautilus_with(|| {
            read_feather_record_batches_with_identity(
                source.storage.object_store.clone(),
                &object_path,
            )
        })?;

        let identifiers = identifiers_from_record_batches(&read.batches)
            .ok()
            .filter(|identifiers| !identifiers.is_empty());
        let identity = feather_replay_identity(
            &source.storage.original_uri,
            file,
            &read.content_hash,
            identifiers.as_deref(),
        );

        if self.identity_recorded(&identity)? {
            return Ok(None);
        }

        let summary = self.catalog.promote_feather_file(
            source,
            file,
            read.batches,
            use_ts_event_for_ts_init,
            &identity,
        )?;

        if summary.is_some() && record_promoted {
            self.record_identity(&identity)?;
        }

        Ok(summary)
    }

    fn delete_file(&mut self, source: &Self::Source, file: &str) -> anyhow::Result<()> {
        let object_path = ObjectPath::parse(file)?;
        block_on_nautilus_with(|| async {
            source.storage.object_store.delete(&object_path).await?;
            Ok::<(), anyhow::Error>(())
        })
    }
}

impl StagedPromotionBackend for ParquetPromotionBackend {
    const REQUIRES_SESSION: bool = true;

    fn record_run_state(
        &mut self,
        source: &FeatherSessionSource,
        _staging_uri: &str,
        status: RunStatus,
        empty: bool,
        _error: Option<&str>,
    ) -> anyhow::Result<()> {
        block_on_nautilus_with(|| {
            source.storage.write_run_manifest(
                source.environment,
                &source.instance_id,
                status,
                empty,
            )
        })
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct ParquetPromotionManifest {
    identities: Vec<String>,
}

fn feather_replay_identity(
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::AtomicU64,
    };

    use arrow::datatypes::SchemaRef;
    use nautilus_common::enums::Environment;
    use nautilus_core::{Params, UnixNanos};
    use nautilus_model::{
        data::{Data, DataBatch, NautilusDataType, NautilusRecordType, QuoteTick},
        identifiers::InstrumentId,
        types::{ERROR_PRICE, Price, Quantity},
    };
    use object_store::memory::InMemory;
    use parquet::{
        basic::Compression,
        file::{
            metadata::RowGroupMetaData,
            reader::{FileReader, SerializedFileReader},
        },
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        backend::parquet::io::read_parquet_from_object_store,
        catalog::traits::{CatalogQuery, CatalogReader},
        common::{paths::normalize_path_to_uri, storage::RUN_MANIFEST_FILENAME},
        config::{CatalogCompression, DataCatalogConfig},
        test_data::RustTestHashMapCustomData,
        writer::{
            feather::{FEATHER_PARTIAL_EXTENSION, FeatherWriter, RotationConfig},
            promotion::{list_session_feather_files, tests::assert_scheduled_count},
        },
    };

    #[rstest]
    fn parquet_default_close_promotes_and_honors_source_retention(
        #[values(false, true)] delete_source: bool,
    ) {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-1");
        let mut config =
            WriterConnectConfig::new(staging.to_string_lossy(), Some(local_catalog(&directory)));
        config.delete_feather_after_promotion = delete_source;
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();
        let quote = sample_quote();
        let mut second = quote;
        second.instrument_id = InstrumentId::from("BTC/USD.SIM");
        second.bid_price = Price::from("100.1234");
        second.ask_price = Price::from("100.5678");
        second.ts_init = UnixNanos::from(24);
        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        sink.write_data(Data::Quote(quote)).unwrap();
        sink.write_data(Data::Quote(second)).unwrap();
        sink.flush().unwrap();
        let before = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap();
        sink.close().unwrap();
        let after = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap();
        let storage = create_storage_backend_from_path(staging.to_str().unwrap(), None).unwrap();
        let staged = block_on_nautilus_with(|| storage.list_files("", None))
            .unwrap()
            .into_iter()
            .filter(|path| path.ends_with(".feather"))
            .count();
        assert!(before.is_empty());

        let DataBatch::Quote(rows) = after else {
            panic!("expected quotes")
        };

        assert_eq!(rows.as_ref(), &[quote, second]);
        assert_eq!(staged, usize::from(!delete_source));
    }

    #[rstest]
    fn parquet_writer_recovers_partial_files_at_startup() {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-recover");
        let quote = sample_quote();

        // A writer that exited before sealing leaves its flushed stream as a partial file
        let mut crashed = FeatherWriter::new(
            staging.clone(),
            WriterClock::Test(Arc::new(AtomicU64::new(0))),
            RotationConfig::NoRotation,
            None,
            None,
        );
        crashed.write(quote).unwrap();
        crashed.close().unwrap();
        let sealed = staging.join("quotes").join("quotes_0.feather");
        std::fs::rename(&sealed, sealed.with_extension(FEATHER_PARTIAL_EXTENSION)).unwrap();

        let config =
            WriterConnectConfig::new(staging.to_string_lossy(), Some(local_catalog(&directory)));
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();
        sink.close().unwrap();

        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let rows = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap();

        let DataBatch::Quote(rows) = rows else {
            panic!("expected quotes")
        };

        assert_eq!(rows.as_ref(), &[quote]);
    }

    #[rstest]
    fn parquet_close_promotes_per_identifier_file_with_non_ascii_identifier(
        #[values(false, true)] delete_source: bool,
    ) {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-1");
        let (quote, staged_file) = stage_per_identifier_quote(&staging);
        let mut config =
            WriterConnectConfig::new(staging.to_string_lossy(), Some(local_catalog(&directory)));
        config.delete_feather_after_promotion = delete_source;
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();

        sink.close().unwrap();

        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let DataBatch::Quote(rows) = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap()
        else {
            panic!("expected quotes")
        };

        assert_eq!(rows.as_ref(), &[quote]);
        assert_eq!(staged_file.exists(), !delete_source);
    }

    #[rstest]
    fn parquet_manual_conversion_reads_per_identifier_file_with_non_ascii_identifier() {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-1");
        let (quote, _) = stage_per_identifier_quote(&staging);
        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        catalog
            .convert_stream_to_data(
                "run-1",
                &NautilusDataType::QuoteTick.into(),
                Environment::Backtest,
                None,
                false,
            )
            .unwrap();

        let DataBatch::Quote(rows) = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap()
        else {
            panic!("expected quotes")
        };

        assert_eq!(rows.as_ref(), &[quote]);
    }

    #[rstest]
    fn parquet_promotion_writes_non_ascii_identifier_to_object_store() {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-1");
        let (quote, staged_file) = stage_per_identifier_quote(&staging);

        let source = FeatherSessionSource::new(
            create_storage_backend_from_path(directory.path().to_str().unwrap(), None).unwrap(),
            Environment::Backtest,
            "run-1",
        );
        let mut catalog = ParquetDataCatalog::new(directory.path(), None, None, None, None);
        catalog.base_path = "catalog".to_string();
        catalog.original_uri = "s3://test-bucket/catalog".to_string();
        catalog.object_store = Arc::new(InMemory::new());

        let mut backend = ParquetPromotionBackend {
            catalog,
            legacy_manifest_missing: Arc::default(),
        };

        let files =
            list_session_feather_files(&source.storage, Environment::Backtest, "run-1").unwrap();

        for file in &files {
            backend.convert_file(&source, file, false, true).unwrap();
            backend.delete_file(&source, file).unwrap();
        }

        let DataBatch::Quote(rows) = backend
            .catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap()
        else {
            panic!("expected quotes")
        };

        assert_eq!(files.len(), 1);
        assert_eq!(rows.as_ref(), &[quote]);
        assert!(!staged_file.exists());
    }

    #[rstest]
    #[case("backtest")]
    #[case("sandbox")]
    #[case("live")]
    fn parquet_close_promotes_into_missing_local_catalog(#[case] environment: &str) {
        let directory = TempDir::new().unwrap();
        let staging = directory
            .path()
            .join("stream")
            .join(environment)
            .join("run-1");
        let catalog_path = directory.path().join("charts").join("catalog");

        let config = WriterConnectConfig::new(
            staging.to_string_lossy(),
            Some(CatalogConnectConfig::new(
                catalog_path.to_string_lossy(),
                None,
            )),
        );
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();
        let quote = sample_quote();

        sink.write_data(Data::Quote(quote)).unwrap();
        sink.close().unwrap();

        let mut catalog =
            ParquetDataCatalog::from_uri(catalog_path.to_str().unwrap(), None, None, None, None)
                .unwrap();

        let DataBatch::Quote(rows) = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap()
        else {
            panic!("expected quotes")
        };

        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(staging.join(RUN_MANIFEST_FILENAME)).unwrap())
                .unwrap();

        assert_eq!(rows.as_ref(), &[quote]);
        assert_eq!(manifest["kind"], environment);
        assert_eq!(manifest["status"], "promoted");
        assert!(!catalog_path.join(environment).exists());
    }

    #[rstest]
    #[case::non_ascii("run-é")]
    #[case::space("run 1")]
    #[case::fragment("run#1")]
    fn parquet_writer_rejects_run_id_that_paths_encode(#[case] run_id: &str) {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join(run_id);
        let config =
            WriterConnectConfig::new(staging.to_string_lossy(), Some(local_catalog(&directory)));

        let error = parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
            .unwrap_err();

        let staging_uri = normalize_path_to_uri(&staging.to_string_lossy()).unwrap();
        assert_eq!(
            error.to_string(),
            format!(
                "Parquet writer URI '{staging_uri}' has a run ID that object-store paths \
                 percent-encode; use a run ID without spaces, non-ASCII, or reserved characters",
            )
        );
    }

    #[rstest]
    #[case::windows_separators(r"file:///C:\catalog\backtest\run-1", "file:///C:/catalog")]
    #[case::windows_trailing_separator(r"file:///C:\catalog\backtest\run-1\", "file:///C:/catalog")]
    #[case::trailing_separator("file:///tmp/catalog/backtest/run-1/", "file:///tmp/catalog")]
    #[case::parent_components(
        "file:///tmp/unused/../catalog/backtest/run-1",
        "file:///tmp/catalog"
    )]
    fn parquet_writer_session_accepts_separator_forms(#[case] uri: &str, #[case] root_uri: &str) {
        let session = StagedFeatherWriter::<ParquetPromotionBackend>::required_session(
            uri,
            "Parquet writer URI",
        )
        .unwrap();

        assert_eq!(
            session,
            PromotionSession {
                root_uri: root_uri.to_string(),
                environment: Environment::Backtest,
                instance_id: "run-1".to_string(),
            }
        );
    }

    #[rstest]
    fn parquet_writer_requires_a_catalog() {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-1");
        let config = WriterConnectConfig::new(staging.to_string_lossy(), None);

        let error = parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Parquet writer requires a promotion catalog"
        );
        assert!(!staging.exists());
    }

    // Opening an S3 store needs no network, and a run with no data promotes nothing, so the
    // writer records the run as completed when dropped
    #[cfg(feature = "cloud")]
    #[rstest]
    fn parquet_writer_stages_locally_for_a_remote_catalog() {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-1");

        let config = WriterConnectConfig::new(
            staging.to_string_lossy(),
            Some(CatalogConnectConfig::new("s3://bucket/catalog", None)),
        );

        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();
        sink.close().unwrap();
        drop(sink);

        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(staging.join(RUN_MANIFEST_FILENAME)).unwrap())
                .unwrap();
        assert_eq!(manifest["kind"], "backtest");
        assert_eq!(manifest["instance_id"], "run-1");
        assert_eq!(manifest["status"], "completed");
        assert_eq!(manifest["empty"], true);
    }

    #[rstest]
    fn parquet_promotion_writes_the_direct_write_schema() {
        let directory = TempDir::new().unwrap();

        let config = WriterConnectConfig::new(
            directory
                .path()
                .join("backtest/run-schema")
                .to_string_lossy(),
            Some(local_catalog(&directory)),
        );
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();
        let quote = sample_quote();
        sink.write_data(Data::Quote(quote)).unwrap();
        sink.close().unwrap();

        let promoted = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let direct_directory = TempDir::new().unwrap();
        let direct = ParquetDataCatalog::from_uri(
            direct_directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        direct.write_to_parquet(&[quote], None, None, None).unwrap();

        assert_eq!(
            quote_file_schema(&promoted).fields(),
            quote_file_schema(&direct).fields(),
        );
    }

    #[rstest]
    fn parquet_promotion_honors_catalog_settings() {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-1");
        let catalog = DataCatalogConfig::builder()
            .path(directory.path().to_string_lossy().to_string())
            .batch_size(7)
            .compression(CatalogCompression::Uncompressed)
            .max_row_group_size(1)
            .build()
            .unwrap()
            .connect_config();
        let backend =
            ParquetPromotionBackend::new(&catalog, Arc::new(AtomicBool::new(false))).unwrap();

        let config = WriterConnectConfig::new(staging.to_string_lossy(), Some(catalog));
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();
        let quote = sample_quote();
        sink.write_data(Data::Quote(quote)).unwrap();
        sink.write_data(Data::Quote(QuoteTick {
            ts_event: UnixNanos::from(29),
            ts_init: UnixNanos::from(31),
            ..quote
        }))
        .unwrap();

        sink.close().unwrap();

        let files = ParquetDataCatalog::new(directory.path(), None, None, None, None)
            .query_files(&NautilusDataType::QuoteTick.into(), None, None, None)
            .unwrap();
        let reader = SerializedFileReader::new(
            std::fs::File::open(directory.path().join(&files[0])).unwrap(),
        )
        .unwrap();
        let metadata = reader.metadata();

        assert_eq!(backend.catalog.batch_size, 7);
        assert_eq!(files.len(), 1);
        assert_eq!(metadata.num_row_groups(), 2);
        assert!(
            metadata
                .row_groups()
                .iter()
                .flat_map(RowGroupMetaData::columns)
                .all(|column| column.compression() == Compression::UNCOMPRESSED)
        );
    }

    #[rstest]
    fn parquet_writer_rejects_unknown_catalog_param() {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-1");
        let mut catalog = local_catalog(&directory);
        let mut params = Params::new();
        params.insert("compression".to_string(), serde_json::json!(0));
        catalog.params = Some(params);

        let config = WriterConnectConfig::new(staging.to_string_lossy(), Some(catalog));
        let error = parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown Parquet catalog param 'compression': this catalog takes no params"
        );
    }

    fn quote_file_schema(catalog: &ParquetDataCatalog) -> SchemaRef {
        let files = catalog
            .query_files(&NautilusDataType::QuoteTick.into(), None, None, None)
            .unwrap();
        assert_eq!(files.len(), 1);

        let path = ObjectPath::from(files[0].as_str());

        let (_, schema) = block_on_nautilus_with(|| {
            read_parquet_from_object_store(catalog.object_store.clone(), &path)
        })
        .unwrap();

        schema
    }

    #[rstest]
    #[case(None)]
    #[case(Some(0))]
    #[case(Some(1))]
    fn parquet_interval_uses_test_clock_and_close_can_leave_staged_data(
        #[case] interval: Option<u64>,
    ) {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-2");
        let mut config =
            WriterConnectConfig::new(staging.to_string_lossy(), Some(local_catalog(&directory)));
        config.promotion_interval_ms = interval;
        config.promote_on_close = false;
        let clock = Arc::new(AtomicU64::new(0));
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::clone(&clock))).unwrap();
        let quote = sample_quote();
        sink.write_data(Data::Quote(quote)).unwrap();
        clock.store(1_000_000, Ordering::Relaxed);
        sink.flush().unwrap();
        sink.close().unwrap();
        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let before_manual = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap();

        if interval != Some(1) {
            catalog
                .convert_stream_to_data(
                    "run-2",
                    &NautilusDataType::QuoteTick.into(),
                    Environment::Backtest,
                    None,
                    false,
                )
                .unwrap();
        }

        let after = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap();
        assert_eq!(before_manual.len(), usize::from(interval == Some(1)));

        let DataBatch::Quote(rows) = after else {
            panic!("expected quotes")
        };

        assert_eq!(rows.as_ref(), &[quote]);
    }

    #[rstest]
    fn parquet_streaming_funding_round_trip() {
        use nautilus_model::data::FundingRateUpdate;
        let directory = TempDir::new().unwrap();

        let config = WriterConnectConfig::new(
            directory
                .path()
                .join("backtest/run-funding")
                .to_string_lossy(),
            Some(local_catalog(&directory)),
        );
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();

        let funding = FundingRateUpdate::new(
            InstrumentId::from("AUD/USD.SIM"),
            "0.0012".parse().unwrap(),
            Some(480),
            Some(789.into()),
            123.into(),
            456.into(),
        );
        assert!(sink.write_any(&funding).unwrap());
        sink.close().unwrap();
        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let rows = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::FundingRateUpdate))
            .unwrap();

        let DataBatch::FundingRate(rows) = rows else {
            panic!("expected funding rates")
        };

        assert_eq!(rows.as_ref(), &[funding]);
    }

    #[rstest]
    fn parquet_streaming_write_error_reaches_flush() {
        let directory = TempDir::new().unwrap();

        let config = WriterConnectConfig::new(
            directory
                .path()
                .join("backtest/run-error")
                .to_string_lossy(),
            Some(local_catalog(&directory)),
        );
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();
        let mut quote = sample_quote();
        quote.bid_price = ERROR_PRICE;
        let write_error = sink.write_any(&quote).unwrap_err().to_string();
        let flush_error = sink.flush().unwrap_err().to_string();
        assert_eq!(flush_error, write_error);
    }

    #[rstest]
    fn parquet_streaming_instrument_promotes() {
        use nautilus_model::instruments::{InstrumentAny, stubs::audusd_sim};
        let directory = TempDir::new().unwrap();

        let config = WriterConnectConfig::new(
            directory
                .path()
                .join("backtest/run-instrument")
                .to_string_lossy(),
            Some(local_catalog(&directory)),
        );
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        assert!(sink.write_any(&instrument).unwrap());
        sink.close().unwrap();
        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let rows = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::Instrument))
            .unwrap();

        let DataBatch::Instrument(rows) = rows else {
            panic!("expected instruments")
        };

        assert_eq!(rows.as_ref(), &[instrument]);
    }

    #[rstest]
    fn parquet_streaming_voided_fill_promotes() {
        use nautilus_model::events::{OrderFillVoided, order::spec::OrderFillVoidedSpec};
        use nautilus_serialization::arrow::DecodeTypedFromRecordBatch;
        let directory = TempDir::new().unwrap();

        let config = WriterConnectConfig::new(
            directory.path().join("backtest/run-void").to_string_lossy(),
            Some(local_catalog(&directory)),
        );
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();
        let event = OrderFillVoidedSpec::builder().is_reopened(true).build();
        assert!(sink.write_any(&event).unwrap());
        sink.close().unwrap();
        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let batches = catalog
            .query_record_batches(
                &NautilusRecordType::OrderFillVoided.into(),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        let mut rows = Vec::new();
        for batch in batches {
            rows.extend(
                OrderFillVoided::decode_typed_batch(batch.schema().metadata(), batch.clone())
                    .unwrap(),
            );
        }

        assert_eq!(rows, vec![event]);
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn parquet_promotion_unifies_empty_and_populated_depth(#[case] automatic: bool) {
        use nautilus_model::{
            data::{BookOrder, OrderBookDepth},
            enums::OrderSide,
        };
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest/run-depth-ties");
        let mut config =
            WriterConnectConfig::new(staging.to_string_lossy(), Some(local_catalog(&directory)));
        config.delete_feather_after_promotion = true;
        config.promote_on_close = automatic;
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();
        let id = InstrumentId::from("AUD/USD.SIM");
        let empty =
            OrderBookDepth::new(id, vec![], vec![], vec![], vec![], 1, 2, 3.into(), 4.into());
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.23"),
            Quantity::from("4.5"),
            6,
        );
        let populated = OrderBookDepth::new(
            id,
            vec![order],
            vec![],
            vec![7],
            vec![],
            8,
            9,
            3.into(),
            4.into(),
        );
        sink.write_any(&empty).unwrap();
        sink.write_any(&populated).unwrap();
        sink.close().unwrap();

        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        if !automatic {
            catalog
                .convert_stream_to_data(
                    "run-depth-ties",
                    &NautilusDataType::OrderBookDepth.into(),
                    Environment::Backtest,
                    None,
                    false,
                )
                .unwrap();
        }

        let batch = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::OrderBookDepth))
            .unwrap();
        let DataBatch::BookDepth(rows) = batch else {
            panic!("expected order book depth");
        };
        let mut rows = rows.as_ref().to_vec();
        rows.sort_by_key(|depth| depth.sequence);
        assert_eq!(rows, vec![empty, populated]);

        let storage = create_storage_backend_from_path(staging.to_str().unwrap(), None).unwrap();
        let staged = block_on_nautilus_with(|| storage.list_files("", Some(".feather"))).unwrap();
        assert_eq!(staged.len(), usize::from(!automatic));
    }

    #[rstest]
    fn parquet_manual_conversion_promotes_custom_data() {
        use nautilus_model::data::{CustomData, DataType};
        use nautilus_serialization::ensure_custom_data_registered;

        ensure_custom_data_registered::<RustTestHashMapCustomData>();

        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-custom");
        let mut config =
            WriterConnectConfig::new(staging.to_string_lossy(), Some(local_catalog(&directory)));
        config.promote_on_close = false;
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();

        let data_type = DataType::new("RustTestHashMapCustomData", None, None);
        let records = [
            sample_custom_data("first", "AUD/USD.SIM", "1.23456", 11),
            sample_custom_data("second", "BTCUSDT.BINANCE", "65432.10", 21),
        ];

        for record in &records {
            sink.write_data(Data::Custom(CustomData::new(
                Arc::new(record.clone()),
                data_type.clone(),
            )))
            .unwrap();
        }

        sink.close().unwrap();

        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        for _ in 0..2 {
            catalog
                .convert_stream_to_data(
                    "run-custom",
                    &NautilusDataType::Custom {
                        type_name: "RustTestHashMapCustomData".to_string(),
                    }
                    .into(),
                    Environment::Backtest,
                    None,
                    false,
                )
                .unwrap();
        }

        let expected: Vec<(Option<String>, RustTestHashMapCustomData)> = records
            .iter()
            .map(|record| (None, record.clone()))
            .collect();
        assert_eq!(query_custom_records(&mut catalog, None), expected);
    }

    #[rstest]
    fn parquet_manual_conversion_filters_custom_data_identifiers() {
        use nautilus_model::data::{CustomData, DataType};
        use nautilus_serialization::ensure_custom_data_registered;

        ensure_custom_data_registered::<RustTestHashMapCustomData>();

        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-custom-ids");
        let mut config =
            WriterConnectConfig::new(staging.to_string_lossy(), Some(local_catalog(&directory)));
        config.promote_on_close = false;
        let mut sink =
            parquet_writer_factory(&config, WriterClock::Test(Arc::new(AtomicU64::new(0))))
                .unwrap();

        let audusd = "AUD/USD.SIM";
        let btcusdt = "BTCUSDT.BINANCE";
        let records = [
            (audusd, sample_custom_data("first", audusd, "1.23456", 11)),
            (audusd, sample_custom_data("second", audusd, "1.23457", 21)),
            (
                btcusdt,
                sample_custom_data("third", btcusdt, "65432.10", 31),
            ),
        ];

        for (identifier, record) in &records {
            let data_type = DataType::new(
                "RustTestHashMapCustomData",
                None,
                Some((*identifier).to_string()),
            );
            sink.write_data(Data::Custom(CustomData::new(
                Arc::new(record.clone()),
                data_type,
            )))
            .unwrap();
        }

        sink.close().unwrap();

        let mut catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        catalog
            .convert_stream_to_data(
                "run-custom-ids",
                &NautilusDataType::Custom {
                    type_name: "RustTestHashMapCustomData".to_string(),
                }
                .into(),
                Environment::Backtest,
                Some(&[audusd.to_string()]),
                false,
            )
            .unwrap();

        let expected_audusd: Vec<(Option<String>, RustTestHashMapCustomData)> = records
            .iter()
            .filter(|(identifier, _)| *identifier == audusd)
            .map(|(identifier, record)| (Some((*identifier).to_string()), record.clone()))
            .collect();
        assert_eq!(query_custom_records(&mut catalog, None), expected_audusd);

        // Catalog identifier directories are urisafe, so a scoped query pins that spelling and
        // proves the converted records landed under their own identifier rather than together.
        assert_eq!(
            query_custom_records(&mut catalog, Some(&["AUDUSD.SIM".to_string()])),
            expected_audusd,
        );

        catalog
            .convert_stream_to_data(
                "run-custom-ids",
                &NautilusDataType::Custom {
                    type_name: "RustTestHashMapCustomData".to_string(),
                }
                .into(),
                Environment::Backtest,
                None,
                false,
            )
            .unwrap();

        let expected_all: Vec<(Option<String>, RustTestHashMapCustomData)> = records
            .iter()
            .map(|(identifier, record)| (Some((*identifier).to_string()), record.clone()))
            .collect();
        assert_eq!(query_custom_records(&mut catalog, None), expected_all);
        let expected_btcusdt: Vec<(Option<String>, RustTestHashMapCustomData)> = records
            .iter()
            .filter(|(identifier, _)| *identifier == btcusdt)
            .map(|(identifier, record)| (Some((*identifier).to_string()), record.clone()))
            .collect();
        assert_eq!(
            query_custom_records(&mut catalog, Some(&[btcusdt.to_string()])),
            expected_btcusdt,
        );
    }

    fn sample_custom_data(
        name: &str,
        instrument_id: &str,
        price: &str,
        ts: u64,
    ) -> RustTestHashMapCustomData {
        RustTestHashMapCustomData {
            name: name.to_string(),
            prices: [(instrument_id.to_string(), Price::from(price))]
                .into_iter()
                .collect(),
            ts_event: UnixNanos::from(ts - 1),
            ts_init: UnixNanos::from(ts),
        }
    }

    fn query_custom_records(
        catalog: &mut ParquetDataCatalog,
        identifiers: Option<&[String]>,
    ) -> Vec<(Option<String>, RustTestHashMapCustomData)> {
        let loaded = catalog
            .query_custom_data_dynamic(
                "RustTestHashMapCustomData",
                identifiers,
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        let mut decoded = Vec::new();

        for data in &loaded {
            let Data::Custom(custom) = data else {
                panic!("expected custom data, found {data:?}")
            };

            assert_eq!(custom.data_type.type_name(), "RustTestHashMapCustomData");
            decoded.push((
                custom.data_type.identifier().map(ToString::to_string),
                custom
                    .data
                    .as_any()
                    .downcast_ref::<RustTestHashMapCustomData>()
                    .expect("expected RustTestHashMapCustomData")
                    .clone(),
            ));
        }

        decoded.sort_by_key(|(_, record)| record.ts_init);
        decoded
    }

    fn local_catalog(directory: &TempDir) -> CatalogConnectConfig {
        CatalogConnectConfig::new(directory.path().to_string_lossy(), None)
    }

    // Earlier per-instrument writers staged files in identifier directories
    fn stage_per_identifier_quote(staging: &Path) -> (QuoteTick, PathBuf) {
        let mut quote = sample_quote();
        quote.instrument_id = InstrumentId::from("CAFÉ.SIM");

        let mut writer = FeatherWriter::new(
            staging.to_path_buf(),
            WriterClock::Test(Arc::new(AtomicU64::new(0))),
            RotationConfig::NoRotation,
            None,
            None,
        );
        writer.write(quote).unwrap();
        writer.close().unwrap();

        let identifier_directory = staging.join("quotes").join("CAFÉ.SIM");
        let staged_file = identifier_directory.join("quotes_0.feather");
        fs::create_dir(&identifier_directory).unwrap();
        fs::rename(
            staging.join("quotes").join("quotes_0.feather"),
            &staged_file,
        )
        .unwrap();

        (quote, staged_file)
    }

    fn sample_quote() -> QuoteTick {
        QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("0.65"),
            Price::from("0.67"),
            Quantity::from("11"),
            Quantity::from("17"),
            UnixNanos::from(19),
            UnixNanos::from(23),
        )
    }

    #[rstest]
    fn kept_promotions_release_scheduled_paths() {
        let directory = TempDir::new().unwrap();
        let config = WriterConnectConfig::new(
            directory.path().join("live/run-bounded").to_string_lossy(),
            Some(local_catalog(&directory)),
        );
        let mut writer = ParquetWriter::new(&config, WriterClock::Live).unwrap();

        for timestamp in 1..=32 {
            writer
                .core
                .write_data(Data::Quote(QuoteTick {
                    ts_event: UnixNanos::from(timestamp),
                    ts_init: UnixNanos::from(timestamp),
                    ..sample_quote()
                }))
                .unwrap();
            let work = writer.prepare_promotion().unwrap().unwrap();
            assert_eq!(work.files().len(), 1);
            writer.finalize(work.execute()).unwrap();
            assert_scheduled_count(&writer.core.promotion_driver.schedule(), 0);
            assert!(writer.prepare_promotion().unwrap().is_none());
        }

        assert_eq!(
            list_session_feather_files(&writer.source.storage, Environment::Live, "run-bounded")
                .unwrap()
                .len(),
            32
        );
    }

    #[rstest]
    #[case::timer_seal(RotationConfig::NoRotation, false, 1)]
    #[case::close(RotationConfig::NoRotation, true, 1)]
    #[case::rotation(RotationConfig::Size { max_size: 1 }, false, 2)]
    fn sealed_paths_promote_once_and_do_not_resubmit_in_flight(
        #[case] rotation: RotationConfig,
        #[case] close: bool,
        #[case] expected_files: usize,
    ) {
        let directory = TempDir::new().unwrap();
        let mut config = WriterConnectConfig::new(
            directory.path().join("live/run-sealing").to_string_lossy(),
            Some(local_catalog(&directory)),
        );
        config.rotation_config = rotation;
        let mut writer = ParquetWriter::new(&config, WriterClock::Live).unwrap();
        let first = sample_quote();
        let second = QuoteTick {
            ts_event: UnixNanos::from(29),
            ts_init: UnixNanos::from(31),
            ..first
        };
        writer.core.write_data(Data::Quote(first)).unwrap();
        writer.core.write_data(Data::Quote(second)).unwrap();

        if close {
            writer.core.close().unwrap();
        }

        let work = writer.prepare_promotion().unwrap().unwrap();
        let files = work.files().to_vec();
        assert_eq!(files.len(), expected_files);
        assert!(writer.prepare_promotion().unwrap().is_none());
        assert_scheduled_count(&writer.core.promotion_driver.schedule(), expected_files);
        let result = work.execute();
        assert_scheduled_count(&writer.core.promotion_driver.schedule(), 0);
        assert_eq!(result.committed_paths, files);
        assert_eq!(writer.finalize(result).unwrap().len(), expected_files);
        assert!(writer.prepare_promotion().unwrap().is_none());

        let mut catalog = ParquetDataCatalog::new(directory.path(), None, None, None, None);
        let DataBatch::Quote(rows) = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap()
        else {
            panic!("expected quotes")
        };
        assert_eq!(rows.as_ref(), &[first, second]);
    }

    #[rstest]
    #[case::space("my data")]
    #[case::non_ascii("Données")]
    #[case::fragment("data#1")]
    #[case::literal_percent("data%20")]
    fn writer_root_path_promotes_and_replays(
        #[case] root_name: &str,
        #[values(false, true)] file_uri: bool,
    ) {
        let directory = TempDir::new().unwrap();
        let root = directory.path().join(root_name);
        let staging = root.join("live/run-root");
        let staging_uri = if file_uri {
            normalize_path_to_uri(&staging.to_string_lossy()).unwrap()
        } else {
            staging.to_string_lossy().into_owned()
        };
        let config = WriterConnectConfig::new(staging_uri, Some(local_catalog(&directory)));
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));
        let quote = sample_quote();
        let mut writer = ParquetWriter::new(&config, clock.clone()).unwrap();
        writer.core.write_data(Data::Quote(quote)).unwrap();
        let work = writer.prepare_promotion().unwrap().unwrap();
        assert_eq!(work.files(), &["live/run-root/quotes/quotes_0.feather"]);
        assert_eq!(writer.finalize(work.execute()).unwrap().len(), 1);
        assert!(writer.prepare_promotion().unwrap().is_none());
        drop(writer);

        let mut restarted = ParquetWriter::new(&config, clock).unwrap();
        let work = restarted.prepare_promotion().unwrap().unwrap();
        assert_eq!(work.files(), &["live/run-root/quotes/quotes_0.feather"]);
        assert!(restarted.finalize(work.execute()).unwrap().is_empty());
        assert!(restarted.prepare_promotion().unwrap().is_none());

        let mut catalog = ParquetDataCatalog::new(directory.path(), None, None, None, None);
        let DataBatch::Quote(rows) = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap()
        else {
            panic!("expected quotes")
        };
        assert_eq!(rows.as_ref(), &[quote]);
    }

    #[rstest]
    fn parent_relative_writer_path_promotes_and_replays() {
        let directory = TempDir::new_in(".").unwrap();
        let current = std::env::current_dir().unwrap();
        let staging = PathBuf::from("..")
            .join(current.file_name().unwrap())
            .join(directory.path().file_name().unwrap())
            .join("live/run-relative");
        let config =
            WriterConnectConfig::new(staging.to_string_lossy(), Some(local_catalog(&directory)));
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));
        let quote = sample_quote();
        let mut writer = ParquetWriter::new(&config, clock.clone()).unwrap();
        writer.core.write_data(Data::Quote(quote)).unwrap();
        let work = writer.prepare_promotion().unwrap().unwrap();
        assert_eq!(work.files(), &["live/run-relative/quotes/quotes_0.feather"]);
        assert_eq!(writer.finalize(work.execute()).unwrap().len(), 1);
        assert!(writer.prepare_promotion().unwrap().is_none());
        drop(writer);

        let mut restarted = ParquetWriter::new(&config, clock).unwrap();
        let work = restarted.prepare_promotion().unwrap().unwrap();
        assert_eq!(work.files(), &["live/run-relative/quotes/quotes_0.feather"]);
        assert!(restarted.finalize(work.execute()).unwrap().is_empty());
        assert!(restarted.prepare_promotion().unwrap().is_none());

        let mut catalog = ParquetDataCatalog::new(directory.path(), None, None, None, None);
        let DataBatch::Quote(rows) = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap()
        else {
            panic!("expected quotes")
        };
        assert_eq!(rows.as_ref(), &[quote]);
    }

    #[rstest]
    #[case::sealed(false)]
    #[case::partial(true)]
    fn restart_discovers_crashed_files_and_deduplicates_kept_promotions(#[case] partial: bool) {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest/run-restart");
        let quote = sample_quote();
        let mut crashed = FeatherWriter::new(
            staging.clone(),
            WriterClock::Test(Arc::new(AtomicU64::new(0))),
            RotationConfig::NoRotation,
            None,
            None,
        );
        crashed.write(quote).unwrap();
        crashed.close().unwrap();
        let sealed = staging.join("quotes/quotes_0.feather");

        if partial {
            fs::rename(&sealed, sealed.with_extension(FEATHER_PARTIAL_EXTENSION)).unwrap();
        }

        drop(crashed);
        let config =
            WriterConnectConfig::new(staging.to_string_lossy(), Some(local_catalog(&directory)));
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));
        let mut writer = ParquetWriter::new(&config, clock.clone()).unwrap();
        let work = writer.prepare_promotion().unwrap().unwrap();
        assert_eq!(
            work.files(),
            &["backtest/run-restart/quotes/quotes_0.feather".to_string()]
        );
        assert_eq!(writer.finalize(work.execute()).unwrap().len(), 1);
        assert!(writer.prepare_promotion().unwrap().is_none());
        drop(writer);

        let mut restarted = ParquetWriter::new(&config, clock).unwrap();
        let work = restarted.prepare_promotion().unwrap().unwrap();
        assert_eq!(work.files().len(), 1);
        assert!(restarted.finalize(work.execute()).unwrap().is_empty());
        assert!(restarted.prepare_promotion().unwrap().is_none());
        assert_scheduled_count(&restarted.core.promotion_driver.schedule(), 0);
        assert!(sealed.exists());

        let mut catalog = ParquetDataCatalog::new(directory.path(), None, None, None, None);
        let DataBatch::Quote(rows) = catalog
            .query_batch(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap()
        else {
            panic!("expected quotes")
        };
        assert_eq!(rows.as_ref(), &[quote]);
    }

    #[rstest]
    #[case::nested_base("/prefix/catalog/", "prefix/catalog/")]
    #[case::root_base("", "")]
    fn promotion_paths_live_under_the_catalog_base_path(
        #[case] base_path: &str,
        #[case] expected_prefix: &str,
    ) {
        let directory = TempDir::new().unwrap();
        let mut catalog = ParquetDataCatalog::new(directory.path(), None, None, None, None);
        catalog.base_path = base_path.to_string();

        let backend = ParquetPromotionBackend {
            catalog,
            legacy_manifest_missing: Arc::default(),
        };

        assert_eq!(
            backend.manifest_path(),
            ObjectPath::from(format!("{expected_prefix}_nautilus_promotions.json")),
        );
        assert_eq!(
            backend.marker_path("replay-id"),
            ObjectPath::from(format!(
                "{expected_prefix}_nautilus_promotions/\
                 cd3112001080bc2d11985ffe2b8b90b324d336d82c17ddb66127d3a05f08c69c.json"
            )),
        );
    }

    #[rstest]
    fn feather_replay_identity_is_stable_and_ignores_identifier_order() {
        let identity = |identifiers: Option<&[String]>| {
            feather_replay_identity(
                "file:///catalog/backtest/run-1",
                "quotes/AUDUSD.SIM/part-0.feather",
                "content-hash",
                identifiers,
            )
        };

        let unordered = ["B".to_string(), "A".to_string(), "A".to_string()];
        let ordered = ["A".to_string(), "B".to_string()];

        let expected =
            "nautilus-feather:10a9435c28f7536f26653c3fc808571be89bfe769e06c7307cb4743e273a23fd";

        assert_eq!(identity(Some(&unordered)), expected);
        assert_eq!(identity(Some(&ordered)), expected);
        assert_ne!(identity(None), expected);
    }
}
