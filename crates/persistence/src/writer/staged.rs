// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Shared state owned by Feather-staged promoting writers.

use std::{
    any::Any,
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::JoinHandle,
};

use arrow::record_batch::RecordBatch;
use nautilus_common::live::block_on_nautilus_with;
use nautilus_model::data::{CustomData, Data};
use object_store::path::PathPart;

use super::{
    feather::{
        FEATHER_EXTENSION, FEATHER_PARTIAL_EXTENSION, FeatherWriteCommand, FeatherWriter,
        RotationConfig, WriterClock, feather_error, recover_partial_feather_files,
    },
    filter::WriterRecordFilter,
    promotion::{
        PromotionDriver, PromotionResult, PromotionSchedule, PromotionScope, PromotionSink,
        PromotionTimer, PromotionWork, StagedPromotionBackend, list_session_feather_files,
        schedule_new_paths,
    },
    run::FeatherSessionSource,
};
use crate::{
    catalog::types::CatalogDataType,
    common::{
        conversion::FeatherConversionSummary,
        paths::{
            environment_directory, local_to_object_store_path, local_writer_directory,
            make_object_store_path, normalize_path_separators,
        },
        storage::StorageBackend,
    },
};

type StagingResult<T> = Result<T, String>;

enum StagingMessage {
    ShouldWriteCustom(String, Option<String>, SyncSender<StagingReply>),
    WriteData(Data, SyncSender<StagingReply>),
    WriteBatch(Vec<Data>, SyncSender<StagingReply>),
    WriteCustom(CustomData, RecordBatch, SyncSender<StagingReply>),
    WriteAny(FeatherWriteCommand, SyncSender<StagingReply>),
    Flush(SyncSender<StagingReply>),
    Seal(SyncSender<StagingReply>),
    Close(SyncSender<StagingReply>),
    IsClosed(SyncSender<StagingReply>),
    Stop,
}

enum StagingReply {
    Operation(StagingResult<()>),
    SealedPaths(StagingResult<Vec<PathBuf>>),
    ShouldWriteCustom(bool),
    Closed(bool),
}

#[derive(Clone)]
struct StagingClient {
    tx: SyncSender<StagingMessage>,
}

impl StagingClient {
    fn write_data(&self, data: Data) -> anyhow::Result<()> {
        self.request(|reply| StagingMessage::WriteData(data, reply))
    }

    fn write_batch(&self, data: Vec<Data>) -> anyhow::Result<()> {
        self.request(|reply| StagingMessage::WriteBatch(data, reply))
    }

    fn write_custom(&self, custom: CustomData) -> anyhow::Result<()> {
        let type_name = custom.data.type_name();
        let identifier = custom.data_type.identifier().map(String::from);
        if !self.should_write_custom(type_name, identifier)? {
            return Ok(());
        }

        let batch = FeatherWriter::encode_custom_to_batch(&custom).map_err(feather_error)?;
        self.request(|reply| StagingMessage::WriteCustom(custom, batch, reply))
    }

    fn should_write_custom(
        &self,
        type_name: &str,
        identifier: Option<String>,
    ) -> anyhow::Result<bool> {
        match self.query(|reply| {
            StagingMessage::ShouldWriteCustom(type_name.to_string(), identifier, reply)
        })? {
            StagingReply::ShouldWriteCustom(should_write) => Ok(should_write),
            _ => anyhow::bail!("Staging worker returned an unexpected reply"),
        }
    }

    fn write_any(&self, command: FeatherWriteCommand) -> anyhow::Result<()> {
        self.request(|reply| StagingMessage::WriteAny(command, reply))
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.request(StagingMessage::Flush)
    }

    fn seal(&self) -> anyhow::Result<Vec<PathBuf>> {
        match self.query(StagingMessage::Seal)? {
            StagingReply::SealedPaths(result) => result.map_err(anyhow::Error::msg),
            _ => anyhow::bail!("Staging worker returned an unexpected reply"),
        }
    }

    fn close(&self) -> anyhow::Result<()> {
        self.request(StagingMessage::Close)
    }

    fn is_closed(&self) -> anyhow::Result<bool> {
        match self.query(StagingMessage::IsClosed)? {
            StagingReply::Closed(closed) => Ok(closed),
            _ => anyhow::bail!("Staging worker returned an unexpected reply"),
        }
    }

    fn request<F>(&self, message: F) -> anyhow::Result<()>
    where
        F: FnOnce(SyncSender<StagingReply>) -> StagingMessage,
    {
        match self.query(message)? {
            StagingReply::Operation(result) => result.map_err(anyhow::Error::msg),
            _ => anyhow::bail!("Staging worker returned an unexpected reply"),
        }
    }

    fn query<F>(&self, message: F) -> anyhow::Result<StagingReply>
    where
        F: FnOnce(SyncSender<StagingReply>) -> StagingMessage,
    {
        // A per-request channel disconnects if the worker panics
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.tx
            .send(message(reply_tx))
            .map_err(|e| anyhow::anyhow!("Staging worker disconnected: {e}"))?;
        reply_rx
            .recv()
            .map_err(|e| anyhow::anyhow!("Staging worker disconnected: {e}"))
    }
}

struct StagingWorker {
    client: StagingClient,
    handle: Option<JoinHandle<()>>,
}

impl StagingWorker {
    fn spawn(writer: FeatherWriter) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(64);
        let (sealed_tx, sealed_rx) = mpsc::channel();
        let writer = writer.with_sealed_sender(sealed_tx);
        let handle = std::thread::Builder::new() // dst-ok: dedicated blocking writer thread
            .name("feather-staging".to_string())
            .spawn(move || run_staging_worker(writer, &rx, &sealed_rx))?;
        Ok(Self {
            client: StagingClient { tx },
            handle: Some(handle),
        })
    }
}

impl Drop for StagingWorker {
    fn drop(&mut self) {
        let _ = self.client.tx.send(StagingMessage::Stop);

        if let Some(handle) = self.handle.take()
            && let Err(e) = handle.join()
        {
            log::warn!("Feather staging worker panicked: {e:?}");
        }
    }
}

fn run_staging_worker(
    mut writer: FeatherWriter,
    rx: &Receiver<StagingMessage>,
    sealed_rx: &Receiver<PathBuf>,
) {
    while let Ok(message) = rx.recv() {
        match message {
            StagingMessage::ShouldWriteCustom(type_name, identifier, reply) => {
                let should_write = writer.should_write_custom(&type_name, identifier.as_deref());
                let _ = reply.send(StagingReply::ShouldWriteCustom(should_write));
            }
            StagingMessage::WriteData(data, reply) => {
                let result = writer.write_data(data).map_err(|e| e.to_string());
                let _ = reply.send(StagingReply::Operation(result));
            }
            StagingMessage::WriteBatch(data, reply) => {
                let result = writer.write_data_batch(data).map_err(|e| e.to_string());
                let _ = reply.send(StagingReply::Operation(result));
            }
            StagingMessage::WriteCustom(custom, batch, reply) => {
                let result = writer
                    .write_custom_batch(&custom, batch)
                    .map_err(|e| e.to_string());
                let _ = reply.send(StagingReply::Operation(result));
            }
            StagingMessage::WriteAny(command, reply) => {
                let result = command(&mut writer).map_err(|e| e.to_string());
                let _ = reply.send(StagingReply::Operation(result));
            }
            StagingMessage::Flush(reply) => {
                let result = writer.flush().map_err(|e| e.to_string());
                let _ = reply.send(StagingReply::Operation(result));
            }
            StagingMessage::Seal(reply) => {
                let result = writer
                    .seal()
                    .map(|()| sealed_rx.try_iter().collect())
                    .map_err(|e| e.to_string());
                let _ = reply.send(StagingReply::SealedPaths(result));
            }
            StagingMessage::Close(reply) => {
                let result = writer.close().map_err(|e| e.to_string());
                let _ = reply.send(StagingReply::Operation(result));
            }
            StagingMessage::IsClosed(reply) => {
                let _ = reply.send(StagingReply::Closed(writer.is_closed()));
            }
            StagingMessage::Stop => break,
        }
    }
}

pub(crate) struct StagedFeatherWriter<B>
where
    B: StagedPromotionBackend,
{
    pub(crate) storage: StorageBackend,
    staging: StagingWorker,
    pub(crate) clock: WriterClock,
    pub(crate) promotion_driver: PromotionDriver<B>,
    promotion_timer: Option<PromotionTimer>,
    pending_errors: Receiver<String>,
    error_tx: Sender<String>,
}

pub(crate) trait StagedWriter: Send {
    type Backend: StagedPromotionBackend;

    fn staged(&mut self) -> &mut StagedFeatherWriter<Self::Backend>;

    fn mark_non_empty(&mut self) -> anyhow::Result<()>;

    fn maybe_promote(&mut self) -> anyhow::Result<()>;

    fn wait_for_promotions(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>>;

    fn promote_on_flush(&self) -> bool;

    fn promote_on_close(&self) -> bool;

    fn promote_now(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>>;

    fn record_completed(&mut self);
}

impl<T> PromotionSink for T
where
    T: StagedWriter,
{
    fn stage_data(&mut self, data: Data) -> anyhow::Result<()> {
        self.staged().write_data(data)
    }

    fn stage_batch(&mut self, data: Vec<Data>) -> anyhow::Result<()> {
        self.staged().write_batch(data)
    }

    fn stage_any(&mut self, message: &dyn Any) -> anyhow::Result<bool> {
        let result = self.staged().write_any(message);
        if let Err(e) = &result {
            let _ = self.staged().error_tx.send(e.to_string());
        }

        result
    }

    fn flush_staging(&mut self) -> anyhow::Result<()> {
        self.staged().flush()
    }

    fn close_staging(&mut self) -> anyhow::Result<()> {
        self.staged().close()
    }

    fn mark_run_non_empty(&mut self) -> anyhow::Result<()> {
        self.mark_non_empty()
    }

    fn maybe_promote_by_period(&mut self) -> anyhow::Result<()> {
        self.maybe_promote()
    }

    fn wait_for_background_promotions(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>> {
        self.wait_for_promotions()
    }

    fn stop_periodic_promotion(&mut self) {
        self.staged().stop_promotion_timer();
    }

    fn take_pending_error(&mut self) -> anyhow::Result<()> {
        self.staged().pending_error()
    }

    fn should_promote_on_flush(&self) -> bool {
        self.promote_on_flush()
    }

    fn should_promote_on_close(&self) -> bool {
        self.promote_on_close()
    }

    fn promote(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>> {
        self.promote_now()
    }

    fn record_completed(&mut self) {
        StagedWriter::record_completed(self);
    }
}

impl<B> StagedFeatherWriter<B>
where
    B: StagedPromotionBackend,
{
    pub(crate) fn new(
        storage: StorageBackend,
        source: &FeatherSessionSource,
        clock: WriterClock,
        rotation_config: RotationConfig,
        included_types: Option<HashSet<CatalogDataType>>,
        flush_interval_ms: Option<u64>,
        record_filter: Option<WriterRecordFilter>,
    ) -> anyhow::Result<Self> {
        let directory = local_writer_directory(&storage.original_uri)?;
        recover_partial_feather_files(&directory);
        let files =
            list_session_feather_files(&source.storage, source.environment, &source.instance_id)?;

        let writer = FeatherWriter::new(
            directory,
            clock.clone(),
            rotation_config,
            included_types,
            flush_interval_ms,
        )
        .with_record_filter(record_filter);
        let last_promotion_ns = clock.timestamp_ns();

        let (error_tx, pending_errors) = mpsc::channel();
        Ok(Self {
            storage,

            staging: StagingWorker::spawn(writer)?,
            clock,
            promotion_driver: PromotionDriver::new(last_promotion_ns, files),
            promotion_timer: None,
            pending_errors,
            error_tx,
        })
    }

    pub(crate) fn start_promotion_timer<F>(
        &mut self,
        interval_ms: Option<u64>,
        source: FeatherSessionSource,
        worker_name: &'static str,
        backend: F,
        use_ts_event_for_ts_init: bool,
        delete_feather_after_commit: bool,
    ) -> anyhow::Result<()>
    where
        F: Fn() -> anyhow::Result<B> + Send + Sync + 'static,
    {
        let Some(interval_ms) = interval_ms
            .filter(|interval_ms| *interval_ms > 0 && matches!(self.clock, WriterClock::Live))
        else {
            return Ok(());
        };

        let staging = self.staging.client.clone();
        let submitter = self.promotion_driver.submitter(worker_name)?;
        let schedule = self.promotion_driver.schedule();
        let error_tx = self.error_tx.clone();
        let staging_uri = self.storage.original_uri.clone();

        self.promotion_timer = Some(PromotionTimer::spawn(
            &format!("{worker_name}-timer"),
            interval_ms,
            move || {
                let result = (|| {
                    let files =
                        prepare_promotion_paths(&staging, &source, &staging_uri, &schedule)?;

                    if files.is_empty() {
                        return Ok(());
                    }

                    let promotion = backend().and_then(|backend| {
                        submitter.submit(
                            backend
                                .into_work(PromotionScope {
                                    source: source.clone(),
                                    staging_uri: staging_uri.clone(),
                                    files: files.clone(),
                                    use_ts_event_for_ts_init,
                                    delete_feather_after_commit,
                                })
                                .with_schedule(Arc::clone(&schedule)),
                        )
                    });

                    if promotion.is_err() {
                        schedule
                            .lock()
                            .map_err(|e| anyhow::anyhow!("Promotion schedule lock poisoned: {e}"))?
                            .unschedule(&files);
                    }

                    promotion
                })();

                if let Err(e) = result {
                    log::warn!("{worker_name} timer failed: {e}");
                    let _ = error_tx.send(e.to_string());
                }
            },
        )?);

        Ok(())
    }

    pub(crate) fn prepare_promotion(
        &self,
        backend: B,
        source: FeatherSessionSource,
        use_ts_event_for_ts_init: bool,
        delete_feather_after_commit: bool,
    ) -> anyhow::Result<Option<PromotionWork<B>>> {
        let schedule = self.promotion_driver.schedule();
        let files = prepare_promotion_paths(
            &self.staging.client,
            &source,
            &self.storage.original_uri,
            &schedule,
        )?;

        if files.is_empty() {
            return Ok(None);
        }

        Ok(Some(
            backend
                .into_work(PromotionScope {
                    source,
                    staging_uri: self.storage.original_uri.clone(),
                    files,
                    use_ts_event_for_ts_init,
                    delete_feather_after_commit,
                })
                .with_schedule(schedule),
        ))
    }

    pub(crate) fn finalize_promotion(
        &mut self,
        result: PromotionResult,
    ) -> anyhow::Result<Vec<FeatherConversionSummary>> {
        let PromotionResult {
            files,
            partial_converted,
            converted,
            ..
        } = result;

        match converted {
            Ok(converted) => {
                self.promotion_driver
                    .mark_committed_at(self.clock.timestamp_ns());
                Ok(converted)
            }
            Err(e) => {
                self.promotion_driver.unschedule(&files);

                if !partial_converted.is_empty() {
                    log::warn!(
                        "{} promotion partially committed {} file(s) before failing: {e}",
                        B::NAME,
                        partial_converted.len()
                    );
                }

                Err(e)
            }
        }
    }

    pub(crate) fn warn_if_orphan_feather_present(&self, writer_name: &str) {
        let storage = self.storage.clone();

        let result =
            block_on_nautilus_with(move || async move { storage.list_files("", None).await });

        match result {
            Ok(files) => {
                let partial = files
                    .iter()
                    .filter(|file| file.ends_with(&format!(".{FEATHER_PARTIAL_EXTENSION}")))
                    .count();
                let sealed = files
                    .iter()
                    .filter(|file| file.ends_with(&format!(".{FEATHER_EXTENSION}")))
                    .count();

                if sealed + partial > 0 {
                    log::warn!(
                        "{writer_name} found {sealed} orphan Feather file(s) and {partial} \
                         unsealed Feather file(s) under run session '{}'",
                        self.storage.original_uri
                    );
                }
            }
            Err(e) => log::warn!(
                "Skipping orphan-feather scan for run '{}' (recursive list failed: {e})",
                self.storage.original_uri
            ),
        }
    }

    pub(crate) fn write_data(&self, data: Data) -> anyhow::Result<()> {
        match data {
            Data::Custom(custom) => self.staging.client.write_custom(custom),
            data => self.staging.client.write_data(data),
        }
    }

    pub(crate) fn write_batch(&self, data: Vec<Data>) -> anyhow::Result<()> {
        if data.iter().any(|item| matches!(item, Data::Custom(_))) {
            for item in data {
                self.write_data(item)?;
            }

            return Ok(());
        }

        self.staging.client.write_batch(data)
    }

    pub(crate) fn write_any(&self, message: &dyn Any) -> anyhow::Result<bool> {
        if let Some(custom) = message.downcast_ref::<CustomData>() {
            self.staging.client.write_custom(custom.clone())?;
            return Ok(true);
        }

        if let Some(Data::Custom(custom)) = message.downcast_ref::<Data>() {
            self.staging.client.write_custom(custom.clone())?;
            return Ok(true);
        }

        let Some(command) = FeatherWriter::write_command(message) else {
            return Ok(false);
        };

        self.staging.client.write_any(command)?;
        Ok(true)
    }

    pub(crate) fn flush(&self) -> anyhow::Result<()> {
        self.staging.client.flush()
    }

    pub(crate) fn close(&mut self) -> anyhow::Result<()> {
        self.stop_promotion_timer();
        self.staging.client.close()
    }

    pub(crate) fn is_closed(&self) -> anyhow::Result<bool> {
        self.staging.client.is_closed()
    }

    pub(crate) fn stop_promotion_timer(&mut self) {
        if let Some(timer) = self.promotion_timer.as_mut() {
            timer.stop();
        }

        self.promotion_timer = None;
    }

    fn pending_error(&self) -> anyhow::Result<()> {
        let errors = self.pending_errors.try_iter().collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("{}", errors.join("; "))
        }
    }
}

impl<B> StagedFeatherWriter<B>
where
    B: StagedPromotionBackend,
{
    pub(crate) fn required_session(
        uri: &str,
        uri_name: &str,
    ) -> anyhow::Result<super::promotion::PromotionSession> {
        if !B::REQUIRES_SESSION {
            anyhow::bail!("{} does not require a run session", B::NAME);
        }

        let session = super::promotion::PromotionSession::from_uri(uri).ok_or_else(|| {
            anyhow::anyhow!("{uri_name} must end with /{{backtest|sandbox|live}}/{{run_id}}")
        })?;

        // Staged files keep the raw run directory name, but URL parsing and run listings change it
        let normalized_uri = normalize_path_separators(uri);
        let run_directory = normalized_uri
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default();
        anyhow::ensure!(
            run_directory == session.instance_id
                && PathPart::from(session.instance_id.as_str()).as_ref() == session.instance_id,
            "{uri_name} '{uri}' has a run ID that object-store paths percent-encode; use a run ID \
             without spaces, non-ASCII, or reserved characters",
        );

        Ok(session)
    }
}

impl<B> Drop for StagedFeatherWriter<B>
where
    B: StagedPromotionBackend,
{
    fn drop(&mut self) {
        self.stop_promotion_timer();
    }
}

fn prepare_promotion_paths(
    staging: &StagingClient,
    source: &FeatherSessionSource,
    staging_uri: &str,
    schedule: &Arc<Mutex<PromotionSchedule>>,
) -> anyhow::Result<Vec<String>> {
    let directory = local_writer_directory(staging_uri)?;
    let files = staging
        .seal()?
        .into_iter()
        .map(|path| {
            let relative = local_to_object_store_path(path.strip_prefix(&directory)?);
            Ok(make_object_store_path(
                environment_directory(source.environment),
                [source.instance_id.as_str(), relative.as_str()],
            ))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    schedule_new_paths(schedule, files)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };

    use nautilus_common::enums::Environment;
    use nautilus_model::{
        data::{NautilusDataType, QuoteTick},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use object_store::memory::InMemory;
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        common::storage::create_storage_backend_from_path,
        writer::promotion::{PromotionBackend, tests::assert_scheduled_count},
    };

    #[derive(Clone)]
    struct RecordingBackend {
        files: Sender<String>,
        fail: bool,
    }

    impl PromotionBackend for RecordingBackend {
        type Source = FeatherSessionSource;
        const NAME: &'static str = "Recording";

        fn convert_file(
            &mut self,
            _source: &Self::Source,
            file: &str,
            _use_ts_event_for_ts_init: bool,
            _record_promoted: bool,
        ) -> anyhow::Result<Option<FeatherConversionSummary>> {
            self.files.send(file.to_string()).unwrap();

            if self.fail {
                self.fail = false;
                anyhow::bail!("conversion failed");
            }

            Ok(Some(FeatherConversionSummary {
                data_type: NautilusDataType::QuoteTick.into(),
                identifier: None,
                feather_path: file.to_string(),
                native_version: None,
                unmatched_identifiers: None,
            }))
        }

        fn delete_file(&mut self, _source: &Self::Source, _file: &str) -> anyhow::Result<()> {
            unreachable!("tests keep promoted files")
        }
    }

    impl StagedPromotionBackend for RecordingBackend {}

    #[rstest]
    #[case::ready(false)]
    #[case::factory_retry(true)]
    fn live_timer_promotes_only_sealed_notifications_without_listing(#[case] fail_factory: bool) {
        let directory = TempDir::new().unwrap();
        let root =
            create_storage_backend_from_path(directory.path().to_str().unwrap(), None).unwrap();
        let mut source = FeatherSessionSource::new(root, Environment::Live, "run-timer");
        let storage = create_storage_backend_from_path(
            directory.path().join("live/run-timer").to_str().unwrap(),
            None,
        )
        .unwrap();
        let mut writer = StagedFeatherWriter::<RecordingBackend>::new(
            storage,
            &source,
            WriterClock::Live,
            RotationConfig::Size { max_size: 1 },
            None,
            Some(0),
            None,
        )
        .unwrap();
        let listing_source = source.clone();
        source.storage.object_store = Arc::new(InMemory::new());
        let (files_tx, files_rx) = mpsc::channel();
        let backend = RecordingBackend {
            files: files_tx,
            fail: false,
        };
        let attempts = Arc::new(AtomicU64::new(0));
        let factory_attempts = Arc::clone(&attempts);
        writer
            .start_promotion_timer(
                Some(1),
                source,
                "recording-promotion",
                move || {
                    let attempt = factory_attempts.fetch_add(1, Ordering::Relaxed);
                    if fail_factory && attempt == 0 {
                        anyhow::bail!("backend factory failed");
                    }

                    Ok(backend.clone())
                },
                false,
                false,
            )
            .unwrap();
        let mut promoted = Vec::new();

        for timestamp in 1..=32 {
            writer.write_data(Data::Quote(quote_at(timestamp))).unwrap();
            promoted.push(files_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }

        writer.stop_promotion_timer();
        let results = writer.promotion_driver.wait().unwrap();
        assert_eq!(results.len(), 32);

        for result in results {
            assert_eq!(result.files.len(), 1);
            assert_eq!(writer.finalize_promotion(result).unwrap().len(), 1);
        }

        let files =
            list_session_feather_files(&listing_source.storage, Environment::Live, "run-timer")
                .unwrap();
        promoted.sort();
        assert_eq!(files.len(), 32);
        assert_eq!(promoted, files);
        assert_eq!(
            attempts.load(Ordering::Relaxed),
            32 + u64::from(fail_factory)
        );
        assert_scheduled_count(&writer.promotion_driver.schedule(), 0);

        if fail_factory {
            assert_eq!(
                writer.pending_error().unwrap_err().to_string(),
                "backend factory failed"
            );
        }

        writer.pending_error().unwrap();
        assert_eq!(
            files_rx.try_iter().collect::<Vec<_>>(),
            Vec::<String>::new()
        );
    }

    #[rstest]
    #[case::ascii("CustomData")]
    #[case::non_ascii("Données")]
    #[case::reserved("Data[100%]")]
    fn sealed_notification_paths_match_object_store_listing(#[case] type_name: &str) {
        let directory = TempDir::new().unwrap();
        let root =
            create_storage_backend_from_path(directory.path().to_str().unwrap(), None).unwrap();
        let source = FeatherSessionSource::new(root, Environment::Live, "run-paths");
        let relative = format!("live/run-paths/data/custom/{type_name}/{type_name}_0.feather");
        let path = directory.path().join(&relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"sealed").unwrap();
        let (tx, requests) = mpsc::sync_channel(1);
        let staging = StagingClient { tx };

        let reply_thread = std::thread::spawn(move || {
            let StagingMessage::Seal(reply) = requests.recv().unwrap() else {
                panic!("expected seal request")
            };
            assert!(
                reply
                    .send(StagingReply::SealedPaths(Ok(vec![path])))
                    .is_ok()
            );
        });
        let driver = PromotionDriver::<RecordingBackend>::new(0_u64.into(), Vec::new());
        let staging_uri = directory.path().join("live/run-paths");
        let files = prepare_promotion_paths(
            &staging,
            &source,
            staging_uri.to_str().unwrap(),
            &driver.schedule(),
        )
        .unwrap();
        reply_thread.join().unwrap();

        assert_eq!(files, vec![relative]);
        assert_eq!(
            files,
            list_session_feather_files(&source.storage, Environment::Live, "run-paths").unwrap()
        );
    }

    #[rstest]
    fn failed_promotion_retries_only_uncommitted_paths() {
        let directory = TempDir::new().unwrap();
        let root =
            create_storage_backend_from_path(directory.path().to_str().unwrap(), None).unwrap();
        let source = FeatherSessionSource::new(root, Environment::Backtest, "run-retry");
        let storage = create_storage_backend_from_path(
            directory
                .path()
                .join("backtest/run-retry")
                .to_str()
                .unwrap(),
            None,
        )
        .unwrap();
        let mut writer = StagedFeatherWriter::<RecordingBackend>::new(
            storage,
            &source,
            WriterClock::Test(Arc::new(AtomicU64::new(0))),
            RotationConfig::Size { max_size: 1 },
            None,
            Some(0),
            None,
        )
        .unwrap();
        let (files_tx, files_rx) = mpsc::channel();
        writer.write_data(Data::Quote(quote_at(1))).unwrap();
        writer.write_data(Data::Quote(quote_at(2))).unwrap();
        let backend = RecordingBackend {
            files: files_tx,
            fail: true,
        };
        let work = writer
            .prepare_promotion(backend.clone(), source.clone(), false, false)
            .unwrap()
            .unwrap();
        let files = work.files().to_vec();
        assert_eq!(files.len(), 2);
        let result = work.execute();
        assert_eq!(result.committed_paths, vec![files[1].clone()]);
        assert_eq!(result.partial_converted.len(), 1);
        assert_scheduled_count(&writer.promotion_driver.schedule(), 1);

        for _ in 0..32 {
            assert!(
                writer
                    .prepare_promotion(backend.clone(), source.clone(), false, false)
                    .unwrap()
                    .is_none()
            );
        }
        assert!(writer.finalize_promotion(result).is_err());
        assert_scheduled_count(&writer.promotion_driver.schedule(), 0);
        let backend = RecordingBackend {
            fail: false,
            ..backend
        };
        let work = writer
            .prepare_promotion(backend.clone(), source.clone(), false, false)
            .unwrap()
            .unwrap();
        assert_eq!(work.files(), &files[..1]);
        assert_eq!(writer.finalize_promotion(work.execute()).unwrap().len(), 1);
        assert!(
            writer
                .prepare_promotion(backend, source, false, false)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            files_rx.try_iter().collect::<Vec<_>>(),
            vec![files[0].clone(), files[1].clone(), files[0].clone()]
        );
    }

    #[rstest]
    #[cfg(unix)]
    fn failed_seal_keeps_previously_queued_paths() {
        let directory = TempDir::new().unwrap();
        let writer = FeatherWriter::new(
            directory.path().to_path_buf(),
            WriterClock::Test(Arc::new(AtomicU64::new(0))),
            RotationConfig::NoRotation,
            None,
            Some(0),
        );

        let worker = StagingWorker::spawn(writer).unwrap();
        worker.client.write_data(Data::Quote(quote_at(1))).unwrap();
        worker
            .client
            .write_any(Box::new(FeatherWriter::seal))
            .unwrap();
        worker.client.write_data(Data::Quote(quote_at(2))).unwrap();
        std::fs::remove_file(directory.path().join("quotes/quotes_0-1.feather.partial")).unwrap();

        assert_eq!(
            worker.client.seal().unwrap_err().to_string(),
            "No such file or directory (os error 2)"
        );
        assert_eq!(
            worker.client.seal().unwrap(),
            vec![directory.path().join("quotes/quotes_0.feather")]
        );
        assert_eq!(worker.client.seal().unwrap(), Vec::<PathBuf>::new());
    }

    fn quote_at(timestamp: u64) -> QuoteTick {
        QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("0.65"),
            Price::from("0.67"),
            Quantity::from("11"),
            Quantity::from("17"),
            timestamp.into(),
            timestamp.into(),
        )
    }

    #[rstest]
    fn test_staging_request_returns_error_when_worker_panics() {
        let temp_dir = TempDir::new().unwrap();

        let writer = FeatherWriter::new(
            temp_dir.path().to_path_buf(),
            WriterClock::Test(Arc::new(AtomicU64::new(0))),
            RotationConfig::NoRotation,
            None,
            Some(0),
        );

        let worker = StagingWorker::spawn(writer).unwrap();
        let client = worker.client.clone();
        let (result_tx, result_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = client.write_any(Box::new(|_| panic!("staging worker test panic")));
            let _ = result_tx.send(result.map_err(|e| e.to_string()));
        });

        let result = result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("staging request did not return after the worker panicked");
        assert_eq!(
            result,
            Err("Staging worker disconnected: receiving on a closed channel".to_string())
        );
    }
}
