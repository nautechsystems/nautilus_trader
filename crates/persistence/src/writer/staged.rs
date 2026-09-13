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
    sync::mpsc::{self, Receiver, Sender, SyncSender},
    thread::JoinHandle,
};

use arrow::record_batch::RecordBatch;
use nautilus_common::live::block_on_nautilus_with;
use nautilus_model::data::{CustomData, Data};

use super::{
    feather::{FeatherWriteCommand, FeatherWriter, RotationConfig, WriterClock, feather_error},
    filter::WriterRecordFilter,
    promotion::{
        PromotionDriver, PromotionResult, PromotionScope, PromotionSink, PromotionTimer,
        PromotionWork, StagedPromotionBackend, list_session_feather_files, schedule_new_paths,
    },
    run::FeatherSessionSource,
};
use crate::common::{conversion::FeatherConversionSummary, storage::StorageBackend};

type StagingResult<T> = Result<T, String>;

enum StagingMessage {
    ShouldWriteCustom(String, Option<String>, SyncSender<StagingReply>),
    WriteData(Data, SyncSender<StagingReply>),
    WriteBatch(Vec<Data>, SyncSender<StagingReply>),
    WriteCustom(CustomData, RecordBatch, SyncSender<StagingReply>),
    WriteAny(FeatherWriteCommand, SyncSender<StagingReply>),
    Flush(SyncSender<StagingReply>),
    Close(SyncSender<StagingReply>),
    IsClosed(SyncSender<StagingReply>),
    BufferedTotals(SyncSender<StagingReply>),
    Stop,
}

enum StagingReply {
    Operation(StagingResult<()>),
    ShouldWriteCustom(bool),
    Closed(bool),
    BufferedTotals(u64, u64),
}

struct StagingClient {
    tx: SyncSender<StagingMessage>,
    reply_tx: SyncSender<StagingReply>,
    replies: Receiver<StagingReply>,
}

impl StagingClient {
    fn new(tx: SyncSender<StagingMessage>) -> Self {
        let (reply_tx, replies) = mpsc::sync_channel(1);
        Self {
            tx,
            reply_tx,
            replies,
        }
    }
}

impl Clone for StagingClient {
    fn clone(&self) -> Self {
        Self::new(self.tx.clone())
    }
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

    fn close(&self) -> anyhow::Result<()> {
        self.request(StagingMessage::Close)
    }

    fn is_closed(&self) -> anyhow::Result<bool> {
        match self.query(StagingMessage::IsClosed)? {
            StagingReply::Closed(closed) => Ok(closed),
            _ => anyhow::bail!("Staging worker returned an unexpected reply"),
        }
    }

    fn buffered_totals(&self) -> anyhow::Result<(u64, u64)> {
        match self.query(StagingMessage::BufferedTotals)? {
            StagingReply::BufferedTotals(bytes, rows) => Ok((bytes, rows)),
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
        self.tx
            .send(message(self.reply_tx.clone()))
            .map_err(|e| anyhow::anyhow!("Staging worker disconnected: {e}"))?;
        self.replies
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
        let handle = std::thread::Builder::new() // dst-ok: dedicated blocking writer thread
            .name("feather-staging".to_string())
            .spawn(move || run_staging_worker(writer, &rx))?;
        Ok(Self {
            client: StagingClient::new(tx),
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

fn run_staging_worker(mut writer: FeatherWriter, rx: &Receiver<StagingMessage>) {
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
                let result = block_on_nautilus_with(|| async {
                    writer.flush().await.map_err(feather_error)
                })
                .map_err(|e| e.to_string());
                let _ = reply.send(StagingReply::Operation(result));
            }
            StagingMessage::Close(reply) => {
                let result = block_on_nautilus_with(|| async {
                    writer.close().await.map_err(feather_error)
                })
                .map_err(|e| e.to_string());
                let _ = reply.send(StagingReply::Operation(result));
            }
            StagingMessage::IsClosed(reply) => {
                let _ = reply.send(StagingReply::Closed(writer.is_closed()));
            }
            StagingMessage::BufferedTotals(reply) => {
                let (bytes, rows) = writer.buffered_totals();
                let _ = reply.send(StagingReply::BufferedTotals(bytes, rows));
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
    timer_errors: Receiver<String>,
    timer_error_tx: Sender<String>,
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
        self.staged().write_any(message)
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

    fn take_periodic_promotion_error(&mut self) -> anyhow::Result<()> {
        self.staged().timer_error()
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
        clock: WriterClock,
        rotation_config: RotationConfig,
        included_types: Option<HashSet<String>>,
        per_instrument_types: Option<HashSet<String>>,
        flush_interval_ms: Option<u64>,
        record_filter: Option<WriterRecordFilter>,
    ) -> anyhow::Result<Self> {
        let writer = FeatherWriter::new(
            storage.base_path.clone(),
            storage.object_store.clone(),
            clock.clone(),
            rotation_config,
            included_types,
            per_instrument_types,
            flush_interval_ms,
        )
        .with_catalog_identifier_column()
        .with_record_filter(record_filter);
        let last_promotion_ns = clock.timestamp_ns();

        let (timer_error_tx, timer_errors) = mpsc::channel();
        Ok(Self {
            storage,

            staging: StagingWorker::spawn(writer)?,
            clock,
            promotion_driver: PromotionDriver::new(last_promotion_ns),
            promotion_timer: None,
            timer_errors,
            timer_error_tx,
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
        let scheduled_paths = self.promotion_driver.scheduled_paths();
        let timer_error_tx = self.timer_error_tx.clone();
        let staging_uri = self.storage.original_uri.clone();

        self.promotion_timer = Some(PromotionTimer::spawn(
            &format!("{worker_name}-timer"),
            interval_ms,
            move || {
                let result = (|| {
                    staging.flush()?;
                    let files = list_session_feather_files(
                        &source.storage,
                        &source.kind,
                        &source.instance_id,
                    )?;
                    let files = schedule_new_paths(&scheduled_paths, files)?;
                    if files.is_empty() {
                        return Ok(());
                    }

                    let promotion = backend().and_then(|backend| {
                        submitter.submit(backend.into_work(PromotionScope {
                            source: source.clone(),
                            staging_uri: staging_uri.clone(),
                            files: files.clone(),
                            use_ts_event_for_ts_init,
                            delete_feather_after_commit,
                        }))
                    });

                    if promotion.is_err() {
                        let mut scheduled = scheduled_paths.lock().map_err(|e| {
                            anyhow::anyhow!("Promotion schedule lock poisoned: {e}")
                        })?;

                        for file in files {
                            scheduled.remove(&file);
                        }
                    }
                    promotion
                })();

                if let Err(e) = result {
                    log::warn!("{worker_name} timer failed: {e}");
                    let _ = timer_error_tx.send(e.to_string());
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
        self.flush()?;
        let files = list_session_feather_files(&source.storage, &source.kind, &source.instance_id)?;
        let files = self.promotion_driver.schedule_new(files)?;
        if files.is_empty() {
            return Ok(None);
        }
        Ok(Some(backend.into_work(PromotionScope {
            source,
            staging_uri: self.storage.original_uri.clone(),
            files,
            use_ts_event_for_ts_init,
            delete_feather_after_commit,
        })))
    }

    pub(crate) fn finalize_promotion(
        &mut self,
        result: PromotionResult,
    ) -> anyhow::Result<Vec<FeatherConversionSummary>> {
        let PromotionResult {
            files,
            committed_paths,
            deleted_paths,
            partial_converted,
            run_state_recorded: _run_state_recorded,
            converted,
        } = result;

        match converted {
            Ok(converted) => {
                self.promotion_driver.unschedule(&deleted_paths);
                self.promotion_driver
                    .mark_committed_at(self.clock.timestamp_ns());
                Ok(converted)
            }
            Err(e) => {
                if !partial_converted.is_empty() {
                    log::warn!(
                        "{} promotion partially committed {} file(s) before failing: {e}",
                        B::NAME,
                        partial_converted.len()
                    );
                }
                self.promotion_driver
                    .unschedule_uncommitted(&files, &committed_paths);
                Err(e)
            }
        }
    }

    pub(crate) fn warn_if_orphan_feather_present(&self, writer_name: &str) {
        let storage = self.storage.clone();
        let result = block_on_nautilus_with(move || async move {
            storage.list_files("", Some(".feather")).await
        });

        match result {
            Ok(files) if !files.is_empty() => log::warn!(
                "{writer_name} found {} orphan Feather file(s) under run session '{}'",
                files.len(),
                self.storage.original_uri
            ),
            Ok(_) => {}
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

    pub(crate) fn flush_on_drop(&self, writer_name: &str) {
        let Ok((buffered_bytes, buffered_rows)) = self.staging.client.buffered_totals() else {
            log::warn!("{writer_name} could not inspect buffered staging data during drop");
            return;
        };

        if buffered_bytes == 0 && buffered_rows == 0 {
            return;
        }

        if let Err(e) = self.staging.client.flush() {
            log::warn!(
                "{writer_name} drop failed to flush staging; discarded {buffered_bytes} \
                 buffered byte(s) across {buffered_rows} row(s): {e}"
            );
        }
    }

    pub(crate) fn stop_promotion_timer(&mut self) {
        if let Some(timer) = self.promotion_timer.as_mut() {
            timer.stop();
        }
        self.promotion_timer = None;
    }

    fn timer_error(&self) -> anyhow::Result<()> {
        let errors = self.timer_errors.try_iter().collect::<Vec<_>>();
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
        super::promotion::PromotionSession::from_uri(uri).ok_or_else(|| {
            anyhow::anyhow!("{uri_name} must end with /{{backtest|sandbox|live}}/{{run_id}}")
        })
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
