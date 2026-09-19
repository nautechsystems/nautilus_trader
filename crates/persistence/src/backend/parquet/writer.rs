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
    backend::migration::feather_replay_identity,
    common::{
        conversion::FeatherConversionSummary, datafusion::identifiers_from_record_batches,
        storage::create_storage_backend_from_path,
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
    let params = config.params.as_ref();
    let interval_ms = params.and_then(|params| params.get_u64("parquet_commit_interval_ms"));
    let promote_on_close = params
        .and_then(|params| params.get_bool("promote_on_close"))
        .unwrap_or(true);
    let delete_feather_after_commit = params
        .and_then(|params| params.get_bool("delete_feather_after_commit"))
        .unwrap_or(false);
    let use_ts_event_for_ts_init = params
        .and_then(|params| params.get_bool("use_ts_event_for_ts_init"))
        .unwrap_or(false);
    Ok(Box::new(ParquetWriter::new(
        config,
        clock,
        interval_ms,
        promote_on_close,
        delete_feather_after_commit,
        use_ts_event_for_ts_init,
    )?))
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "the booleans are independent writer policy and lifecycle flags"
)]
struct ParquetWriter {
    core: StagedFeatherWriter<ParquetPromotionBackend>,
    session: PromotionSession,
    source: FeatherSessionSource,
    catalog_uri: String,
    storage_options: Option<ahash::AHashMap<String, String>>,
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
            .field("catalog_uri", &self.catalog_uri)
            .field("interval_ms", &self.interval_ms)
            .finish_non_exhaustive()
    }
}

impl ParquetWriter {
    fn new(
        config: &WriterConnectConfig,
        clock: WriterClock,
        interval_ms: Option<u64>,
        promote_on_close: bool,
        delete_feather_after_commit: bool,
        use_ts_event_for_ts_init: bool,
    ) -> anyhow::Result<Self> {
        let storage =
            create_storage_backend_from_path(&config.uri, config.storage_options.clone())?;
        let session = StagedFeatherWriter::<ParquetPromotionBackend>::required_session(
            &storage.original_uri,
            "Parquet writer URI",
        )?;
        let source_storage =
            create_storage_backend_from_path(&session.catalog_uri, config.storage_options.clone())?;
        let source = FeatherSessionSource::new(
            source_storage,
            session.kind.clone(),
            session.instance_id.clone(),
        );
        let mut core = StagedFeatherWriter::new(
            storage.clone(),
            clock,
            config.rotation_config.clone(),
            None,
            None,
            config.flush_interval_ms,
            config.record_filter.clone(),
        )?;
        block_on_nautilus_with(|| {
            storage.write_current_run_manifest(
                &session.kind,
                &session.instance_id,
                "in_progress",
                true,
            )
        })?;
        let timer_catalog_uri = session.catalog_uri.clone();
        let timer_storage_options = config.storage_options.clone();
        core.warn_if_orphan_feather_present("Parquet");
        let legacy_manifest_missing = Arc::new(AtomicBool::new(false));
        let timer_legacy_manifest_missing = Arc::clone(&legacy_manifest_missing);
        core.start_promotion_timer(
            interval_ms,
            source.clone(),
            "parquet-promotion",
            move || {
                Ok(ParquetPromotionBackend::new(
                    ParquetDataCatalog::from_uri(
                        &timer_catalog_uri,
                        timer_storage_options.clone(),
                        None,
                        None,
                        None,
                    )?,
                    Arc::clone(&timer_legacy_manifest_missing),
                ))
            },
            use_ts_event_for_ts_init,
            delete_feather_after_commit,
        )?;
        Ok(Self {
            core,
            catalog_uri: session.catalog_uri.clone(),
            storage_options: config.storage_options.clone(),
            session,
            source,
            interval_ms,
            promote_on_close,
            delete_feather_after_commit,
            use_ts_event_for_ts_init,
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
                &self.session.kind,
                &self.session.instance_id,
                status.as_str(),
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
                &self.session.kind,
                &self.session.instance_id,
                "in_progress",
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
        let catalog = ParquetDataCatalog::from_uri(
            &self.catalog_uri,
            self.storage_options.clone(),
            None,
            None,
            None,
        )?;
        self.core.prepare_promotion(
            ParquetPromotionBackend::new(catalog, Arc::clone(&self.legacy_manifest_missing)),
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
        self.core.flush_on_drop("ParquetWriter");
    }
}

struct ParquetPromotionBackend {
    catalog: ParquetDataCatalog,
    legacy_manifest_missing: Arc<AtomicBool>,
}

const PROMOTION_MANIFEST: &str = "_nautilus_promotions.json";
const PROMOTION_MARKERS: &str = "_nautilus_promotions";

#[derive(Debug, Default, Deserialize, Serialize)]
struct ParquetPromotionManifest {
    identities: Vec<String>,
}

impl ParquetPromotionBackend {
    fn new(catalog: ParquetDataCatalog, legacy_manifest_missing: Arc<AtomicBool>) -> Self {
        Self {
            catalog,
            legacy_manifest_missing,
        }
    }

    fn manifest_path(&self) -> ObjectPath {
        let base = self.catalog.base_path.trim_matches('/');
        if base.is_empty() {
            ObjectPath::from(PROMOTION_MANIFEST)
        } else {
            ObjectPath::from(format!("{base}/{PROMOTION_MANIFEST}"))
        }
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
        let base = self.catalog.base_path.trim_matches('/');
        let digest = blake3::hash(identity.as_bytes()).to_hex();
        let path = format!("{PROMOTION_MARKERS}/{digest}.json");
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
        let object_path = ObjectPath::from(file);
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
        block_on_nautilus_with(|| async {
            source
                .storage
                .object_store
                .delete(&ObjectPath::from(file))
                .await?;
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
        status: crate::writer::run::RunStatus,
        empty: bool,
        _error: Option<&str>,
    ) -> anyhow::Result<()> {
        block_on_nautilus_with(|| {
            source.storage.write_run_manifest(
                &source.kind,
                &source.instance_id,
                status.as_str(),
                empty,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{Data, DataBatch, NautilusDataType, QuoteTick},
        identifiers::InstrumentId,
        types::{ERROR_PRICE, Price, Quantity},
    };
    use rstest::rstest;
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        catalog::traits::{CatalogQuery, CatalogReader},
        test_data::RustTestHashMapCustomData,
    };

    #[rstest]
    fn parquet_default_close_promotes_and_honors_source_retention(
        #[values(false, true)] delete_source: bool,
    ) {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-1");
        let mut config = WriterConnectConfig::new(staging.to_string_lossy(), None);
        config.params = Some(
            serde_json::from_value(json!({"delete_feather_after_commit": delete_source})).unwrap(),
        );
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
    #[case(None)]
    #[case(Some(0))]
    #[case(Some(1))]
    fn parquet_interval_uses_test_clock_and_close_can_leave_staged_data(
        #[case] interval: Option<u64>,
    ) {
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-2");
        let mut config = WriterConnectConfig::new(staging.to_string_lossy(), None);
        config.params = Some(
            serde_json::from_value(
                json!({"parquet_commit_interval_ms": interval, "promote_on_close": false}),
            )
            .unwrap(),
        );
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
                .convert_stream_to_data("run-2", "quotes", Some("backtest"), None, false)
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
            None,
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
            None,
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
            None,
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
            None,
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
            .query_record_batches("order_fill_voided", None, None, None, None, true)
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
    fn parquet_promotion_retains_conflicting_schema_groups(#[case] automatic: bool) {
        use nautilus_model::{
            data::{BookOrder, OrderBookDepth},
            enums::OrderSide,
        };
        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest/run-depth-ties");
        let mut config = WriterConnectConfig::new(staging.to_string_lossy(), None);
        config.params = Some(
            serde_json::from_value(
                json!({"delete_feather_after_commit": true, "promote_on_close": automatic}),
            )
            .unwrap(),
        );
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

        let error = if automatic {
            sink.close().unwrap_err().to_string()
        } else {
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
                    "run-depth-ties",
                    "order_book_depths",
                    Some("backtest"),
                    None,
                    false,
                )
                .unwrap_err()
                .to_string()
        };

        let storage = create_storage_backend_from_path(staging.to_str().unwrap(), None).unwrap();
        let staged = block_on_nautilus_with(|| storage.list_files("", Some(".feather"))).unwrap();
        assert!(error.contains("non-disjoint intervals"), "{error}");
        assert_eq!(staged.len(), 1);
    }

    #[rstest]
    fn parquet_manual_conversion_promotes_custom_data() {
        use nautilus_model::data::{CustomData, DataType};
        use nautilus_serialization::ensure_custom_data_registered;

        ensure_custom_data_registered::<RustTestHashMapCustomData>();

        let directory = TempDir::new().unwrap();
        let staging = directory.path().join("backtest").join("run-custom");
        let mut config = WriterConnectConfig::new(staging.to_string_lossy(), None);
        config.params = Some(serde_json::from_value(json!({"promote_on_close": false})).unwrap());
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
                    "custom/RustTestHashMapCustomData",
                    Some("backtest"),
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
        let mut config = WriterConnectConfig::new(staging.to_string_lossy(), None);
        config.params = Some(serde_json::from_value(json!({"promote_on_close": false})).unwrap());
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
                "custom/RustTestHashMapCustomData",
                Some("backtest"),
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
                "custom/RustTestHashMapCustomData",
                Some("backtest"),
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
}
