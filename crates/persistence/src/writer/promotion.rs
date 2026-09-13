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

//! Shared background execution for Feather-to-catalog promotion.

use std::{
    any::Any,
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::JoinHandle,
    time::Duration,
};

use ahash::AHashSet;
use nautilus_common::live::block_on_nautilus_with;
use nautilus_core::UnixNanos;
use nautilus_model::data::Data;

use crate::{
    common::{conversion::FeatherConversionSummary, storage::StorageBackend},
    writer::{
        run::{FeatherSessionSource, RunStatus},
        traits::StreamingDataSink,
    },
};

pub(crate) fn list_session_feather_files(
    storage: &StorageBackend,
    kind: &str,
    instance_id: &str,
) -> anyhow::Result<Vec<String>> {
    let run_directory = format!("{kind}/{instance_id}");
    let mut files =
        block_on_nautilus_with(|| storage.list_files(&run_directory, Some(".feather")))?;
    files.sort();
    Ok(files)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PromotionSession {
    pub(crate) catalog_uri: String,
    pub(crate) kind: String,
    pub(crate) instance_id: String,
}

impl PromotionSession {
    pub(crate) fn from_uri(uri: &str) -> Option<Self> {
        if let Ok(url) = url::Url::parse(uri)
            && let Some(session) = Self::from_url(url)
        {
            return Some(session);
        }
        Self::from_local_path(uri)
    }

    fn from_url(mut url: url::Url) -> Option<Self> {
        let components = url
            .path()
            .trim_matches('/')
            .split('/')
            .filter(|component| !component.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let instance_id = components.last()?.clone();
        let kind = components.get(components.len().checked_sub(2)?)?.clone();
        if !is_run_kind(&kind) {
            return None;
        }

        let catalog_segments = &components[..components.len().saturating_sub(2)];
        let catalog_path = if catalog_segments.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", catalog_segments.join("/"))
        };
        url.set_path(&catalog_path);
        url.set_query(None);
        url.set_fragment(None);

        Some(Self {
            catalog_uri: url.to_string(),
            kind,
            instance_id,
        })
    }

    fn from_local_path(path: &str) -> Option<Self> {
        let normalized;
        let path = if path.as_bytes().get(1) == Some(&b':') {
            normalized = path.replace('\\', "/");
            PathBuf::from(&normalized)
        } else {
            PathBuf::from(path)
        };
        let instance_id = path.file_name()?.to_string_lossy().to_string();
        let kind = path.parent()?.file_name()?.to_string_lossy().to_string();
        if !is_run_kind(&kind) {
            return None;
        }
        let catalog_uri = path.parent()?.parent()?.to_string_lossy().to_string();
        Some(Self {
            catalog_uri,
            kind,
            instance_id,
        })
    }
}

fn is_run_kind(kind: &str) -> bool {
    matches!(kind, "backtest" | "live" | "sandbox")
}

pub(crate) trait PromotionSink: Send {
    fn stage_data(&mut self, data: Data) -> anyhow::Result<()>;

    fn stage_batch(&mut self, data: Vec<Data>) -> anyhow::Result<()>;

    fn stage_any(&mut self, message: &dyn Any) -> anyhow::Result<bool>;

    fn flush_staging(&mut self) -> anyhow::Result<()>;

    fn close_staging(&mut self) -> anyhow::Result<()>;

    fn mark_run_non_empty(&mut self) -> anyhow::Result<()>;

    fn maybe_promote_by_period(&mut self) -> anyhow::Result<()>;

    fn wait_for_background_promotions(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>>;

    fn stop_periodic_promotion(&mut self);

    fn take_periodic_promotion_error(&mut self) -> anyhow::Result<()>;

    fn should_promote_on_flush(&self) -> bool;

    fn should_promote_on_close(&self) -> bool;

    fn promote(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>>;

    fn record_completed(&mut self);
}

impl<T> StreamingDataSink for T
where
    T: PromotionSink + std::fmt::Debug,
{
    fn write_data(&mut self, data: Data) -> anyhow::Result<()> {
        self.stage_data(data)?;
        self.mark_run_non_empty()?;
        self.maybe_promote_by_period()
    }

    fn write_batch(&mut self, data: Vec<Data>) -> anyhow::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        self.stage_batch(data)?;
        self.mark_run_non_empty()?;
        self.maybe_promote_by_period()
    }

    fn write_any(&mut self, message: &dyn Any) -> anyhow::Result<bool> {
        let handled = self.stage_any(message)?;

        if handled {
            self.mark_run_non_empty()?;
            self.maybe_promote_by_period()?;
        }
        Ok(handled)
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        self.flush_staging()?;
        let background_result = self.wait_for_background_promotions();

        let promotion_result = if self.should_promote_on_flush() {
            self.promote().map(|_| ())
        } else {
            Ok(())
        };
        let timer_result = self.take_periodic_promotion_error();

        background_result?;
        promotion_result?;
        timer_result
    }

    fn close(&mut self) -> anyhow::Result<()> {
        self.stop_periodic_promotion();
        self.close_staging()?;
        let background_result = self.wait_for_background_promotions();
        let timer_result = self.take_periodic_promotion_error();

        let promotion_result = if self.should_promote_on_close() {
            self.promote().map(|_| ())
        } else if background_result.is_ok() && timer_result.is_ok() {
            self.record_completed();
            Ok(())
        } else {
            Ok(())
        };

        background_result?;
        promotion_result?;
        timer_result
    }
}

pub(crate) trait PromotionBackend: Send + 'static {
    type Source: Send + 'static;

    const NAME: &'static str;

    fn convert_file(
        &mut self,
        source: &Self::Source,
        file: &str,
        use_ts_event_for_ts_init: bool,
        record_promoted: bool,
    ) -> anyhow::Result<Option<FeatherConversionSummary>>;

    fn delete_file(&mut self, source: &Self::Source, file: &str) -> anyhow::Result<()>;
}

pub(crate) struct PromotionScope {
    pub(crate) source: FeatherSessionSource,
    pub(crate) staging_uri: String,
    pub(crate) files: Vec<String>,
    pub(crate) use_ts_event_for_ts_init: bool,
    pub(crate) delete_feather_after_commit: bool,
}

pub(crate) trait StagedPromotionBackend:
    PromotionBackend<Source = FeatherSessionSource> + Sized
{
    const REQUIRES_SESSION: bool = false;
    const RECORDS_PROMOTED_ATOMICALLY: bool = false;

    fn into_work(self, scope: PromotionScope) -> PromotionWork<Self> {
        PromotionWork::new(
            self,
            scope.source,
            scope.staging_uri,
            scope.files,
            scope.use_ts_event_for_ts_init,
            scope.delete_feather_after_commit,
        )
    }

    fn record_run_state(
        &mut self,
        _source: &FeatherSessionSource,
        _staging_uri: &str,
        _status: RunStatus,
        _empty: bool,
        _error: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Deletes staged files whose promotion this backend has already durably recorded
    /// with a deletion intent, returning the deleted paths.
    ///
    /// The hook owns both the predicate and the deletion, so a file whose retention
    /// was intended is never reported and never removed. The work loop runs it before
    /// converting and skips the returned paths.
    fn delete_recorded_leftovers(
        &mut self,
        _source: &FeatherSessionSource,
        _files: &[String],
    ) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }
}

pub(crate) struct PromotionWork<B>
where
    B: StagedPromotionBackend,
{
    backend: B,
    source: B::Source,
    staging_uri: String,
    files: Vec<String>,
    use_ts_event_for_ts_init: bool,
    delete_feather_after_commit: bool,
}

impl<B> PromotionWork<B>
where
    B: StagedPromotionBackend,
{
    pub(crate) fn new(
        backend: B,
        source: B::Source,
        staging_uri: String,
        files: Vec<String>,
        use_ts_event_for_ts_init: bool,
        delete_feather_after_commit: bool,
    ) -> Self {
        Self {
            backend,
            source,
            staging_uri,
            files,
            use_ts_event_for_ts_init,
            delete_feather_after_commit,
        }
    }

    pub(crate) fn execute(self) -> PromotionResult {
        let files = self.files.clone();

        match panic::catch_unwind(AssertUnwindSafe(|| self.execute_inner())) {
            Ok(result) => result,
            Err(payload) => PromotionResult {
                files,
                committed_paths: Vec::new(),
                deleted_paths: Vec::new(),
                partial_converted: Vec::new(),
                run_state_recorded: false,
                converted: Err(anyhow::anyhow!(
                    "{} promotion worker panicked: {}",
                    B::NAME,
                    panic_payload_message(payload.as_ref()),
                )),
            },
        }
    }

    pub(crate) fn files(&self) -> &[String] {
        &self.files
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the promotion loop keeps conversion, promoted-record fallback, and deletion accounting together"
    )]
    fn execute_inner(mut self) -> PromotionResult {
        let mut converted = Vec::new();
        let mut committed_paths = Vec::new();
        let mut deleted_paths = Vec::new();
        let mut errors = Vec::new();
        let mut promoted_recorded = false;
        let mut record_error = None;

        // Recorded leftovers are promotions the backend already recorded durably with
        // a deletion intent; a failed scan only logs so promotion still proceeds.
        let leftovers = match self
            .backend
            .delete_recorded_leftovers(&self.source, &self.files)
        {
            Ok(leftovers) => leftovers,
            Err(e) => {
                log::warn!(
                    "{} recorded-leftover cleanup failed; continuing with promotion: {e:#}",
                    B::NAME,
                );
                Vec::new()
            }
        };
        deleted_paths.extend(leftovers.iter().cloned());
        let convert_files = self
            .files
            .iter()
            .filter(|file| !leftovers.contains(file))
            .cloned()
            .collect::<Vec<_>>();

        for (index, file) in convert_files.iter().enumerate() {
            let record_promoted = index + 1 == convert_files.len() && errors.is_empty();

            match self.backend.convert_file(
                &self.source,
                file,
                self.use_ts_event_for_ts_init,
                record_promoted,
            ) {
                Ok(Some(summary)) => {
                    if B::RECORDS_PROMOTED_ATOMICALLY && record_promoted {
                        if summary.native_version.is_some() {
                            promoted_recorded = true;
                        } else {
                            // A no-op conversion (replay dedup, fully covered rows)
                            // commits nothing to carry the promoted record, so write it
                            // before deleting the staged file: a crash then leaves a
                            // promoted run with leftover files, which recovery deletes,
                            // never a promotion recorded nowhere.
                            match self.backend.record_run_state(
                                &self.source,
                                &self.staging_uri,
                                RunStatus::Promoted,
                                false,
                                None,
                            ) {
                                Ok(()) => promoted_recorded = true,
                                Err(e) => record_error = Some(e),
                            }
                        }
                    }
                    converted.push(summary);
                    committed_paths.push(file.clone());
                    if record_error.is_some() {
                        // Keep the staged file so the retried promotion can re-run the
                        // record write; the replay dedup makes the re-run a no-op.
                    } else if self.delete_feather_after_commit
                        && let Err(e) = self.backend.delete_file(&self.source, file)
                    {
                        errors.push(format!(
                            "{:#}",
                            e.context(format!(
                                "{} source deletion failed for Feather file '{file}'",
                                B::NAME,
                            )),
                        ));
                    } else if self.delete_feather_after_commit {
                        deleted_paths.push(file.clone());
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    let e = e.context(format!(
                        "{} promotion failed for Feather file '{file}'",
                        B::NAME,
                    ));
                    errors.push(format!("{e:#}"));
                }
            }
        }

        if let Some(e) = record_error {
            return PromotionResult {
                files: self.files,
                committed_paths,
                deleted_paths,
                partial_converted: converted,
                run_state_recorded: false,
                converted: Err(
                    e.context(format!("{} failed to record promoted run state", B::NAME))
                ),
            };
        }

        if !errors.is_empty() {
            let error = errors.join("; ");
            let run_state_recorded = match self.backend.record_run_state(
                &self.source,
                &self.staging_uri,
                RunStatus::Failed,
                false,
                Some(&error),
            ) {
                Ok(()) => true,
                Err(state_error) => {
                    errors.push(format!(
                        "{} failed to record failed run state: {state_error}",
                        B::NAME,
                    ));
                    false
                }
            };
            return PromotionResult {
                files: self.files,
                committed_paths,
                deleted_paths,
                partial_converted: converted,
                run_state_recorded,
                converted: Err(anyhow::anyhow!(errors.join("; "))),
            };
        }

        if !converted.is_empty()
            && !B::RECORDS_PROMOTED_ATOMICALLY
            && let Err(e) = self.backend.record_run_state(
                &self.source,
                &self.staging_uri,
                RunStatus::Promoted,
                false,
                None,
            )
        {
            return PromotionResult {
                files: self.files,
                committed_paths,
                deleted_paths,
                partial_converted: converted,
                run_state_recorded: false,
                converted: Err(
                    e.context(format!("{} failed to record promoted run state", B::NAME))
                ),
            };
        }
        let run_state_recorded = if B::RECORDS_PROMOTED_ATOMICALLY {
            promoted_recorded
        } else {
            !converted.is_empty()
        };
        PromotionResult {
            files: self.files,
            committed_paths,
            deleted_paths,
            partial_converted: Vec::new(),
            run_state_recorded,
            converted: Ok(converted),
        }
    }
}

/// Opaque outcome returned by a prepared catalog promotion.
///
/// Callers pass this value back to the writer that prepared the promotion so it
/// can update scheduling and run state.
pub struct PromotionResult {
    pub(crate) files: Vec<String>,
    pub(crate) committed_paths: Vec<String>,
    pub(crate) deleted_paths: Vec<String>,
    pub(crate) partial_converted: Vec<FeatherConversionSummary>,
    pub(crate) run_state_recorded: bool,
    pub(crate) converted: anyhow::Result<Vec<FeatherConversionSummary>>,
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_string();
    }

    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic payload".to_string()
}

pub(crate) trait PromotionTask: Send + 'static {
    type Output: Send + 'static;

    fn execute(self) -> Self::Output;
}

pub(crate) struct PromotionTimer {
    stop_tx: SyncSender<()>,
    handle: Option<JoinHandle<()>>,
}

impl PromotionTimer {
    pub(crate) fn spawn<F>(
        thread_name: &str,
        interval_ms: u64,
        mut callback: F,
    ) -> anyhow::Result<Self>
    where
        F: FnMut() + Send + 'static,
    {
        anyhow::ensure!(interval_ms > 0, "Promotion timer interval must be positive");
        let (stop_tx, stop_rx) = mpsc::sync_channel(1);
        let interval = Duration::from_millis(interval_ms);
        let handle = std::thread::Builder::new() // dst-ok: timer performs blocking catalog I/O
            .name(thread_name.to_string())
            .spawn(move || {
                loop {
                    match stop_rx.recv_timeout(interval) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => callback(),
                    }
                }
            })?;
        Ok(Self {
            stop_tx,
            handle: Some(handle),
        })
    }

    pub(crate) fn stop(&mut self) {
        let _ = self.stop_tx.try_send(());

        if let Some(handle) = self.handle.take()
            && let Err(e) = handle.join()
        {
            log::warn!("Promotion timer panicked: {e:?}");
        }
    }
}

impl Drop for PromotionTimer {
    fn drop(&mut self) {
        self.stop();
    }
}

enum PromotionMessage<T> {
    Work(Box<T>),
    Close,
}

pub(crate) struct PromotionSubmitter<T>
where
    T: PromotionTask,
{
    tx: SyncSender<PromotionMessage<T>>,
    pending: Arc<AtomicUsize>,
}

impl<T> Clone for PromotionSubmitter<T>
where
    T: PromotionTask,
{
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            pending: Arc::clone(&self.pending),
        }
    }
}

impl<T> PromotionSubmitter<T>
where
    T: PromotionTask,
{
    pub(crate) fn submit(&self, work: T) -> anyhow::Result<()> {
        self.pending.fetch_add(1, Ordering::AcqRel);
        if let Err(e) = self.tx.send(PromotionMessage::Work(Box::new(work))) {
            self.pending.fetch_sub(1, Ordering::AcqRel);
            anyhow::bail!("Promotion worker disconnected: {e}");
        }
        Ok(())
    }
}

pub(crate) struct PromotionWorker<T>
where
    T: PromotionTask,
{
    tx: SyncSender<PromotionMessage<T>>,
    rx: Receiver<T::Output>,
    handle: Option<JoinHandle<()>>,
    pending: Arc<AtomicUsize>,
}

impl<T> PromotionWorker<T>
where
    T: PromotionTask,
{
    pub(crate) fn spawn(thread_name: &str) -> anyhow::Result<Self> {
        let (tx, work_rx) = mpsc::sync_channel::<PromotionMessage<T>>(1);
        let (result_tx, rx) = mpsc::channel::<T::Output>();
        let handle = std::thread::Builder::new() // dst-ok: catalog promotion performs blocking I/O
            .name(thread_name.to_string())
            .spawn(move || {
                while let Ok(message) = work_rx.recv() {
                    match message {
                        PromotionMessage::Work(work) => {
                            let _ = result_tx.send((*work).execute());
                        }
                        PromotionMessage::Close => break,
                    }
                }
            })?;

        Ok(Self {
            tx,
            rx,
            handle: Some(handle),
            pending: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub(crate) fn submit(&self, work: T) -> anyhow::Result<()> {
        self.submitter().submit(work)
    }

    pub(crate) fn submitter(&self) -> PromotionSubmitter<T> {
        PromotionSubmitter {
            tx: self.tx.clone(),
            pending: Arc::clone(&self.pending),
        }
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        self.pending.load(Ordering::Acquire) > 0
    }

    pub(crate) fn try_recv_result(&self) -> anyhow::Result<Option<T::Output>> {
        match self.rx.try_recv() {
            Ok(result) => {
                self.pending.fetch_sub(1, Ordering::AcqRel);
                Ok(Some(result))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => anyhow::bail!("Promotion worker disconnected"),
        }
    }

    pub(crate) fn recv_result(&self) -> anyhow::Result<T::Output> {
        let result = self
            .rx
            .recv()
            .map_err(|e| anyhow::anyhow!("Promotion worker disconnected: {e}"))?;
        self.pending.fetch_sub(1, Ordering::AcqRel);
        Ok(result)
    }
}

impl<T> Drop for PromotionWorker<T>
where
    T: PromotionTask,
{
    fn drop(&mut self) {
        while self.pending.load(Ordering::Acquire) > 0 {
            if self.recv_result().is_err() {
                break;
            }
        }

        let _ = self.tx.send(PromotionMessage::Close);

        if let Some(handle) = self.handle.take()
            && let Err(e) = handle.join()
        {
            log::warn!("Promotion worker panicked: {e:?}");
        }
    }
}

pub(crate) struct PromotionDriver<B>
where
    B: StagedPromotionBackend,
{
    worker: Option<PromotionWorker<PromotionWork<B>>>,
    scheduled_paths: Arc<Mutex<AHashSet<String>>>,
    last_commit_time_ns: UnixNanos,
}

impl<B> PromotionDriver<B>
where
    B: StagedPromotionBackend,
{
    pub(crate) fn new(last_commit_time_ns: UnixNanos) -> Self {
        Self {
            worker: None,
            scheduled_paths: Arc::new(Mutex::new(AHashSet::new())),
            last_commit_time_ns,
        }
    }

    pub(crate) fn is_due(&self, now: UnixNanos, interval_ns: u64) -> bool {
        now.as_u64()
            .saturating_sub(self.last_commit_time_ns.as_u64())
            >= interval_ns
    }

    pub(crate) fn mark_committed_at(&mut self, now: UnixNanos) {
        self.last_commit_time_ns = now;
    }

    pub(crate) fn schedule_new(&self, files: Vec<String>) -> anyhow::Result<Vec<String>> {
        schedule_new_paths(&self.scheduled_paths, files)
    }

    pub(crate) fn submit(
        &mut self,
        work: PromotionWork<B>,
        worker_name: &str,
    ) -> anyhow::Result<()> {
        let files = work.files().to_vec();

        if self.worker.is_none() {
            self.worker = Some(PromotionWorker::spawn(worker_name)?);
        }

        if let Err(e) = self.worker.as_mut().unwrap().submit(work) {
            self.unschedule(&files);
            return Err(e);
        }
        Ok(())
    }

    pub(crate) fn submitter(
        &mut self,
        worker_name: &str,
    ) -> anyhow::Result<PromotionSubmitter<PromotionWork<B>>> {
        if self.worker.is_none() {
            self.worker = Some(PromotionWorker::spawn(worker_name)?);
        }
        Ok(self.worker.as_ref().unwrap().submitter())
    }

    pub(crate) fn scheduled_paths(&self) -> Arc<Mutex<AHashSet<String>>> {
        Arc::clone(&self.scheduled_paths)
    }

    pub(crate) fn drain_completed(&mut self) -> anyhow::Result<Vec<PromotionResult>> {
        let mut results = Vec::new();

        loop {
            let Some(worker) = self.worker.as_mut() else {
                return Ok(results);
            };

            match worker.try_recv_result()? {
                Some(result) => {
                    results.push(result);
                }
                None => return Ok(results),
            }
        }
    }

    pub(crate) fn wait(&mut self) -> anyhow::Result<Vec<PromotionResult>> {
        let mut results = self.drain_completed()?;

        loop {
            let Some(worker) = self.worker.as_mut() else {
                return Ok(results);
            };

            if !worker.has_pending_work() {
                return Ok(results);
            }

            results.push(worker.recv_result()?);
        }
    }

    pub(crate) fn unschedule(&self, files: &[String]) {
        let mut scheduled_paths = self
            .scheduled_paths
            .lock()
            .expect("promotion schedule lock poisoned");

        for file in files {
            scheduled_paths.remove(file);
        }
    }

    pub(crate) fn unschedule_uncommitted(&self, files: &[String], committed_paths: &[String]) {
        let committed_paths = committed_paths
            .iter()
            .map(String::as_str)
            .collect::<AHashSet<_>>();

        let mut scheduled_paths = self
            .scheduled_paths
            .lock()
            .expect("promotion schedule lock poisoned");

        for file in files {
            if !committed_paths.contains(file.as_str()) {
                scheduled_paths.remove(file);
            }
        }
    }
}

pub(crate) fn schedule_new_paths(
    scheduled_paths: &Arc<Mutex<AHashSet<String>>>,
    files: Vec<String>,
) -> anyhow::Result<Vec<String>> {
    let mut scheduled = scheduled_paths
        .lock()
        .map_err(|e| anyhow::anyhow!("Promotion schedule lock poisoned: {e}"))?;
    Ok(files
        .into_iter()
        .filter(|file| scheduled.insert(file.clone()))
        .collect())
}

impl<B> PromotionTask for PromotionWork<B>
where
    B: StagedPromotionBackend,
{
    type Output = PromotionResult;

    fn execute(self) -> Self::Output {
        Self::execute(self)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        any::Any,
        sync::{Arc, Mutex},
    };

    use nautilus_model::data::Data;
    use rstest::rstest;

    use super::{
        PromotionBackend, PromotionSession, PromotionSink, PromotionWork, StagedPromotionBackend,
    };
    use crate::{
        common::{conversion::FeatherConversionSummary, storage::create_storage_backend_from_path},
        writer::{
            run::{FeatherSessionSource, RunStatus},
            traits::StreamingDataSink,
        },
    };

    #[derive(Debug, Default)]
    struct RecordingState {
        converted: Vec<(String, bool)>,
        deleted: Vec<String>,
        recorded_states: Vec<RunStatus>,
    }

    #[derive(Debug)]
    struct RecordingBackend {
        leftovers: Vec<String>,
        state: Arc<Mutex<RecordingState>>,
    }

    impl PromotionBackend for RecordingBackend {
        type Source = FeatherSessionSource;

        const NAME: &'static str = "Recording";

        fn convert_file(
            &mut self,
            _source: &FeatherSessionSource,
            file: &str,
            _use_ts_event_for_ts_init: bool,
            record_promoted: bool,
        ) -> anyhow::Result<Option<FeatherConversionSummary>> {
            self.state
                .lock()
                .unwrap()
                .converted
                .push((file.to_string(), record_promoted));
            Ok(Some(FeatherConversionSummary {
                type_name: "quotes".to_string(),
                identifier: None,
                feather_path: file.to_string(),
                native_version: Some(1),
                unmatched_identifiers: None,
            }))
        }

        fn delete_file(
            &mut self,
            _source: &FeatherSessionSource,
            file: &str,
        ) -> anyhow::Result<()> {
            self.state.lock().unwrap().deleted.push(file.to_string());
            Ok(())
        }
    }

    impl StagedPromotionBackend for RecordingBackend {
        fn record_run_state(
            &mut self,
            _source: &FeatherSessionSource,
            _staging_uri: &str,
            status: RunStatus,
            _empty: bool,
            _error: Option<&str>,
        ) -> anyhow::Result<()> {
            self.state.lock().unwrap().recorded_states.push(status);
            Ok(())
        }

        fn delete_recorded_leftovers(
            &mut self,
            _source: &FeatherSessionSource,
            files: &[String],
        ) -> anyhow::Result<Vec<String>> {
            Ok(files
                .iter()
                .filter(|file| self.leftovers.contains(file))
                .cloned()
                .collect())
        }
    }

    #[rstest]
    fn recorded_leftovers_are_deleted_and_skipped_before_conversion() {
        let temp = tempfile::TempDir::new().unwrap();
        let storage =
            create_storage_backend_from_path(temp.path().to_str().unwrap(), None).unwrap();
        let source = FeatherSessionSource::new(storage, "backtest", "run-leftovers");
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let backend = RecordingBackend {
            leftovers: vec!["backtest/run-leftovers/b.feather".to_string()],
            state: Arc::clone(&state),
        };
        let files = vec![
            "backtest/run-leftovers/a.feather".to_string(),
            "backtest/run-leftovers/b.feather".to_string(),
            "backtest/run-leftovers/c.feather".to_string(),
        ];

        let result = PromotionWork::new(backend, source, "staging".to_string(), files, false, true)
            .execute();

        let converted = result.converted.unwrap();
        let state = state.lock().unwrap();
        assert_eq!(
            converted
                .iter()
                .map(|summary| summary.feather_path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "backtest/run-leftovers/a.feather",
                "backtest/run-leftovers/c.feather",
            ],
        );
        assert_eq!(
            result.committed_paths,
            vec![
                "backtest/run-leftovers/a.feather",
                "backtest/run-leftovers/c.feather",
            ],
        );
        // The leftover is deleted without conversion; the converted files are deleted
        // by the configured post-commit deletion.
        assert_eq!(
            result.deleted_paths,
            vec![
                "backtest/run-leftovers/b.feather",
                "backtest/run-leftovers/a.feather",
                "backtest/run-leftovers/c.feather",
            ],
        );
        assert_eq!(
            state.converted,
            vec![
                ("backtest/run-leftovers/a.feather".to_string(), false),
                ("backtest/run-leftovers/c.feather".to_string(), true),
            ],
        );
        assert_eq!(
            state.deleted,
            vec![
                "backtest/run-leftovers/a.feather",
                "backtest/run-leftovers/c.feather",
            ],
        );
        assert_eq!(state.recorded_states, vec![RunStatus::Promoted]);
        assert!(result.run_state_recorded);
    }

    #[derive(Debug)]
    struct FailingBackgroundSink {
        calls: Vec<&'static str>,
        promote_on_flush: bool,
        promote_on_close: bool,
    }

    impl FailingBackgroundSink {
        const fn new(promote_on_flush: bool, promote_on_close: bool) -> Self {
            Self {
                calls: Vec::new(),
                promote_on_flush,
                promote_on_close,
            }
        }
    }

    impl PromotionSink for FailingBackgroundSink {
        fn stage_data(&mut self, _data: Data) -> anyhow::Result<()> {
            unreachable!()
        }

        fn stage_batch(&mut self, _data: Vec<Data>) -> anyhow::Result<()> {
            unreachable!()
        }

        fn stage_any(&mut self, _message: &dyn Any) -> anyhow::Result<bool> {
            unreachable!()
        }

        fn flush_staging(&mut self) -> anyhow::Result<()> {
            self.calls.push("flush_staging");
            Ok(())
        }

        fn close_staging(&mut self) -> anyhow::Result<()> {
            self.calls.push("close_staging");
            Ok(())
        }

        fn mark_run_non_empty(&mut self) -> anyhow::Result<()> {
            unreachable!()
        }

        fn maybe_promote_by_period(&mut self) -> anyhow::Result<()> {
            unreachable!()
        }

        fn wait_for_background_promotions(
            &mut self,
        ) -> anyhow::Result<Vec<FeatherConversionSummary>> {
            self.calls.push("wait_for_background_promotions");
            anyhow::bail!("background promotion failed")
        }

        fn stop_periodic_promotion(&mut self) {
            self.calls.push("stop_periodic_promotion");
        }

        fn take_periodic_promotion_error(&mut self) -> anyhow::Result<()> {
            self.calls.push("take_periodic_promotion_error");
            Ok(())
        }

        fn should_promote_on_flush(&self) -> bool {
            self.promote_on_flush
        }

        fn should_promote_on_close(&self) -> bool {
            self.promote_on_close
        }

        fn promote(&mut self) -> anyhow::Result<Vec<FeatherConversionSummary>> {
            self.calls.push("promote");
            Ok(Vec::new())
        }

        fn record_completed(&mut self) {
            self.calls.push("record_completed");
        }
    }

    #[rstest]
    fn promotion_session_parses_windows_drive_path() {
        let session =
            PromotionSession::from_uri(r"C:\catalog\backtest\run-1").expect("valid run path");

        assert_eq!(session.catalog_uri, "C:/catalog");
        assert_eq!(session.kind, "backtest");
        assert_eq!(session.instance_id, "run-1");
    }

    #[rstest]
    fn flush_finalizes_and_promotes_before_returning_background_error() {
        let mut sink = FailingBackgroundSink::new(true, false);

        let e = StreamingDataSink::flush(&mut sink).unwrap_err();

        assert_eq!(e.to_string(), "background promotion failed");
        assert_eq!(
            sink.calls,
            [
                "flush_staging",
                "wait_for_background_promotions",
                "promote",
                "take_periodic_promotion_error",
            ]
        );
    }

    #[rstest]
    fn close_finalizes_and_promotes_before_returning_background_error() {
        let mut sink = FailingBackgroundSink::new(false, true);

        let e = StreamingDataSink::close(&mut sink).unwrap_err();

        assert_eq!(e.to_string(), "background promotion failed");
        assert_eq!(
            sink.calls,
            [
                "stop_periodic_promotion",
                "close_staging",
                "wait_for_background_promotions",
                "take_periodic_promotion_error",
                "promote",
            ]
        );
    }
}
