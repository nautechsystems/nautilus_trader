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

//! Run lifecycle and kernel boot integration for the event store.
//!
//! This module owns the kernel side of the SPEC's run lifecycle: it scans the on-disk
//! instance directory for crashed predecessors before a fresh run opens, seals each
//! survivor, opens the new run, blocks `start()` until the writer acknowledges the
//! `RunStarted` entry, and seals the manifest with a final `RunEnded` entry on graceful
//! stop. The writer's halt callback is wrapped in a typed [`HaltSignal`] that the kernel
//! caller polls to convert a fail-stop into kernel shutdown rather than a panic.

use std::{
    any::Any,
    cell::RefCell,
    fmt::Debug,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use bytes::Bytes;
use nautilus_common::{
    cache::{Cache, CacheSnapshotRef},
    clock::Clock,
    enums::Environment,
    msgbus::{self, BusTap, Endpoint, MStr, MessagingSwitchboard},
};
#[cfg(feature = "live")]
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_core::{
    UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_static},
};
use nautilus_execution::engine::SnapshotAnchorer;
use nautilus_system::{
    KernelEventStore as KernelEventStoreTrait, RegisteredComponents,
    event_store::{DataMarkerClass, DataMarkerConfig, EventStoreConfig, RetentionMode},
};
use ustr::Ustr;

use crate::{
    BusCaptureAdapter, CacheReplayError, CacheReplayReport, CaptureError, EncoderRegistry,
    EntryDraft, EventStore, EventStoreError, EventStoreWriter, HaltCallback, HaltReason, Headers,
    RedbBackend, RunId, RunManifest, RunStatus, ScanDirection, Topic, WriterConfig,
    compute_snapshot_content_hash, default_registry,
    markers::{
        DataClass, DataMarkerCapture, DataMarkerExtractorRegistry, MarkerBackend, MarkerManifest,
        MarkerWriter, MarkerWriterConfig, RedbMarkerBackend,
    },
    restore_cache_from_sealed_run, validate_event_store_replay_source,
};

const RUN_STARTED_TOPIC: &str = "run.lifecycle.RunStarted";
const RUN_STARTED_PAYLOAD_TYPE: &str = "RunStarted";
const RUN_ENDED_TOPIC: &str = "run.lifecycle.RunEnded";
const RUN_ENDED_PAYLOAD_TYPE: &str = "RunEnded";

/// The outcome of sealing a single crashed predecessor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveredRun {
    /// The id of the sealed predecessor.
    pub run_id: RunId,
    /// The terminal status applied: [`RunStatus::CrashedRecovered`] or
    /// [`RunStatus::Quarantined`].
    pub status: RunStatus,
}

/// Result of the predecessor recovery sweep performed in the kernel constructor.
#[derive(Debug, Default)]
pub struct RecoveryOutcome {
    /// One entry per predecessor that was sealed by the sweep.
    pub recovered: Vec<RecoveredRun>,
    /// The id of the most-recently-crashed predecessor sealed as
    /// [`RunStatus::CrashedRecovered`], or `None` when no recoverable predecessor
    /// existed (or every predecessor was quarantined).
    pub parent_run_id: Option<RunId>,
}

type RegistryFactory = dyn Fn() -> EncoderRegistry + Send + Sync + 'static;
type BackendOpenResult = Result<Box<dyn EventStore + Send>, EventStoreError>;
type BackendOpener =
    dyn Fn(&EventStoreConfig, &RunManifest) -> BackendOpenResult + Send + Sync + 'static;
type MarkerRegistryFactory =
    dyn Fn(&[DataClass]) -> DataMarkerExtractorRegistry + Send + Sync + 'static;
type SharedMarkerCapture = Rc<RefCell<Option<DataMarkerCapture>>>;

/// Non-serialized lifecycle policy for advanced event-store callers.
///
/// [`EventStoreConfig`] remains the serializable run policy. This type carries process-local
/// construction choices, such as the encoder registry and backend opener used when a kernel
/// opens a run.
#[derive(Clone)]
pub struct EventStoreLifecycleOptions {
    registry_factory: Arc<RegistryFactory>,
    backend_opener: Arc<BackendOpener>,
    marker_registry_factory: Arc<MarkerRegistryFactory>,
}

impl Debug for EventStoreLifecycleOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EventStoreLifecycleOptions))
            .finish_non_exhaustive()
    }
}

impl Default for EventStoreLifecycleOptions {
    fn default() -> Self {
        Self {
            registry_factory: Arc::new(default_registry),
            backend_opener: Arc::new(default_backend_opener),
            marker_registry_factory: Arc::new(DataMarkerExtractorRegistry::default_registry),
        }
    }
}

impl EventStoreLifecycleOptions {
    /// Creates options that use [`default_registry`] and [`RedbBackend`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses a caller-supplied encoder registry factory for each opened run.
    #[must_use]
    pub fn with_registry_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> EncoderRegistry + Send + Sync + 'static,
    {
        self.registry_factory = Arc::new(factory);
        self
    }

    /// Uses a caller-supplied encoder registry for each opened run.
    #[must_use]
    pub fn with_encoder_registry(self, registry: EncoderRegistry) -> Self {
        self.with_registry_factory(move || registry.clone())
    }

    /// Uses a caller-supplied backend opener for each opened run.
    #[must_use]
    pub fn with_backend_opener<F>(mut self, opener: F) -> Self
    where
        F: Fn(&EventStoreConfig, &RunManifest) -> BackendOpenResult + Send + Sync + 'static,
    {
        self.backend_opener = Arc::new(opener);
        self
    }

    /// Uses a caller-supplied data-marker extractor registry factory for each opened run.
    #[must_use]
    pub fn with_marker_registry_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn(&[DataClass]) -> DataMarkerExtractorRegistry + Send + Sync + 'static,
    {
        self.marker_registry_factory = Arc::new(factory);
        self
    }

    fn build_registry(&self) -> EncoderRegistry {
        (self.registry_factory)()
    }

    fn open_backend(&self, config: &EventStoreConfig, manifest: &RunManifest) -> BackendOpenResult {
        (self.backend_opener)(config, manifest)
    }

    fn build_marker_registry(&self, classes: &[DataClass]) -> DataMarkerExtractorRegistry {
        (self.marker_registry_factory)(classes)
    }
}

fn default_backend_opener(config: &EventStoreConfig, manifest: &RunManifest) -> BackendOpenResult {
    let mut backend = RedbBackend::new(config.base_dir.clone());
    backend.open_run(manifest.clone())?;
    Ok(Box::new(backend))
}

/// Errors surfaced by the boot path.
#[derive(Debug, thiserror::Error)]
pub enum BootError {
    /// The event store backend rejected an open, scan, or seal during recovery or
    /// new-run creation.
    #[error(transparent)]
    EventStore(#[from] EventStoreError),
    /// The writer rejected the `RunStarted` submit.
    #[error("RunStarted submit failed: {0}")]
    RunStartedSubmit(String),
    /// The writer accepted `RunStarted` but did not durably commit it inside the
    /// configured timeout.
    #[error("RunStarted did not durably commit within {timeout:?}")]
    RunStartedTimeout {
        /// The configured ceiling that elapsed before the writer's high-watermark
        /// advanced.
        timeout: Duration,
    },
    /// The writer signaled fail-stop while the boot path was waiting for the
    /// `RunStarted` entry to commit.
    #[error("event store halted during boot: {0:?}")]
    HaltedDuringBoot(HaltReason),
}

/// A thread-safe halt signal the kernel registers with the writer.
///
/// The writer thread fires the callback once on the first unrecoverable condition;
/// the kernel polls [`HaltSignal::is_halted`] and converts it into a typed kernel
/// shutdown rather than letting the writer-thread error escape as a panic.
#[derive(Clone, Debug)]
pub struct HaltSignal {
    halted: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<HaltReason>>>,
}

impl Default for HaltSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl HaltSignal {
    /// Constructs a fresh, un-fired halt signal.
    #[must_use]
    pub fn new() -> Self {
        Self {
            halted: Arc::new(AtomicBool::new(false)),
            reason: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns the [`HaltCallback`] the writer fires when an unrecoverable condition
    /// occurs.
    ///
    /// The callback records the [`HaltReason`] (preserving only the first one when
    /// multiple submits race past the halt threshold) and then flips the halted flag,
    /// so a poller that observes `is_halted()` never reads back an empty reason.
    #[must_use]
    pub fn callback(&self) -> HaltCallback {
        let halted = Arc::clone(&self.halted);
        let reason = Arc::clone(&self.reason);
        Arc::new(move |r| {
            // The mutex gates first-reason-wins; the flag flips after the reason is
            // stored. On the (panic-only) poisoned path the reason is lost but the
            // halt itself must still be observable.
            if let Ok(mut slot) = reason.lock()
                && slot.is_none()
            {
                *slot = Some(r);
            }
            halted.store(true, Ordering::Release);
        })
    }

    /// Returns whether the writer has signaled fail-stop.
    #[must_use]
    pub fn is_halted(&self) -> bool {
        self.halted.load(Ordering::Acquire)
    }

    /// Returns the [`HaltReason`] recorded on the first fail-stop, if any.
    ///
    /// Calling this does not clear the signal; the kernel's halted flag remains set so
    /// subsequent submits surface as fail-stopped.
    #[must_use]
    pub fn reason(&self) -> Option<HaltReason> {
        self.reason.lock().ok().and_then(|guard| guard.clone())
    }
}

/// Live event-store session owned by the kernel between `start()` and `finalize_stop()`.
pub struct EventStoreSession {
    writer: Option<Arc<EventStoreWriter>>,
    adapter: Option<Arc<BusCaptureAdapter>>,
    marker_capture: Option<SharedMarkerCapture>,
    manifest: RunManifest,
    halt_signal: HaltSignal,
}

impl Debug for EventStoreSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EventStoreSession))
            .field("run_id", &self.manifest.run_id)
            .field("parent_run_id", &self.manifest.parent_run_id)
            .field("instance_id", &self.manifest.instance_id)
            .field("halted", &self.halt_signal.is_halted())
            .field("writer_attached", &self.writer.is_some())
            .field("marker_capture_attached", &self.marker_capture.is_some())
            .finish_non_exhaustive()
    }
}

impl EventStoreSession {
    /// Returns the captured manifest as it was written to disk at run start.
    ///
    /// The high-watermark and `end_ts_init` advance after seal; the snapshot here is
    /// frozen at boot time.
    #[must_use]
    pub const fn manifest(&self) -> &RunManifest {
        &self.manifest
    }

    /// Returns the run id of the currently open run.
    #[must_use]
    pub fn run_id(&self) -> &str {
        self.manifest.run_id.as_str()
    }

    /// Returns the parent run id for the current run.
    #[must_use]
    pub fn parent_run_id(&self) -> Option<&str> {
        self.manifest.parent_run_id.as_deref()
    }

    /// Returns whether the writer has fail-stopped.
    #[must_use]
    pub fn is_halted(&self) -> bool {
        self.halt_signal.is_halted()
    }

    /// Returns the writer's current durable high-watermark.
    ///
    /// Returns `0` when the writer has been consumed by a prior `close`.
    #[must_use]
    pub fn high_watermark(&self) -> u64 {
        self.writer.as_ref().map_or(0, |w| w.high_watermark())
    }

    /// Returns a snapshot anchorer bound to the open writer.
    ///
    /// The execution engine installs this callback while the run is open. The callback
    /// records the cache-owned snapshot reference against the writer's durable
    /// high-watermark after flushing earlier captured entries.
    #[must_use]
    pub fn snapshot_anchorer(&self) -> Option<SnapshotAnchorer> {
        let writer = Arc::clone(self.writer.as_ref()?);

        Some(Rc::new(move |snapshot_ref: CacheSnapshotRef| {
            let content_hash = compute_snapshot_content_hash(snapshot_ref.blob.as_ref());
            writer
                .record_snapshot_anchor(snapshot_ref.blob_ref, content_hash)
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("record snapshot anchor: {e}"))
        }))
    }

    /// Returns the live bus capture adapter, when one was wired into this run.
    ///
    /// `None` after [`Self::close`] consumes the writer.
    #[must_use]
    pub fn adapter(&self) -> Option<&Arc<BusCaptureAdapter>> {
        self.adapter.as_ref()
    }

    /// Submits the terminal `RunEnded` entry, drains pending entries, and seals the
    /// manifest as [`RunStatus::Ended`].
    ///
    /// Consumes the inner writer; subsequent calls return without effect.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError`] if the writer fails to commit the final batch, the
    /// seal step fails, or the writer Arc has outstanding clones (the bus tap must be
    /// cleared before close to release the adapter's writer reference).
    pub fn close(&mut self, ts_init: UnixNanos) -> Result<(), EventStoreError> {
        // Drop the adapter first so the writer Arc has no other strong owners on
        // try_unwrap. The kernel clears the bus tap before this site, so dropping the
        // session-side adapter clone here is the last release before close.
        self.adapter = None;
        let marker_capture = self.marker_capture.take();

        let Some(writer_arc) = self.writer.take() else {
            close_marker_capture(marker_capture);
            return Ok(());
        };
        let Ok(writer) = Arc::try_unwrap(writer_arc) else {
            close_marker_capture(marker_capture);
            return Err(EventStoreError::Backend(
                "event store writer has multiple owners; clear the bus tap before close"
                    .to_string(),
            ));
        };

        let run_ended = run_ended_draft(ts_init);
        let result = writer.close(run_ended);
        close_marker_capture(marker_capture);
        result?;
        Ok(())
    }
}

impl Drop for EventStoreSession {
    fn drop(&mut self) {
        // Drop without close: release adapter then writer so the writer thread exits
        // unsealed; the next boot recovers.
        self.adapter.take();
        self.marker_capture.take();
        self.writer.take();
    }
}

fn close_marker_capture(marker_capture: Option<SharedMarkerCapture>) {
    if let Some(marker_capture) = marker_capture
        && let Some(capture) = marker_capture.borrow_mut().take()
    {
        capture.close();
    }
}

/// Typed error surfaced when the event store fails the run lifecycle.
///
/// Wraps the boot-time and shutdown-time failure modes so a kernel caller can react to a
/// fail-stop without inspecting individual writer/backend errors.
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    /// The event-store boot path failed.
    #[error("event store boot failed: {0}")]
    EventStoreBoot(#[from] BootError),
    /// Cache state reconstruction from a recovered event-store run failed.
    #[error("event store cache replay failed: {0}")]
    CacheReplay(#[from] CacheReplayError),
    /// The writer signaled fail-stop after the kernel was already started.
    #[error("event store halted: {0:?}")]
    EventStoreHalted(HaltReason),
}

/// Kernel-facing wrapper that bundles every event-store concern: predecessor recovery,
/// the open run, the halt signal, and the seal-on-drop fail-safe.
///
/// One instance is typically owned by [`nautilus_system::NautilusKernel`] via the
/// [`KernelEventStoreTrait`] seam: the kernel calls [`EventStoreLifecycle::open`] from
/// `start()`, [`EventStoreLifecycle::seal`] from `finalize_stop()` / `dispose()`, and
/// the wrapper's [`Drop`] runs as the last-chance seal site for callers that skip both
/// teardown paths (e.g. imperative `engine.run(...)` followed by drop in
/// `BacktestEngine`).
#[derive(Debug)]
pub struct EventStoreLifecycle {
    config: Option<EventStoreConfig>,
    options: EventStoreLifecycleOptions,
    recovered: Vec<RecoveredRun>,
    parent_run_id: Option<String>,
    session: Option<EventStoreSession>,
    halt: HaltSignal,
    // Held so `Drop` can stamp the seal even when the kernel never called seal()
    // explicitly. Cloning the kernel's clock Rc keeps the wrapper independent of
    // its owner.
    clock: Rc<RefCell<dyn Clock>>,
}

impl EventStoreLifecycle {
    /// Boots the wrapper at kernel construction time.
    ///
    /// Runs the predecessor recovery sweep against `<base_dir>/<instance_id>/`. When
    /// `config` is `None` the wrapper is inert: every method becomes a no-op.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`EventStoreError`] when the recovery sweep fails for a
    /// reason other than the expected `CrashedPredecessor` handshake.
    pub fn boot(
        config: Option<EventStoreConfig>,
        instance_id: UUID4,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Self> {
        Self::boot_with_options(
            config,
            instance_id,
            clock,
            EventStoreLifecycleOptions::default(),
        )
    }

    /// Boots the wrapper at kernel construction time with process-local lifecycle options.
    ///
    /// `EventStoreConfig` remains serializable. `options` carries runtime-only construction
    /// policy for the encoder registry and backend opener.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`EventStoreError`] when the recovery sweep fails for a
    /// reason other than the expected `CrashedPredecessor` handshake.
    pub fn boot_with_options(
        config: Option<EventStoreConfig>,
        instance_id: UUID4,
        clock: Rc<RefCell<dyn Clock>>,
        options: EventStoreLifecycleOptions,
    ) -> anyhow::Result<Self> {
        let (recovered, parent_run_id) = if let Some(cfg) = config.as_ref() {
            let outcome = recover_predecessors(&cfg.base_dir, &instance_id.to_string())?;
            if !outcome.recovered.is_empty() {
                log::info!(
                    "Sealed {} crashed event-store predecessor(s); parent_run_id={:?}",
                    outcome.recovered.len(),
                    outcome.parent_run_id,
                );
            }
            (outcome.recovered, outcome.parent_run_id)
        } else {
            (Vec::new(), None)
        };
        Ok(Self {
            config,
            options,
            recovered,
            parent_run_id,
            session: None,
            halt: HaltSignal::new(),
            clock,
        })
    }

    /// Opens a fresh run on kernel `start()`. Idempotent against reset/rerun: a
    /// leftover session from a prior `start()` is sealed before a new one opens, so
    /// `RunStarted` remains the first entry of every run.
    ///
    /// `components` is the manifest captured into the `RunStarted` payload. `environment`
    /// selects the static (backtest) or realtime (live) clock used to stamp `ts_publish`
    /// inside the writer.
    ///
    /// Returns without effect when no event-store config was supplied.
    ///
    /// # Errors
    ///
    /// Returns [`KernelError::EventStoreBoot`] when opening the new run, spawning the
    /// writer, or blocking on the `RunStarted` ack fails.
    pub fn open(
        &mut self,
        instance_id: UUID4,
        components: &RegisteredComponents,
        environment: Environment,
    ) -> Result<(), KernelError> {
        let Some(config) = self.config.clone() else {
            return Ok(());
        };

        if self.session.is_some() {
            // Reset/rerun (BacktestEngine::run -> reset -> run) reuses the kernel
            // across runs. Seal the leftover session before opening a fresh one.
            let ts = self.clock.borrow().timestamp_ns();
            self.seal(ts);
        }

        // Re-arm the fail-stop signal: a halt is terminal for the run that fired it,
        // not for the kernel. A stale signal fails the rerun's boot or opens it
        // permanently halted, downgrading its graceful stop to CrashedRecovered.
        self.halt = HaltSignal::new();

        let clock = Self::clock_for(environment);
        let start_ts_init = self.clock.borrow().timestamp_ns();
        let run_id = build_run_id(start_ts_init);
        let parent_run_id = if let Some(replay_run_id) = config.replay_from_run_id.as_deref() {
            validate_event_store_replay_source(
                config.base_dir.clone(),
                &instance_id.to_string(),
                replay_run_id,
            )?;
            Some(replay_run_id.to_string())
        } else {
            self.parent_run_id.clone()
        };
        let session = open_run_with_options(
            &config,
            &instance_id.to_string(),
            run_id,
            parent_run_id,
            start_ts_init,
            components,
            self.halt.clone(),
            clock,
            &self.options,
        )?;
        log::info!(
            "Opened event-store run {} (parent_run_id={:?})",
            session.run_id(),
            session.parent_run_id(),
        );

        if let Some(adapter) = session.adapter() {
            install_bus_tap(Arc::clone(adapter), session.marker_capture.clone(), clock);
        }
        self.session = Some(session);
        Ok(())
    }

    /// Restores cache state from the configured replay run or recovered parent run.
    ///
    /// This is a bootstrap-only reconstruction path. It opens the sealed replay source
    /// for read-only replay, restores the cache-owned snapshot blob, then replays only
    /// the entries after the snapshot anchor directly into [`Cache`].
    ///
    /// # Errors
    ///
    /// Returns [`KernelError::CacheReplay`] when the source reader, snapshot restore, decode,
    /// or cache apply step fails.
    pub fn restore_parent_cache(
        &self,
        instance_id: UUID4,
        cache: &mut Cache,
    ) -> Result<Option<CacheReplayReport>, KernelError> {
        let Some(config) = self.config.as_ref() else {
            return Ok(None);
        };
        let replay_run_id = config
            .replay_from_run_id
            .as_deref()
            .or(self.parent_run_id.as_deref());
        let Some(replay_run_id) = replay_run_id else {
            return Ok(None);
        };
        let source = if config.replay_from_run_id.is_some() {
            "configured replay run"
        } else {
            "parent run"
        };

        let report = restore_cache_from_sealed_run(
            cache,
            config.base_dir.clone(),
            &instance_id.to_string(),
            replay_run_id,
        )?;

        log::info!(
            "Restored cache from event-store {source} {replay_run_id}: from_seq={}, to_seq={}, applied={}, ignored={}",
            report.cache.plan.from_seq,
            report.cache.plan.to_seq,
            report.cache.applied_entries,
            report.cache.ignored_entries,
        );

        Ok(Some(report.cache))
    }

    /// Seals the open session by writing `RunEnded` and updating the manifest to
    /// `Ended`. Idempotent: a closed or absent session makes this a no-op. Halted
    /// sessions skip the close (the recovery sweep on next boot owns the seal).
    pub fn seal(&mut self, ts_init: UnixNanos) {
        let Some(mut session) = self.session.take() else {
            return;
        };

        // Drop the bus tap before close so the adapter's writer Arc is released; the
        // close path then takes sole ownership of the writer and commits RunEnded.
        msgbus::clear_bus_tap();

        if session.is_halted() {
            log::warn!(
                "Event-store writer fail-stopped before close; run {} sealed by recovery sweep on next boot",
                session.run_id(),
            );
            return;
        }
        let run_id = session.run_id().to_string();
        if let Err(e) = session.close(ts_init) {
            log::error!(
                "Failed to seal event-store run {run_id} on graceful stop: {e}; run will be sealed as CrashedRecovered on next boot",
            );
        } else {
            log::info!("Sealed event-store run {run_id}");
        }
    }

    /// Returns the recovery report from the boot sweep.
    #[must_use]
    pub fn recovered(&self) -> &[RecoveredRun] {
        &self.recovered
    }

    /// Returns the configured replay source or recovered parent run id, when present.
    #[must_use]
    pub fn parent_run_id(&self) -> Option<&str> {
        self.config
            .as_ref()
            .and_then(|config| config.replay_from_run_id.as_deref())
            .or(self.parent_run_id.as_deref())
    }

    /// Returns whether this lifecycle is configured for event-store-only replay.
    #[must_use]
    pub fn is_event_store_replay_configured(&self) -> bool {
        self.config
            .as_ref()
            .is_some_and(|config| config.replay_from_run_id.is_some())
    }

    /// Returns the run id of the open session, when capture is active.
    #[must_use]
    pub fn run_id(&self) -> Option<&str> {
        self.session.as_ref().map(EventStoreSession::run_id)
    }

    /// Returns a snapshot anchorer for the open run, when capture is active.
    #[must_use]
    pub fn snapshot_anchorer(&self) -> Option<SnapshotAnchorer> {
        self.session
            .as_ref()
            .and_then(EventStoreSession::snapshot_anchorer)
    }

    /// Returns whether the writer has signaled fail-stop.
    #[must_use]
    pub fn is_halted(&self) -> bool {
        self.halt.is_halted()
    }

    /// Returns the [`HaltReason`] recorded on the first fail-stop, if any.
    #[must_use]
    pub fn halt_reason(&self) -> Option<HaltReason> {
        self.halt.reason()
    }

    /// Surfaces the current halt as a typed [`KernelError`], or `None` when the
    /// writer has not halted.
    #[must_use]
    pub fn check_halt(&self) -> Option<KernelError> {
        self.halt_reason().map(KernelError::EventStoreHalted)
    }

    #[cfg(feature = "live")]
    fn clock_for(environment: Environment) -> &'static AtomicTime {
        match environment {
            Environment::Backtest => get_atomic_clock_static(),
            Environment::Live | Environment::Sandbox => get_atomic_clock_realtime(),
        }
    }

    #[cfg(not(feature = "live"))]
    fn clock_for(_environment: Environment) -> &'static AtomicTime {
        get_atomic_clock_static()
    }
}

impl Drop for EventStoreLifecycle {
    fn drop(&mut self) {
        // Last-chance seal: callers may skip both finalize_stop() and dispose().
        if self.session.is_none() {
            return;
        }
        let ts = self
            .clock
            .try_borrow()
            .map(|c| c.timestamp_ns())
            .unwrap_or_default();
        self.seal(ts);
    }
}

/// Sweeps `<base_dir>/<instance_id>/` for crashed predecessor runs and seals each one.
///
/// A predecessor is a run file whose manifest still reads [`RunStatus::Running`]: the
/// previous trader exited (cleanly via drop, or crashed) without sealing. The sweep
/// scans every entry in the run, validating hashes; on success the manifest seals as
/// [`RunStatus::CrashedRecovered`], otherwise as [`RunStatus::Quarantined`]. The
/// most-recently-crashed survivor's `run_id` is returned so the new run records it as
/// `parent_run_id`.
///
/// Quarantined runs do not become parents: a future replay must skip the corrupted
/// tail rather than chain through it.
///
/// A predecessor that cannot be reopened, scanned, or sealed is skipped with a logged
/// error rather than failing the sweep: recovery must never leave the trader unbootable
/// because one run file is damaged. Skipped runs keep their on-disk status, so the next
/// boot retries them.
///
/// # Errors
///
/// Returns [`EventStoreError`] when the directory enumeration fails, or when a
/// predecessor unexpectedly reopens without the
/// [`EventStoreError::CrashedPredecessor`] handshake the backend uses to surface
/// unsealed runs.
pub fn recover_predecessors(
    base_dir: &Path,
    instance_id: &str,
) -> Result<RecoveryOutcome, EventStoreError> {
    let manifests = RedbBackend::list_runs(base_dir, instance_id)?;
    let crashed: Vec<RunManifest> = manifests
        .into_iter()
        .filter(|m| matches!(m.status, RunStatus::Running))
        .collect();

    let mut outcome = RecoveryOutcome::default();

    for predecessor in crashed {
        let run_id = predecessor.run_id.clone();
        let mut backend = RedbBackend::new(base_dir.to_path_buf());

        match backend.open_run(predecessor) {
            Err(EventStoreError::CrashedPredecessor) => {}
            Ok(()) => {
                return Err(EventStoreError::Backend(format!(
                    "expected CrashedPredecessor reopening {run_id}, was Ok",
                )));
            }
            Err(other) => {
                log::error!("Skipping recovery of run {run_id}, reopen failed: {other}");
                continue;
            }
        }

        let high_watermark = backend.high_watermark()?;
        let final_status = if high_watermark == 0 {
            RunStatus::CrashedRecovered
        } else {
            match backend.scan_range(1, high_watermark, ScanDirection::Forward) {
                Ok(entries) => {
                    // The writer commits RunEnded before seal; a crash between those
                    // two steps leaves a graceful tail without a sealed manifest.
                    // Honor the tail: if the last entry is the kernel's RunEnded
                    // marker, the predecessor closed cleanly and is not a crash to
                    // chain through. Match both topic and payload_type so a future
                    // capture-registry entry that happens to share the payload tag
                    // cannot be misclassified as a graceful close.
                    let tail_is_run_ended = entries.last().is_some_and(|e| {
                        e.topic.as_ref() == RUN_ENDED_TOPIC
                            && e.payload_type.as_str() == RUN_ENDED_PAYLOAD_TYPE
                    });

                    if tail_is_run_ended {
                        RunStatus::Ended
                    } else {
                        RunStatus::CrashedRecovered
                    }
                }
                Err(
                    EventStoreError::HashMismatch { .. }
                    | EventStoreError::Corrupted(_)
                    | EventStoreError::Gap { .. },
                ) => RunStatus::Quarantined,
                Err(other) => {
                    log::error!("Skipping recovery of run {run_id}, scan failed: {other}");
                    continue;
                }
            }
        };

        if let Err(e) = backend.seal(final_status) {
            log::error!("Skipping recovery of run {run_id}, seal as {final_status:?} failed: {e}");
            continue;
        }
        outcome.recovered.push(RecoveredRun {
            run_id: run_id.clone(),
            status: final_status,
        });

        if matches!(final_status, RunStatus::CrashedRecovered) {
            outcome.parent_run_id = Some(run_id);
        }
    }

    Ok(outcome)
}

/// Builds the `<start_ts_init>-<short_uuid>` run id used as the manifest key and on-disk
/// file name.
///
/// The id is sortable by start time so directory listings produce chronological order;
/// the short uuid suffix keeps it unique even when two kernels start at the same
/// nanosecond on different machines.
#[must_use]
pub fn build_run_id(start_ts_init: UnixNanos) -> RunId {
    let suffix: String = UUID4::new().to_string().chars().take(8).collect();
    format!("{}-{suffix}", u64::from(start_ts_init))
}

/// Opens a fresh run, spawns the writer, and submits a blocking `RunStarted` entry.
///
/// The kernel calls this from `start()` after components have registered with the
/// trader so the captured `RunStarted` payload reflects the actual boot configuration.
/// The function blocks until the writer's high-watermark advances past zero (i.e. the
/// `RunStarted` entry has durably committed) or until [`EventStoreConfig::run_started_timeout`]
/// elapses.
///
/// `feature_flags` is appended after the configured `feature_flags` so the retention
/// mode survives in the manifest as `retention=<mode>`.
///
/// # Errors
///
/// Returns [`BootError::EventStore`] when the backend rejects open, [`BootError::RunStartedSubmit`]
/// when the writer rejects the submit, [`BootError::RunStartedTimeout`] when the
/// commit does not happen inside the configured ceiling, and [`BootError::HaltedDuringBoot`]
/// when the writer fail-stops while waiting for the commit.
#[allow(clippy::too_many_arguments)]
pub fn open_run(
    config: &EventStoreConfig,
    instance_id: &str,
    run_id: RunId,
    parent_run_id: Option<RunId>,
    start_ts_init: UnixNanos,
    components: &RegisteredComponents,
    halt_signal: HaltSignal,
    clock: &'static AtomicTime,
) -> Result<EventStoreSession, BootError> {
    open_run_with_options(
        config,
        instance_id,
        run_id,
        parent_run_id,
        start_ts_init,
        components,
        halt_signal,
        clock,
        &EventStoreLifecycleOptions::default(),
    )
}

/// Opens a fresh run with process-local lifecycle options.
///
/// This follows [`open_run`] but obtains the backend and encoder registry from
/// `options`.
///
/// # Errors
///
/// Returns [`BootError::EventStore`] when the backend rejects open, [`BootError::RunStartedSubmit`]
/// when the writer rejects the submit, [`BootError::RunStartedTimeout`] when the
/// commit does not happen inside the configured ceiling, and [`BootError::HaltedDuringBoot`]
/// when the writer fail-stops while waiting for the commit.
#[allow(clippy::too_many_arguments)]
pub fn open_run_with_options(
    config: &EventStoreConfig,
    instance_id: &str,
    run_id: RunId,
    parent_run_id: Option<RunId>,
    start_ts_init: UnixNanos,
    components: &RegisteredComponents,
    halt_signal: HaltSignal,
    clock: &'static AtomicTime,
    options: &EventStoreLifecycleOptions,
) -> Result<EventStoreSession, BootError> {
    let manifest = build_manifest(
        config,
        instance_id,
        run_id,
        parent_run_id,
        start_ts_init,
        components.clone(),
    );

    let backend = options.open_backend(config, &manifest)?;

    let writer = Arc::new(EventStoreWriter::spawn(
        backend,
        clock,
        halt_signal.callback(),
        writer_config_from(config),
    )?);

    submit_run_started_blocking(
        &writer,
        components,
        start_ts_init,
        &halt_signal,
        config.run_started_timeout,
    )?;

    let (marker_capture, submit_counter) =
        build_marker_capture(config, &manifest, writer.high_watermark(), clock, options);
    let mut adapter = BusCaptureAdapter::new(
        Arc::clone(&writer),
        Arc::new(options.build_registry()),
        halt_signal.callback(),
    );

    if let Some(submit_counter) = submit_counter {
        adapter = adapter.with_submit_counter(submit_counter);
    }
    let adapter = Arc::new(adapter);

    Ok(EventStoreSession {
        writer: Some(writer),
        adapter: Some(adapter),
        marker_capture,
        manifest,
        halt_signal,
    })
}

fn build_marker_capture(
    config: &EventStoreConfig,
    manifest: &RunManifest,
    initial_submit_counter: u64,
    clock: &'static AtomicTime,
    options: &EventStoreLifecycleOptions,
) -> (Option<SharedMarkerCapture>, Option<Arc<AtomicU64>>) {
    let Some(marker_config) = config.data_markers.as_ref() else {
        return (None, None);
    };

    match open_marker_capture(
        config,
        manifest,
        marker_config,
        initial_submit_counter,
        clock,
        options,
    ) {
        Ok((capture, submit_counter)) => (
            Some(Rc::new(RefCell::new(Some(capture)))),
            Some(submit_counter),
        ),
        Err(e) => {
            log::warn!(
                "Data marker sidecar disabled for run {} after marker setup failed: {e}",
                manifest.run_id,
            );
            (None, None)
        }
    }
}

fn open_marker_capture(
    config: &EventStoreConfig,
    manifest: &RunManifest,
    marker_config: &DataMarkerConfig,
    initial_submit_counter: u64,
    clock: &'static AtomicTime,
    options: &EventStoreLifecycleOptions,
) -> Result<(DataMarkerCapture, Arc<AtomicU64>), EventStoreError> {
    let classes = marker_config
        .classes
        .iter()
        .copied()
        .map(data_marker_class_to_data_class)
        .collect::<Vec<_>>();
    let marker_manifest = marker_manifest_for(manifest, classes.clone(), marker_config);
    let marker_path = marker_file_path(config, &manifest.instance_id, &manifest.run_id);
    let mut marker_backend = RedbMarkerBackend::new(marker_path);
    marker_backend.open_run(marker_manifest)?;
    let writer = MarkerWriter::spawn(
        Box::new(marker_backend),
        clock,
        MarkerWriterConfig {
            channel_capacity: marker_config.channel_capacity,
            ..MarkerWriterConfig::default()
        },
    )?;
    let submit_counter = Arc::new(AtomicU64::new(initial_submit_counter));
    let registry = options.build_marker_registry(&classes);
    let capture =
        DataMarkerCapture::new(registry, writer, Arc::clone(&submit_counter), marker_config);

    Ok((capture, submit_counter))
}

fn marker_file_path(config: &EventStoreConfig, instance_id: &str, run_id: &str) -> PathBuf {
    config
        .base_dir
        .join(instance_id)
        .join(format!("{run_id}.markers.redb"))
}

fn marker_manifest_for(
    manifest: &RunManifest,
    enabled_classes: Vec<DataClass>,
    config: &DataMarkerConfig,
) -> MarkerManifest {
    MarkerManifest {
        run_id: manifest.run_id.clone(),
        enabled_classes,
        high_fidelity: !config.high_fidelity.is_empty(),
        snapshot_count: 0,
        hifi_count: 0,
        gap_count: 0,
        dict_count: 0,
        status: RunStatus::Running,
    }
}

const fn data_marker_class_to_data_class(class: DataMarkerClass) -> DataClass {
    match class {
        DataMarkerClass::BookDeltas => DataClass::BookDeltas,
        DataMarkerClass::BookDepth10 => DataClass::BookDepth10,
        DataMarkerClass::Quote => DataClass::Quote,
        DataMarkerClass::Trade => DataClass::Trade,
        DataMarkerClass::Bar => DataClass::Bar,
    }
}

fn build_manifest(
    config: &EventStoreConfig,
    instance_id: &str,
    run_id: RunId,
    parent_run_id: Option<RunId>,
    start_ts_init: UnixNanos,
    components: RegisteredComponents,
) -> RunManifest {
    let mut feature_flags = config.identity.feature_flags.clone();
    feature_flags.push(format!("retention={}", retention_tag(config.retention)));

    RunManifest {
        run_id,
        parent_run_id,
        instance_id: instance_id.to_string(),
        binary_hash: config.identity.binary_hash.clone(),
        schema_version: config.identity.schema_version,
        crate_versions: config.identity.crate_versions.clone(),
        feature_flags,
        adapter_versions: config.identity.adapter_versions.clone(),
        config_hash: config.identity.config_hash.clone(),
        registered_components: components,
        seed: config.identity.seed,
        start_ts_init,
        end_ts_init: None,
        high_watermark: 0,
        status: RunStatus::Running,
    }
}

const fn retention_tag(mode: RetentionMode) -> &'static str {
    match mode {
        RetentionMode::Full => "full",
        RetentionMode::Bounded { .. } => "bounded",
        RetentionMode::SnapshotAnchored => "snapshot",
    }
}

fn writer_config_from(config: &EventStoreConfig) -> WriterConfig {
    WriterConfig {
        channel_capacity: config.channel_capacity,
        max_batch_entries: config.max_batch_entries,
        max_batch_latency: config.max_batch_latency,
        halt_threshold: config.halt_threshold,
    }
}

/// Submits the `RunStarted` draft and blocks until the writer durably acknowledges it,
/// the writer fail-stops, or `timeout` elapses.
///
/// Exposed at `pub(crate)` so tests can drive it against a stub backend without going
/// through [`open_run`].
///
/// # Errors
///
/// Returns [`BootError::RunStartedSubmit`] when the writer rejects the submit,
/// [`BootError::HaltedDuringBoot`] when the writer fail-stops during the wait, and
/// [`BootError::RunStartedTimeout`] when the writer does not commit within `timeout`.
pub(crate) fn submit_run_started_blocking(
    writer: &EventStoreWriter,
    components: &RegisteredComponents,
    ts_init: UnixNanos,
    halt_signal: &HaltSignal,
    timeout: Duration,
) -> Result<(), BootError> {
    let payload = encode_run_started(components);
    let draft = EntryDraft::without_indices(
        Headers::empty(),
        Topic::from(RUN_STARTED_TOPIC),
        Ustr::from(RUN_STARTED_PAYLOAD_TYPE),
        payload,
        ts_init,
    );

    writer
        .submit(draft)
        .map_err(|e| BootError::RunStartedSubmit(e.to_string()))?;

    // Wall-clock timeout against the writer thread: the writer drives the seam,
    // not the kernel state machine, so monotonic Instant timing is correct here.
    let start = Instant::now(); // dst-ok

    while writer.high_watermark() == 0 {
        if halt_signal.is_halted() {
            return Err(BootError::HaltedDuringBoot(
                halt_signal.reason().unwrap_or_else(|| {
                    HaltReason::BackendError("event store halted during boot".to_string())
                }),
            ));
        }

        let elapsed = start.elapsed();

        if elapsed >= timeout {
            return Err(BootError::RunStartedTimeout { timeout });
        }
        thread::sleep(Duration::from_millis(1));
    }

    Ok(())
}

fn encode_run_started(components: &RegisteredComponents) -> Bytes {
    // bincode keeps the payload compact and matches the manifest encoding the backend
    // already uses; replay's RunStarted decoder pairs with this representation.
    let bytes = bincode::serde::encode_to_vec(components, bincode::config::standard())
        .expect("RegisteredComponents serializes via serde, must not fail under standard config");
    Bytes::from(bytes)
}

fn run_ended_draft(ts_init: UnixNanos) -> EntryDraft {
    EntryDraft::without_indices(
        Headers::empty(),
        Topic::from(RUN_ENDED_TOPIC),
        Ustr::from(RUN_ENDED_PAYLOAD_TYPE),
        Bytes::new(),
        ts_init,
    )
}

/// Bus tap that forwards captured publish and send dispatches to the event store.
///
/// Built and registered by [`EventStoreLifecycle::open`]; cleared by
/// [`EventStoreLifecycle::seal`] and the wrapper's [`Drop`]. The tap reads `ts_init` from
/// the kernel's `AtomicTime` at capture time so non-Phase-A headers carry a
/// writer-receive timestamp.
struct EventStoreBusTap {
    adapter: Arc<BusCaptureAdapter>,
    marker_capture: Option<SharedMarkerCapture>,
    clock: &'static AtomicTime,
    // Latch for the one-time halted log: the per-message Halted arm stays silent to
    // avoid log spam, but the transition into dropping captures must leave a trace.
    halted_logged: AtomicBool,
}

impl Debug for EventStoreBusTap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EventStoreBusTap))
            .field("halted", &self.adapter.is_halted())
            .field("marker_capture_attached", &self.marker_capture.is_some())
            .finish_non_exhaustive()
    }
}

impl BusTap for EventStoreBusTap {
    fn on_publish(&self, topic: Topic, message: &dyn Any) {
        let ts_init = self.clock.get_time_ns();
        self.capture(topic, message, ts_init);
    }

    fn on_send(&self, endpoint: MStr<Endpoint>, message: &dyn Any) {
        let ts_init = self.clock.get_time_ns();
        // Reuse the endpoint string as the captured topic. The MStr markers differ but
        // the underlying interned string is the same; offline scans match either way.
        let topic = Topic::from(*endpoint);
        self.capture(topic, message, ts_init);
    }

    fn on_response(&self, _correlation_id: &UUID4, message: &dyn Any) {
        let ts_init = self.clock.get_time_ns();
        let topic = MessagingSwitchboard::data_response_topic();
        self.capture(topic, message, ts_init);
    }
}

impl EventStoreBusTap {
    fn capture(&self, topic: Topic, message: &dyn Any, ts_init: UnixNanos) {
        // The registry both gates capture (no encoder -> no entry) and supplies headers
        // for entries that do flow through. Looking the headers up here keeps the
        // adapter encoder-only and lets header propagation light up per-type as the
        // SPEC's workstream A lands fields on commands and events.
        let headers = self
            .adapter
            .registry()
            .headers_for_any(message)
            .unwrap_or_else(Headers::empty);
        // Submit failures fire the adapter halt callback before returning; HaltSignal
        // is the observation path. Halted means the signal already fired.
        match self.adapter.capture_any(topic, message, headers, ts_init) {
            Ok(captured) => {
                self.capture_marker(topic, message, ts_init, captured);
            }
            Err(CaptureError::Halted) => {
                if !self.halted_logged.swap(true, Ordering::AcqRel) {
                    log::error!(
                        "Event store capture is halted; state-affecting messages are no longer recorded for this run"
                    );
                }
            }
            Err(CaptureError::Submit(e)) => {
                log::error!("Event store capture submit failed on {topic}: {e}");
            }
            Err(CaptureError::Encode(e)) => {
                log::warn!("Event store encoder rejected message on {topic}: {e}");
            }
        }
    }

    fn capture_marker(&self, topic: Topic, message: &dyn Any, ts_init: UnixNanos, captured: bool) {
        let Some(marker_capture) = self.marker_capture.as_ref() else {
            return;
        };
        let mut marker_capture = marker_capture.borrow_mut();
        let Some(capture) = marker_capture.as_mut() else {
            return;
        };

        if captured {
            capture.on_entry_submitted(ts_init);
        } else {
            capture.observe_publish(topic, message, ts_init);
        }
        capture.maybe_safety_flush(ts_init);
    }
}

fn install_bus_tap(
    adapter: Arc<BusCaptureAdapter>,
    marker_capture: Option<SharedMarkerCapture>,
    clock: &'static AtomicTime,
) {
    let tap: Rc<dyn BusTap> = Rc::new(EventStoreBusTap {
        adapter,
        marker_capture,
        clock,
        halted_logged: AtomicBool::new(false),
    });
    msgbus::set_bus_tap(tap);
}

// Use fully qualified `EventStoreLifecycle::` to dispatch to the inherent methods;
// `Self::` would resolve back into this trait impl and recurse.
#[allow(clippy::use_self)]
impl KernelEventStoreTrait for EventStoreLifecycle {
    fn restore_parent_cache(
        &mut self,
        instance_id: UUID4,
        cache: &mut Cache,
    ) -> anyhow::Result<()> {
        EventStoreLifecycle::restore_parent_cache(self, instance_id, cache)
            .map(|_| ())
            .map_err(Into::into)
    }

    fn open(
        &mut self,
        instance_id: UUID4,
        components: &RegisteredComponents,
        environment: Environment,
    ) -> anyhow::Result<()> {
        EventStoreLifecycle::open(self, instance_id, components, environment).map_err(Into::into)
    }

    fn snapshot_anchorer(&self) -> Option<SnapshotAnchorer> {
        EventStoreLifecycle::snapshot_anchorer(self)
    }

    fn seal(&mut self, ts_init: UnixNanos) {
        EventStoreLifecycle::seal(self, ts_init);
    }

    fn run_id(&self) -> Option<&str> {
        EventStoreLifecycle::run_id(self)
    }

    fn parent_run_id(&self) -> Option<&str> {
        EventStoreLifecycle::parent_run_id(self)
    }

    fn is_event_store_replay_configured(&self) -> bool {
        EventStoreLifecycle::is_event_store_replay_configured(self)
    }

    fn is_halted(&self) -> bool {
        EventStoreLifecycle::is_halted(self)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(madsim)]
    use std::path::Path;
    use std::path::PathBuf;

    use indexmap::IndexMap;
    use nautilus_common::{
        clock::TestClock,
        messages::{
            data::{
                DataCommand, DataResponse, QuotesResponse, RequestCommand, RequestQuotes,
                SubscribeCommand, SubscribeQuotes,
            },
            execution::{SubmitOrder, TradingCommand},
        },
        timer::{TimeEvent, TimeEventCallback, TimeEventHandler},
    };
    use nautilus_core::time::get_atomic_clock_static;
    use nautilus_model::{
        data::stubs::{quote_ethusdt_binance, stub_deltas},
        enums::TimeInForce,
        events::{
            OrderEventAny, OrderFilled,
            order::spec::{OrderFilledSpec, OrderInitializedSpec},
        },
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, Venue,
            VenueOrderId,
        },
        types::{Currency, Money, Price, Quantity},
    };
    use nautilus_system::event_store::{DataMarkerClass, DataMarkerConfig, RunIdentity};
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        AppendEntry, DataClass, EncodedPayload, EventStoreEntry, IndexKind, MarkerBackend,
        MemoryBackend, RedbMarkerBackend, SnapshotAnchor,
        capture::builtins::PAYLOAD_TYPE_TIME_EVENT, compute_entry_hash,
    };

    const INSTANCE_ID: &str = "trader-001";

    fn make_config(base_dir: PathBuf) -> EventStoreConfig {
        EventStoreConfig {
            base_dir,
            identity: RunIdentity {
                binary_hash: "deadbeef".to_string(),
                schema_version: 1,
                crate_versions: "feedface".to_string(),
                feature_flags: Vec::new(),
                adapter_versions: IndexMap::new(),
                config_hash: "cafebabe".to_string(),
                seed: None,
            },
            retention: RetentionMode::Full,
            replay_from_run_id: None,
            data_markers: None,
            channel_capacity: 64,
            max_batch_entries: 1,
            max_batch_latency: Duration::from_millis(2),
            halt_threshold: Duration::from_secs(2),
            run_started_timeout: Duration::from_secs(2),
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum CrashPoint {
        BeforeEnqueue,
        AfterEnqueueBeforeCommit,
        AfterCommitBeforeSnapshot,
        AfterSnapshot,
    }

    fn append_entry(seq: u64, topic: &str, payload_type: &str, payload: Bytes) -> AppendEntry {
        let ts = UnixNanos::from(seq);
        let headers = Headers::empty();
        let hash = compute_entry_hash(seq, ts, ts, topic, payload_type, &payload, &headers);
        let entry = EventStoreEntry::new(
            hash,
            seq,
            headers,
            Topic::from(topic),
            Ustr::from(payload_type),
            payload,
            ts,
            ts,
        );
        AppendEntry::without_indices(entry)
    }

    fn make_submit_order(client_order_id: ClientOrderId) -> SubmitOrder {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let order_init = OrderInitializedSpec::builder()
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .quantity(Quantity::from("1"))
            .time_in_force(TimeInForce::Gtc)
            .ts_event(UnixNanos::from(1))
            .ts_init(UnixNanos::from(2))
            .build();
        SubmitOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("BINANCE")),
            StrategyId::from("S-001"),
            instrument_id,
            client_order_id,
            order_init,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(3),
            None, // correlation_id
        )
    }

    fn append_run_started(seq: u64) -> AppendEntry {
        append_entry(
            seq,
            RUN_STARTED_TOPIC,
            RUN_STARTED_PAYLOAD_TYPE,
            encode_run_started(&RegisteredComponents::default()),
        )
    }

    #[derive(Debug)]
    struct TestAuditMessage {
        value: u8,
    }

    fn test_registry() -> EncoderRegistry {
        let mut registry = EncoderRegistry::new();
        registry.register::<TestAuditMessage, _>(Ustr::from("TestAuditMessage"), |message| {
            Ok(EncodedPayload::without_indices(Bytes::copy_from_slice(&[
                message.value,
            ])))
        });
        registry
    }

    fn wait_for_high_watermark(store: &EventStoreLifecycle, expected: u64) {
        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            let hwm = store
                .session
                .as_ref()
                .map_or(0, EventStoreSession::high_watermark);

            if hwm >= expected {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "event store high_watermark did not reach {expected} within deadline (hwm={hwm})",
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[derive(Debug, Clone)]
    struct SharedMemoryBackend(Arc<Mutex<MemoryBackend>>);

    impl EventStore for SharedMemoryBackend {
        fn open_run(&mut self, manifest: RunManifest) -> Result<(), EventStoreError> {
            self.0.lock().expect("memory backend").open_run(manifest)
        }

        fn append_batch(&mut self, entries: &[AppendEntry]) -> Result<u64, EventStoreError> {
            self.0.lock().expect("memory backend").append_batch(entries)
        }

        fn scan_range(
            &self,
            from: u64,
            to: u64,
            direction: ScanDirection,
        ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
            self.0
                .lock()
                .expect("memory backend")
                .scan_range(from, to, direction)
        }

        fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
            self.0.lock().expect("memory backend").scan_seq(seq)
        }

        fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
            self.0.lock().expect("memory backend").lookup(kind, key)
        }

        fn iter_index_keys(&self, kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
            self.0.lock().expect("memory backend").iter_index_keys(kind)
        }

        fn record_snapshot_anchor(
            &mut self,
            anchor: SnapshotAnchor,
        ) -> Result<(), EventStoreError> {
            self.0
                .lock()
                .expect("memory backend")
                .record_snapshot_anchor(anchor)
        }

        fn latest_snapshot_anchor(&self) -> Result<Option<SnapshotAnchor>, EventStoreError> {
            self.0
                .lock()
                .expect("memory backend")
                .latest_snapshot_anchor()
        }

        fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
            self.0.lock().expect("memory backend").seal(status)
        }

        fn manifest(&self) -> Result<RunManifest, EventStoreError> {
            self.0.lock().expect("memory backend").manifest()
        }

        fn high_watermark(&self) -> Result<u64, EventStoreError> {
            self.0.lock().expect("memory backend").high_watermark()
        }
    }

    fn seed_crashed_predecessor(config: &EventStoreConfig, run_id: &str, crash_point: CrashPoint) {
        let mut backend = RedbBackend::new(config.base_dir.clone());
        backend
            .open_run(build_manifest(
                config,
                INSTANCE_ID,
                run_id.to_string(),
                None,
                UnixNanos::from(1_000),
                RegisteredComponents::default(),
            ))
            .expect("open predecessor");

        match crash_point {
            // An entry sitting only in the writer channel leaves no durable redb
            // footprint after process death, so these two fault points intentionally
            // recover from the same on-disk state.
            CrashPoint::BeforeEnqueue | CrashPoint::AfterEnqueueBeforeCommit => {}
            CrashPoint::AfterCommitBeforeSnapshot => {
                backend
                    .append_batch(&[append_run_started(1)])
                    .expect("append committed entry");
            }
            CrashPoint::AfterSnapshot => {
                backend
                    .append_batch(&[append_run_started(1)])
                    .expect("append committed entry");
                backend
                    .record_snapshot_anchor(SnapshotAnchor::new(
                        1,
                        "cache://snapshot/run-crash/1",
                        "blake3:abc",
                    ))
                    .expect("record snapshot anchor");
            }
        }
    }

    #[rstest]
    fn halt_signal_callback_records_first_reason() {
        let signal = HaltSignal::new();
        let cb = signal.callback();
        cb(HaltReason::BackendDisk("ENOSPC".to_string()));
        cb(HaltReason::BackendError("second".to_string()));

        assert!(signal.is_halted());
        match signal.reason() {
            Some(HaltReason::BackendDisk(msg)) => assert!(msg.contains("ENOSPC")),
            other => panic!("expected first reason BackendDisk, was {other:?}"),
        }
    }

    #[rstest]
    fn recover_predecessors_returns_empty_for_missing_directory() {
        let tmp = TempDir::new().expect("tempdir");
        let outcome =
            recover_predecessors(tmp.path(), INSTANCE_ID).expect("recover empty directory");
        assert!(outcome.recovered.is_empty());
        assert!(outcome.parent_run_id.is_none());
    }

    #[rstest]
    fn restore_cache_snapshot_blob_rejects_hash_mismatch() {
        let mut cache = Cache::default();
        let blob = Bytes::from_static(b"snapshot");
        let anchor =
            crate::SnapshotAnchor::new(0, "cache://position-snapshots/P-1/0", "blake3:bad");

        cache
            .add(&anchor.blob_ref, blob)
            .expect("seed snapshot blob");
        let err =
            crate::restore_cache_snapshot_blob(&mut cache, Some(&anchor)).expect_err("hash error");

        assert!(
            err.to_string().contains("content_hash mismatch"),
            "err was: {err}",
        );
    }

    #[rstest]
    fn open_run_writes_run_started_and_advances_watermark() {
        let tmp = TempDir::new().expect("tempdir");
        let config = make_config(tmp.path().to_path_buf());
        let outcome = recover_predecessors(&config.base_dir, INSTANCE_ID).expect("recover empty");
        assert!(outcome.parent_run_id.is_none());

        let halt = HaltSignal::new();
        let session = open_run(
            &config,
            INSTANCE_ID,
            build_run_id(UnixNanos::from(1_000)),
            outcome.parent_run_id,
            UnixNanos::from(1_000),
            &RegisteredComponents::default(),
            halt,
            get_atomic_clock_static(),
        )
        .expect("open run");

        // Watermark + run-status snapshot.
        assert_eq!(session.high_watermark(), 1);
        assert_eq!(session.parent_run_id(), None);

        // Every identity field must thread from EventStoreConfig into the manifest.
        // A field-swap mutation in build_manifest (e.g. assigning binary_hash from
        // config.identity.config_hash) would fail one of these assertions.
        let manifest = session.manifest();
        assert_eq!(manifest.instance_id, INSTANCE_ID);
        assert_eq!(manifest.status, RunStatus::Running);
        assert_eq!(manifest.binary_hash, "deadbeef");
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.crate_versions, "feedface");
        assert_eq!(manifest.config_hash, "cafebabe");
        assert_eq!(manifest.start_ts_init, UnixNanos::from(1_000));
        assert_eq!(manifest.end_ts_init, None);
        assert!(
            manifest
                .feature_flags
                .contains(&"retention=full".to_string()),
            "feature_flags must record the retention mode, was {:?}",
            manifest.feature_flags,
        );
    }

    #[rstest]
    fn lifecycle_options_default_registry_keeps_builtin_encoders() {
        let registry = EventStoreLifecycleOptions::default().build_registry();

        assert!(registry.contains::<SubmitOrder>());
        assert!(registry.contains::<TradingCommand>());
        assert!(!registry.contains::<TestAuditMessage>());
    }

    #[rstest]
    fn lifecycle_options_custom_registry_captures_registered_message() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();
        let options = EventStoreLifecycleOptions::new().with_encoder_registry(test_registry());

        let mut store = EventStoreLifecycle::boot_with_options(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
            options,
        )
        .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();

        let topic: MStr<msgbus::Topic> = MStr::from("events.test.audit");
        msgbus::publish_any(topic, &TestAuditMessage { value: 42 });
        wait_for_high_watermark(&store, 2);

        drop(store);

        let sealed = RedbBackend::open_sealed(tmp.path(), &instance_id.to_string(), &run_id)
            .expect("open sealed");
        let captured = sealed
            .scan_seq(2)
            .expect("scan")
            .expect("captured entry present");

        assert_eq!(captured.payload_type.as_str(), "TestAuditMessage");
        assert_eq!(captured.topic.as_ref(), topic.as_str());
        assert_eq!(captured.payload.as_ref(), &[42]);
    }

    #[rstest]
    fn lifecycle_options_memory_backend_opener_captures_and_seals() {
        let tmp = TempDir::new().expect("tempdir");
        let memory = Arc::new(Mutex::new(MemoryBackend::new()));
        let opener_memory = Arc::clone(&memory);
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();
        let options = EventStoreLifecycleOptions::new()
            .with_encoder_registry(test_registry())
            .with_backend_opener(move |_, manifest| {
                opener_memory
                    .lock()
                    .expect("memory backend")
                    .open_run(manifest.clone())?;
                Ok(Box::new(SharedMemoryBackend(Arc::clone(&opener_memory))))
            });

        let mut store = EventStoreLifecycle::boot_with_options(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
            options,
        )
        .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");

        let topic: MStr<msgbus::Topic> = MStr::from("events.test.memory");
        msgbus::publish_any(topic, &TestAuditMessage { value: 7 });
        wait_for_high_watermark(&store, 2);

        store.seal(UnixNanos::from(1_000));

        let backend = memory.lock().expect("memory backend");
        let manifest = backend.manifest().expect("manifest");
        let captured = backend
            .scan_seq(2)
            .expect("scan")
            .expect("captured entry present");

        assert_eq!(manifest.instance_id, instance_id.to_string());
        assert_eq!(manifest.status, RunStatus::Ended);
        assert_eq!(manifest.high_watermark, 3);
        assert_eq!(captured.payload_type.as_str(), "TestAuditMessage");
        assert_eq!(captured.topic.as_ref(), topic.as_str());
        assert_eq!(captured.payload.as_ref(), &[7]);
    }

    #[cfg(madsim)]
    #[rstest]
    fn lifecycle_options_memory_backend_opener_captures_deterministic_seq_order_under_madsim() {
        let first = capture_madsim_memory_lifecycle_summary(42);
        let second = capture_madsim_memory_lifecycle_summary(42);
        let expected = expected_madsim_memory_entries();

        assert_eq!(first.entries, second.entries);
        assert_eq!(first.entries, expected);
        assert_eq!(
            first
                .entries
                .iter()
                .map(|entry| entry.seq)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4],
        );
        assert!(
            first.redb_files.is_empty(),
            "memory opener must not create redb files, was {:?}",
            first.redb_files,
        );
        assert!(
            second.redb_files.is_empty(),
            "memory opener must not create redb files, was {:?}",
            second.redb_files,
        );
    }

    #[cfg(madsim)]
    fn expected_madsim_memory_entries() -> Vec<CapturedEntrySummary> {
        vec![
            CapturedEntrySummary {
                seq: 1,
                topic: RUN_STARTED_TOPIC.to_string(),
                payload_type: RUN_STARTED_PAYLOAD_TYPE.to_string(),
                payload: encode_run_started(&RegisteredComponents::default()).to_vec(),
                ts_init: UnixNanos::from(0),
                ts_publish: UnixNanos::from(10_000),
            },
            CapturedEntrySummary {
                seq: 2,
                topic: "events.test.madsim".to_string(),
                payload_type: "TestAuditMessage".to_string(),
                payload: vec![1],
                ts_init: UnixNanos::from(20_000),
                ts_publish: UnixNanos::from(20_000),
            },
            CapturedEntrySummary {
                seq: 3,
                topic: "events.test.madsim".to_string(),
                payload_type: "TestAuditMessage".to_string(),
                payload: vec![2],
                ts_init: UnixNanos::from(30_000),
                ts_publish: UnixNanos::from(30_000),
            },
            CapturedEntrySummary {
                seq: 4,
                topic: RUN_ENDED_TOPIC.to_string(),
                payload_type: RUN_ENDED_PAYLOAD_TYPE.to_string(),
                payload: Vec::new(),
                ts_init: UnixNanos::from(40_000),
                ts_publish: UnixNanos::from(40_000),
            },
        ]
    }

    #[rstest]
    fn open_run_with_options_surfaces_backend_opener_error() {
        let tmp = TempDir::new().expect("tempdir");
        let config = make_config(tmp.path().to_path_buf());
        let options = EventStoreLifecycleOptions::new().with_backend_opener(|_, _| {
            Err(EventStoreError::Backend(
                "test backend open failed".to_string(),
            ))
        });

        let err = open_run_with_options(
            &config,
            INSTANCE_ID,
            "run-open-error".to_string(),
            None,
            UnixNanos::from(5_000),
            &RegisteredComponents::default(),
            HaltSignal::new(),
            get_atomic_clock_static(),
            &options,
        )
        .expect_err("backend opener error must stop run open");

        match err {
            BootError::EventStore(EventStoreError::Backend(msg)) => {
                assert!(msg.contains("test backend open failed"));
            }
            other => panic!("expected backend open failure, was {other:?}"),
        }
    }

    #[cfg(madsim)]
    #[derive(Debug, PartialEq, Eq)]
    struct MadsimMemoryLifecycleCapture {
        entries: Vec<CapturedEntrySummary>,
        redb_files: Vec<PathBuf>,
    }

    #[cfg(madsim)]
    #[derive(Debug, PartialEq, Eq)]
    struct CapturedEntrySummary {
        seq: u64,
        topic: String,
        payload_type: String,
        payload: Vec<u8>,
        ts_init: UnixNanos,
        ts_publish: UnixNanos,
    }

    #[cfg(madsim)]
    fn capture_madsim_memory_lifecycle_summary(seed: u64) -> MadsimMemoryLifecycleCapture {
        get_atomic_clock_static().set_time(UnixNanos::from(10_000));

        let tmp = TempDir::new().expect("tempdir");
        let memory = Arc::new(Mutex::new(MemoryBackend::new()));
        let opener_memory = Arc::clone(&memory);
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();
        let mut config = make_config(tmp.path().to_path_buf());
        config.identity.seed = Some(seed);
        let options = EventStoreLifecycleOptions::new()
            .with_encoder_registry(test_registry())
            .with_backend_opener(move |_, manifest| {
                opener_memory
                    .lock()
                    .expect("memory backend")
                    .open_run(manifest.clone())?;
                Ok(Box::new(SharedMemoryBackend(Arc::clone(&opener_memory))))
            });

        let mut store =
            EventStoreLifecycle::boot_with_options(Some(config), instance_id, clock_rc, options)
                .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");

        let topic: MStr<msgbus::Topic> = MStr::from("events.test.madsim");
        get_atomic_clock_static().set_time(UnixNanos::from(20_000));
        msgbus::publish_any(topic, &TestAuditMessage { value: 1 });
        get_atomic_clock_static().set_time(UnixNanos::from(30_000));
        msgbus::publish_any(topic, &TestAuditMessage { value: 2 });
        assert_eq!(
            store
                .session
                .as_ref()
                .expect("open session")
                .high_watermark(),
            3
        );

        get_atomic_clock_static().set_time(UnixNanos::from(40_000));
        store.seal(UnixNanos::from(40_000));

        let backend = memory.lock().expect("memory backend");
        let manifest = backend.manifest().expect("manifest");
        assert_eq!(manifest.seed, Some(seed));
        assert_eq!(manifest.status, RunStatus::Ended);
        assert_eq!(manifest.high_watermark, 4);
        let entries = backend
            .scan_range(1, manifest.high_watermark, ScanDirection::Forward)
            .expect("scan entries")
            .into_iter()
            .map(|entry| CapturedEntrySummary {
                seq: entry.seq,
                topic: entry.topic.as_ref().to_string(),
                payload_type: entry.payload_type.as_str().to_string(),
                payload: entry.payload.to_vec(),
                ts_init: entry.ts_init,
                ts_publish: entry.ts_publish,
            })
            .collect();
        drop(backend);

        MadsimMemoryLifecycleCapture {
            entries,
            redb_files: redb_files_under(tmp.path()),
        }
    }

    #[cfg(madsim)]
    fn redb_files_under(dir: &Path) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        collect_redb_files(dir, &mut paths);
        paths.sort();
        paths
    }

    #[cfg(madsim)]
    fn collect_redb_files(dir: &Path, paths: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                collect_redb_files(&path, paths);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "redb")
            {
                paths.push(path);
            }
        }
    }

    #[rstest]
    fn close_seals_manifest_and_records_run_ended() {
        let tmp = TempDir::new().expect("tempdir");
        let config = make_config(tmp.path().to_path_buf());

        let halt = HaltSignal::new();
        let mut session = open_run(
            &config,
            INSTANCE_ID,
            build_run_id(UnixNanos::from(2_000)),
            None,
            UnixNanos::from(2_000),
            &RegisteredComponents::default(),
            halt,
            get_atomic_clock_static(),
        )
        .expect("open run");

        let run_id = session.run_id().to_string();
        session.close(UnixNanos::from(3_000)).expect("close");

        let manifests = RedbBackend::list_runs(&config.base_dir, INSTANCE_ID).expect("list");
        let manifest = manifests
            .into_iter()
            .find(|m| m.run_id == run_id)
            .expect("manifest present");
        assert_eq!(manifest.status, RunStatus::Ended);
        assert!(manifest.high_watermark >= 2);
    }

    #[rstest]
    fn snapshot_anchorer_persists_anchor_for_open_session() {
        let tmp = TempDir::new().expect("tempdir");
        let config = make_config(tmp.path().to_path_buf());

        let halt = HaltSignal::new();
        let mut session = open_run(
            &config,
            INSTANCE_ID,
            build_run_id(UnixNanos::from(4_000)),
            None,
            UnixNanos::from(4_000),
            &RegisteredComponents::default(),
            halt,
            get_atomic_clock_static(),
        )
        .expect("open run");

        let run_id = session.run_id().to_string();

        {
            let anchorer = session.snapshot_anchorer().expect("snapshot anchorer");
            anchorer(CacheSnapshotRef::new(
                "cache://position-snapshots/P-1/0",
                Bytes::from_static(b"snapshot"),
            ))
            .expect("record snapshot anchor");
        }

        session.close(UnixNanos::from(4_500)).expect("close");

        let reader =
            RedbBackend::open_sealed(&config.base_dir, INSTANCE_ID, &run_id).expect("open sealed");
        let anchor = reader
            .latest_snapshot_anchor()
            .expect("latest snapshot anchor")
            .expect("anchor present");

        assert_eq!(anchor.high_watermark, 1);
        assert_eq!(anchor.blob_ref, "cache://position-snapshots/P-1/0");
        assert_eq!(
            anchor.content_hash,
            compute_snapshot_content_hash(b"snapshot"),
        );
    }

    #[rstest]
    fn recovery_seals_tail_ending_in_run_ended_as_ended_not_crashed() {
        // The writer commits RunEnded before sealing the manifest. A crash between
        // those two steps leaves the manifest Running while the tail already proves
        // graceful close: recovery must seal as Ended (not CrashedRecovered) and
        // must not chain the next run to it as a crashed parent.
        //
        // Reproduce the in-between state by submitting a RunEnded draft through the
        // writer's normal append path and then dropping the session without going
        // through close() (which is what would have sealed the manifest).
        let tmp = TempDir::new().expect("tempdir");
        let config = make_config(tmp.path().to_path_buf());

        let halt = HaltSignal::new();
        let run_id = build_run_id(UnixNanos::from(7_000));
        let session = open_run(
            &config,
            INSTANCE_ID,
            run_id.clone(),
            None,
            UnixNanos::from(7_000),
            &RegisteredComponents::default(),
            halt,
            get_atomic_clock_static(),
        )
        .expect("open run");

        let writer = session.writer.as_ref().expect("writer attached");
        writer
            .submit(run_ended_draft(UnixNanos::from(7_500)))
            .expect("submit RunEnded as tail entry");
        // Wait until the writer durably commits the RunEnded entry before dropping;
        // otherwise the on-disk tail might not include it and the recovery test
        // would fall back to CrashedRecovered for an unrelated reason.
        let deadline = Instant::now() + Duration::from_secs(2);

        while session.high_watermark() < 2 {
            assert!(
                Instant::now() < deadline,
                "writer high_watermark stuck at {} before deadline",
                session.high_watermark(),
            );
            thread::sleep(Duration::from_millis(2));
        }
        drop(session);

        let outcome = recover_predecessors(&config.base_dir, INSTANCE_ID).expect("recover sweep");
        assert_eq!(outcome.recovered.len(), 1);
        assert_eq!(outcome.recovered[0].run_id, run_id);
        assert_eq!(
            outcome.recovered[0].status,
            RunStatus::Ended,
            "tail ending in RunEnded must seal as Ended",
        );
        assert!(
            outcome.parent_run_id.is_none(),
            "Ended runs must not become parents",
        );

        let manifests = RedbBackend::list_runs(&config.base_dir, INSTANCE_ID).expect("list");
        let manifest = manifests
            .into_iter()
            .find(|m| m.run_id == run_id)
            .expect("manifest present");
        assert_eq!(manifest.status, RunStatus::Ended);
    }

    #[rstest]
    fn drop_without_close_leaves_run_for_next_boot_to_recover() {
        let tmp = TempDir::new().expect("tempdir");
        let config = make_config(tmp.path().to_path_buf());

        let halt = HaltSignal::new();
        let session = open_run(
            &config,
            INSTANCE_ID,
            build_run_id(UnixNanos::from(4_000)),
            None,
            UnixNanos::from(4_000),
            &RegisteredComponents::default(),
            halt,
            get_atomic_clock_static(),
        )
        .expect("open run");
        let run_id = session.run_id().to_string();
        drop(session);

        let outcome =
            recover_predecessors(&config.base_dir, INSTANCE_ID).expect("recover after crash");
        assert_eq!(outcome.recovered.len(), 1);
        assert_eq!(outcome.recovered[0].run_id, run_id);
        assert_eq!(outcome.recovered[0].status, RunStatus::CrashedRecovered);
        assert_eq!(outcome.parent_run_id.as_deref(), Some(run_id.as_str()));

        let manifests = RedbBackend::list_runs(&config.base_dir, INSTANCE_ID).expect("list");
        let sealed = manifests
            .into_iter()
            .find(|m| m.run_id == run_id)
            .expect("present");
        assert_eq!(sealed.status, RunStatus::CrashedRecovered);
    }

    #[rstest]
    fn build_run_id_uses_expected_format() {
        // Format: "<start_ts_init>-<8 hex chars>". The prefix is sortable by start
        // time so directory listings produce chronological order; the suffix
        // disambiguates concurrent starts at the same nanosecond.
        let id = build_run_id(UnixNanos::from(123_456));
        let (prefix, suffix) = id.split_once('-').expect("run id must contain a hyphen");
        assert_eq!(prefix, "123456");
        assert_eq!(suffix.len(), 8, "suffix was {suffix:?}");
        assert!(
            suffix.chars().all(|c| c.is_ascii_hexdigit()),
            "suffix must be hex, was {suffix:?}",
        );
    }

    #[rstest]
    fn crash_recovery_seals_predecessor_and_links_parent_run_id() {
        // SPEC Phase 7 acceptance: kill mid-run, restart, assert the predecessor seals
        // as CrashedRecovered, the new run's parent_run_id points to it, and the new
        // run's first entry is a RunStarted at seq=1.
        let tmp = TempDir::new().expect("tempdir");
        let config = make_config(tmp.path().to_path_buf());

        // Kernel boot 1: open a run and crash (drop the session without close).
        let halt_first = HaltSignal::new();
        let first = open_run(
            &config,
            INSTANCE_ID,
            build_run_id(UnixNanos::from(10_000)),
            None,
            UnixNanos::from(10_000),
            &RegisteredComponents::default(),
            halt_first,
            get_atomic_clock_static(),
        )
        .expect("open first run");
        let crashed_run_id = first.run_id().to_string();
        drop(first);

        // Kernel boot 2: recover predecessors then open the next run.
        let outcome = recover_predecessors(&config.base_dir, INSTANCE_ID).expect("recover sweep");
        assert_eq!(outcome.recovered.len(), 1);
        assert_eq!(outcome.recovered[0].run_id, crashed_run_id);
        assert_eq!(outcome.recovered[0].status, RunStatus::CrashedRecovered);
        assert_eq!(
            outcome.parent_run_id.as_deref(),
            Some(crashed_run_id.as_str())
        );

        // Predecessor's on-disk manifest is sealed CrashedRecovered.
        let manifests_after_seal =
            RedbBackend::list_runs(&config.base_dir, INSTANCE_ID).expect("list");
        let predecessor = manifests_after_seal
            .iter()
            .find(|m| m.run_id == crashed_run_id)
            .expect("predecessor present");
        assert_eq!(predecessor.status, RunStatus::CrashedRecovered);

        // New run is opened with parent_run_id pointing to the predecessor.
        let halt_second = HaltSignal::new();
        let new_run_id = build_run_id(UnixNanos::from(20_000));
        let next = open_run(
            &config,
            INSTANCE_ID,
            new_run_id.clone(),
            outcome.parent_run_id,
            UnixNanos::from(20_000),
            &RegisteredComponents::default(),
            halt_second,
            get_atomic_clock_static(),
        )
        .expect("open second run");
        assert_eq!(next.parent_run_id(), Some(crashed_run_id.as_str()));
        assert_eq!(
            next.manifest().parent_run_id.as_deref(),
            Some(crashed_run_id.as_str()),
        );
        assert_eq!(next.high_watermark(), 1, "RunStarted is the first entry");

        // The first entry in the new run is RunStarted at seq=1; close cleanly so we
        // can read the on-disk file without contending with the writer's lock.
        drop(next);
        let outcome_after = recover_predecessors(&config.base_dir, INSTANCE_ID)
            .expect("recover after second crash");
        // Only the second run shows up because the first is already sealed.
        assert_eq!(outcome_after.recovered.len(), 1);
        assert_eq!(outcome_after.recovered[0].run_id, new_run_id);
        assert_eq!(
            outcome_after.recovered[0].status,
            RunStatus::CrashedRecovered,
        );

        // Open the recovered run read-only and verify seq=1 is RunStarted.
        let sealed = RedbBackend::open_sealed(&config.base_dir, INSTANCE_ID, &new_run_id)
            .expect("open sealed");
        let first_entry = sealed
            .scan_seq(1)
            .expect("scan")
            .expect("RunStarted present");
        assert_eq!(first_entry.payload_type.as_str(), "RunStarted");
        assert_eq!(first_entry.topic.as_ref(), "run.lifecycle.RunStarted");
    }

    #[rstest]
    #[case::before_enqueue(CrashPoint::BeforeEnqueue, 0, false)]
    #[case::after_enqueue_before_commit(CrashPoint::AfterEnqueueBeforeCommit, 0, false)]
    #[case::after_commit_before_snapshot(CrashPoint::AfterCommitBeforeSnapshot, 1, false)]
    #[case::after_snapshot(CrashPoint::AfterSnapshot, 1, true)]
    fn crash_recovery_matrix_seals_predecessor_and_links_parent_run_id(
        #[case] crash_point: CrashPoint,
        #[case] expected_hwm: u64,
        #[case] expect_snapshot_anchor: bool,
    ) {
        let tmp = TempDir::new().expect("tempdir");
        let config = make_config(tmp.path().to_path_buf());
        let predecessor_run_id = format!("3000-{crash_point:?}");
        seed_crashed_predecessor(&config, &predecessor_run_id, crash_point);

        let outcome = recover_predecessors(&config.base_dir, INSTANCE_ID).expect("recover sweep");
        assert_eq!(outcome.recovered.len(), 1);
        assert_eq!(outcome.recovered[0].run_id, predecessor_run_id);
        assert_eq!(outcome.recovered[0].status, RunStatus::CrashedRecovered);
        assert_eq!(
            outcome.parent_run_id.as_deref(),
            Some(predecessor_run_id.as_str()),
        );

        let predecessor =
            RedbBackend::open_sealed(&config.base_dir, INSTANCE_ID, &predecessor_run_id)
                .expect("open sealed predecessor");
        let manifest = predecessor.manifest().expect("manifest");
        let snapshot_anchor = predecessor.latest_snapshot_anchor().expect("anchor read");

        assert_eq!(manifest.status, RunStatus::CrashedRecovered);
        assert_eq!(manifest.high_watermark, expected_hwm);
        assert_eq!(
            snapshot_anchor.is_some(),
            expect_snapshot_anchor,
            "snapshot anchor presence must match crash point",
        );

        let next = open_run(
            &config,
            INSTANCE_ID,
            "4000-next".to_string(),
            outcome.parent_run_id,
            UnixNanos::from(4_000),
            &RegisteredComponents::default(),
            HaltSignal::new(),
            get_atomic_clock_static(),
        )
        .expect("open next run");
        assert_eq!(next.parent_run_id(), Some(predecessor_run_id.as_str()));
        assert_eq!(
            next.manifest().parent_run_id.as_deref(),
            Some(predecessor_run_id.as_str()),
        );
    }

    #[rstest]
    fn kernel_event_store_open_seals_leftover_session_before_reopen() {
        // BacktestEngine::run -> reset -> run reuses the kernel. EventStoreLifecycle::open
        // must seal any leftover session before opening a fresh one so RunStarted is
        // the first entry of every run. The UUID suffix in build_run_id keeps the
        // two ids distinct even though TestClock holds start_ts_init at zero.
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = EventStoreLifecycle::boot(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
        )
        .expect("boot store");

        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open first run");
        let run_one = store.run_id().expect("run one open").to_string();

        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open second run");
        let run_two = store.run_id().expect("run two open").to_string();

        assert_ne!(run_one, run_two, "second open must produce a fresh run id");

        // Drop the wrapper so any open run seals via Drop, then read both manifests
        // off disk and assert each closed cleanly as Ended.
        drop(store);
        let manifests =
            RedbBackend::list_runs(tmp.path(), &instance_id.to_string()).expect("list runs");
        let m1 = manifests
            .iter()
            .find(|m| m.run_id == run_one)
            .expect("first run present");
        let m2 = manifests
            .iter()
            .find(|m| m.run_id == run_two)
            .expect("second run present");
        assert_eq!(m1.status, RunStatus::Ended);
        assert_eq!(m2.status, RunStatus::Ended);
    }

    #[rstest]
    fn open_after_halt_re_arms_signal_and_next_run_seals_ended() {
        // One halt must be terminal for the run that fired it, not for the kernel: a
        // rerun (reset -> run) opens with a fresh signal, reports no stale halt, and
        // its graceful stop still seals Ended.
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = EventStoreLifecycle::boot(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
        )
        .expect("boot store");

        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open first run");
        let run_one = store.run_id().expect("run one open").to_string();

        store.halt.callback()(HaltReason::BackendDisk("ENOSPC".to_string()));
        assert!(store.halt.is_halted());

        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open second run after halt");
        let run_two = store.run_id().expect("run two open").to_string();

        assert_ne!(run_one, run_two);
        assert!(!store.halt.is_halted(), "open must re-arm the halt signal");
        assert!(store.halt.reason().is_none());

        drop(store);
        let manifests =
            RedbBackend::list_runs(tmp.path(), &instance_id.to_string()).expect("list runs");
        let m1 = manifests
            .iter()
            .find(|m| m.run_id == run_one)
            .expect("first run present");
        let m2 = manifests
            .iter()
            .find(|m| m.run_id == run_two)
            .expect("second run present");
        // The halted run skips the in-process seal; the recovery sweep on next boot
        // owns it. The post-halt rerun must close cleanly.
        assert_eq!(m1.status, RunStatus::Running);
        assert_eq!(m2.status, RunStatus::Ended);
    }

    #[rstest]
    fn recover_picks_most_recent_crashed_recovered_as_parent() {
        // With multiple unsealed predecessors, the sweep must seal every one and the
        // new run's parent_run_id must point to the most recently started survivor.
        let tmp = TempDir::new().expect("tempdir");
        let config = make_config(tmp.path().to_path_buf());

        for ts in [1_000_u64, 2_000_u64, 3_000_u64] {
            let session = open_run(
                &config,
                INSTANCE_ID,
                build_run_id(UnixNanos::from(ts)),
                None,
                UnixNanos::from(ts),
                &RegisteredComponents::default(),
                HaltSignal::new(),
                get_atomic_clock_static(),
            )
            .expect("open");
            drop(session);
        }

        let outcome = recover_predecessors(&config.base_dir, INSTANCE_ID).expect("recover sweep");
        assert_eq!(outcome.recovered.len(), 3);
        assert!(
            outcome
                .recovered
                .iter()
                .all(|r| r.status == RunStatus::CrashedRecovered),
            "every predecessor must seal CrashedRecovered, was {:?}",
            outcome.recovered,
        );
        // Most-recent (start_ts_init=3_000) becomes the parent.
        let parent = outcome.parent_run_id.as_deref().expect("parent set");
        assert!(
            parent.starts_with("3000-"),
            "parent must be the run with the highest start_ts_init, was {parent}",
        );
    }

    #[rstest]
    fn submit_run_started_returns_timeout_when_writer_stalls() {
        // A backend whose append_batch never returns simulates a stuck writer. The
        // wait loop must surface BootError::RunStartedTimeout after the configured
        // ceiling elapses, never block indefinitely.
        let stub = StallBackend::default();
        let manifest = manifest_for("run-timeout");
        let mut backend: Box<dyn EventStore + Send> = Box::new(stub.clone());
        backend.open_run(manifest).expect("open stub");

        let halt = HaltSignal::new();

        let writer = EventStoreWriter::spawn(
            backend,
            get_atomic_clock_static(),
            halt.callback(),
            WriterConfig::default(),
        )
        .expect("spawn writer");

        let err = submit_run_started_blocking(
            &writer,
            &RegisteredComponents::default(),
            UnixNanos::from(100),
            &halt,
            Duration::from_millis(20),
        )
        .expect_err("must time out");

        match err {
            BootError::RunStartedTimeout { timeout } => {
                assert_eq!(timeout, Duration::from_millis(20));
            }
            other => panic!("expected RunStartedTimeout, was {other:?}"),
        }

        // Release the gate so the writer thread can exit cleanly before drop.
        stub.release();
    }

    #[rstest]
    fn submit_run_started_returns_halted_when_writer_halts_during_wait() {
        // A halt signal fired before the writer can commit must surface
        // BootError::HaltedDuringBoot with the recorded reason.
        let stub = StallBackend::default();
        let manifest = manifest_for("run-halt");
        let mut backend: Box<dyn EventStore + Send> = Box::new(stub.clone());
        backend.open_run(manifest).expect("open stub");

        let halt = HaltSignal::new();

        let writer = EventStoreWriter::spawn(
            backend,
            get_atomic_clock_static(),
            halt.callback(),
            WriterConfig::default(),
        )
        .expect("spawn writer");

        // Fire the halt from a peer thread shortly after we begin waiting.
        let halt_for_thread = halt.clone();

        let firer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            halt_for_thread.callback()(HaltReason::BackendDisk("test stall".to_string()));
        });

        let err = submit_run_started_blocking(
            &writer,
            &RegisteredComponents::default(),
            UnixNanos::from(200),
            &halt,
            Duration::from_secs(2),
        )
        .expect_err("must observe halt");

        match err {
            BootError::HaltedDuringBoot(HaltReason::BackendDisk(msg)) => {
                assert!(msg.contains("test stall"), "reason msg was {msg}");
            }
            other => panic!("expected HaltedDuringBoot(BackendDisk), was {other:?}"),
        }
        firer.join().expect("halt firer joined");
        stub.release();
    }

    fn manifest_for(run_id: &str) -> RunManifest {
        RunManifest {
            run_id: run_id.to_string(),
            parent_run_id: None,
            instance_id: INSTANCE_ID.to_string(),
            binary_hash: String::new(),
            schema_version: 1,
            crate_versions: String::new(),
            feature_flags: Vec::new(),
            adapter_versions: IndexMap::new(),
            config_hash: String::new(),
            registered_components: RegisteredComponents::default(),
            seed: None,
            start_ts_init: UnixNanos::default(),
            end_ts_init: None,
            high_watermark: 0,
            status: RunStatus::Running,
        }
    }

    /// Stub backend whose `append_batch` blocks until `release()` is called. Used to
    /// hold the writer's high-watermark at zero so the boot path's wait loop can
    /// exercise its timeout and halt branches deterministically.
    #[derive(Debug, Default, Clone)]
    struct StallBackend {
        inner: Arc<Mutex<StallInner>>,
        gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
    }

    #[derive(Debug, Default)]
    struct StallInner {
        manifest: Option<RunManifest>,
    }

    impl StallBackend {
        fn release(&self) {
            let (lock, cvar) = &*self.gate;
            *lock.lock().expect("gate") = true;
            cvar.notify_all();
        }
    }

    impl crate::EventStore for StallBackend {
        fn open_run(&mut self, manifest: RunManifest) -> Result<(), EventStoreError> {
            self.inner.lock().expect("inner").manifest = Some(manifest);
            Ok(())
        }

        fn append_batch(&mut self, _: &[crate::AppendEntry]) -> Result<u64, EventStoreError> {
            let (lock, cvar) = &*self.gate;
            let mut released = lock.lock().expect("gate");

            while !*released {
                released = cvar.wait(released).expect("gate wait");
            }
            Ok(0)
        }

        fn scan_range(
            &self,
            _: u64,
            _: u64,
            _: crate::ScanDirection,
        ) -> Result<Vec<crate::EventStoreEntry>, EventStoreError> {
            Ok(Vec::new())
        }

        fn scan_seq(&self, _: u64) -> Result<Option<crate::EventStoreEntry>, EventStoreError> {
            Ok(None)
        }

        fn lookup(&self, _: crate::IndexKind, _: &str) -> Result<Option<u64>, EventStoreError> {
            Ok(None)
        }

        fn iter_index_keys(
            &self,
            _: crate::IndexKind,
        ) -> Result<Vec<(String, u64)>, EventStoreError> {
            Ok(Vec::new())
        }

        fn seal(&mut self, _: RunStatus) -> Result<(), EventStoreError> {
            Ok(())
        }

        fn manifest(&self) -> Result<RunManifest, EventStoreError> {
            self.inner
                .lock()
                .expect("inner")
                .manifest
                .clone()
                .ok_or_else(|| EventStoreError::Backend("no manifest".to_string()))
        }

        fn high_watermark(&self) -> Result<u64, EventStoreError> {
            Ok(0)
        }
    }

    /// Integration: the kernel-installed bus tap forwards a `SubmitOrder` dispatched
    /// through the typed-send path into the event store before any subscriber observes
    /// it. The captured entry carries the dispatching endpoint as the topic and the
    /// canonical `SubmitOrder` payload type tag.
    #[rstest]
    fn bus_tap_captures_submit_order_sent_through_msgbus() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = EventStoreLifecycle::boot(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
        )
        .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();

        let trader_id = TraderId::from("TRADER-001");
        let strategy_id = StrategyId::from("S-001");
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let client_order_id = ClientOrderId::from("O-20260510-000001");
        let order_init = OrderInitializedSpec::builder()
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .quantity(Quantity::from("1"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        let submit_order = SubmitOrder::new(
            trader_id,
            Some(ClientId::from("BINANCE")),
            strategy_id,
            instrument_id,
            client_order_id,
            order_init,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(3),
            None, // correlation_id
        );

        let endpoint = MStr::<Endpoint>::from("test.exec.engine.process");
        msgbus::send_any_value(endpoint, &submit_order);

        // RunStarted is seq=1; the captured SubmitOrder lands at seq=2 once the
        // writer commits.
        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            let hwm = store
                .session
                .as_ref()
                .map_or(0, EventStoreSession::high_watermark);

            if hwm >= 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "captured SubmitOrder did not commit within deadline (hwm={hwm})",
            );
            thread::sleep(Duration::from_millis(2));
        }

        // Seal cleanly so we can re-open the run read-only
        drop(store);

        let sealed = RedbBackend::open_sealed(tmp.path(), &instance_id.to_string(), &run_id)
            .expect("open sealed");
        let captured = sealed
            .scan_seq(2)
            .expect("scan")
            .expect("captured entry present");
        assert_eq!(captured.payload_type.as_str(), "SubmitOrder");
        assert_eq!(captured.topic.as_ref(), endpoint.as_str());

        // The SubmitOrder encoder commits a ClientOrderId sidecar index; the lookup
        // must resolve to the captured seq.
        let by_client = sealed
            .lookup(IndexKind::ClientOrderId, client_order_id.as_str())
            .expect("lookup")
            .expect("indexed");
        assert_eq!(by_client, 2);
    }

    #[rstest]
    fn kernel_with_markers_captures_snapshots_over_synthetic_bus() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();
        let mut config = make_config(tmp.path().to_path_buf());
        config.data_markers = Some(DataMarkerConfig {
            classes: vec![DataMarkerClass::BookDeltas, DataMarkerClass::Quote],
            safety_flush_interval: Duration::from_secs(1),
            channel_capacity: 128,
            high_fidelity: Vec::new(),
        });

        let mut store =
            EventStoreLifecycle::boot(Some(config), instance_id, clock_rc).expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();

        let first = make_submit_order(ClientOrderId::from("O-marker-1"));
        msgbus::send_any_value(MStr::<Endpoint>::from("test.exec.process"), &first);

        let quote = quote_ethusdt_binance();
        msgbus::publish_quote(MStr::from("data.quotes.BINANCE.ETHUSDT-PERP"), &quote);
        let deltas = stub_deltas();
        msgbus::publish_deltas(MStr::from("data.deltas.XNAS.AAPL"), &deltas);

        let second = make_submit_order(ClientOrderId::from("O-marker-2"));
        msgbus::send_any_value(MStr::<Endpoint>::from("test.exec.process"), &second);
        wait_for_high_watermark(&store, 3);
        store.seal(UnixNanos::from(500));

        let marker_path = tmp
            .path()
            .join(instance_id.to_string())
            .join(format!("{run_id}.markers.redb"));
        let marker = RedbMarkerBackend::open_read_only_file(marker_path).expect("open markers");
        let snapshots = marker.scan_snapshots().expect("scan snapshots");
        let dict = marker.scan_dict().expect("scan dict");

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].event_seq_before, 3);
        assert_eq!(snapshots[0].advanced.len(), 2);
        assert_eq!(
            snapshots[0]
                .advanced
                .iter()
                .map(|cursor| cursor.count)
                .collect::<Vec<_>>(),
            vec![1, 1]
        );
        assert_eq!(
            dict.iter()
                .map(|entry| (entry.data_cls, entry.identifier.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (DataClass::Quote, "ETHUSDT-PERP.BINANCE"),
                (DataClass::BookDeltas, "AAPL.XNAS"),
            ],
        );
    }

    #[rstest]
    fn boot_recovery_ignores_marker_sidecar_files() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();
        let mut config = make_config(tmp.path().to_path_buf());
        config.data_markers = Some(DataMarkerConfig {
            classes: vec![DataMarkerClass::Quote],
            safety_flush_interval: Duration::from_secs(1),
            channel_capacity: 128,
            high_fidelity: Vec::new(),
        });

        let mut store =
            EventStoreLifecycle::boot(Some(config.clone()), instance_id, Rc::clone(&clock_rc))
                .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();
        store.seal(UnixNanos::from(500));

        let marker_path = tmp
            .path()
            .join(instance_id.to_string())
            .join(format!("{run_id}.markers.redb"));
        assert!(marker_path.exists());

        let rebooted = EventStoreLifecycle::boot(Some(config), instance_id, clock_rc)
            .expect("boot after marker sidecar");

        assert!(rebooted.recovered().is_empty());
    }

    #[rstest]
    fn marker_setup_failure_disables_markers_without_blocking_open() {
        let tmp = TempDir::new().expect("tempdir");
        let bad_base = tmp.path().join("not-a-directory");
        std::fs::write(&bad_base, b"not a directory").expect("write bad base");
        let memory = Arc::new(Mutex::new(MemoryBackend::new()));
        let opener_memory = Arc::clone(&memory);
        let mut config = make_config(bad_base);
        config.data_markers = Some(DataMarkerConfig {
            classes: vec![DataMarkerClass::Quote],
            safety_flush_interval: Duration::from_secs(1),
            channel_capacity: 128,
            high_fidelity: Vec::new(),
        });
        let options = EventStoreLifecycleOptions::new()
            .with_encoder_registry(test_registry())
            .with_backend_opener(move |_, manifest| {
                opener_memory
                    .lock()
                    .expect("memory backend")
                    .open_run(manifest.clone())?;
                Ok(Box::new(SharedMemoryBackend(Arc::clone(&opener_memory))))
            });

        let mut session = open_run_with_options(
            &config,
            INSTANCE_ID,
            "run-marker-setup-fails".to_string(),
            None,
            UnixNanos::from(5_000),
            &RegisteredComponents::default(),
            HaltSignal::new(),
            get_atomic_clock_static(),
            &options,
        )
        .expect("open run despite marker failure");

        assert!(session.marker_capture.is_none());

        let topic: MStr<msgbus::Topic> = MStr::from("events.test.marker-fallback");
        session
            .adapter()
            .expect("adapter")
            .capture::<TestAuditMessage>(
                topic,
                &TestAuditMessage { value: 11 },
                Headers::empty(),
                UnixNanos::from(5_001),
            )
            .expect("capture");
        let deadline = Instant::now() + Duration::from_secs(2);

        while session.high_watermark() < 2 {
            assert!(
                Instant::now() < deadline,
                "event-store high_watermark {} did not reach 2 within deadline",
                session.high_watermark(),
            );
            thread::sleep(Duration::from_millis(2));
        }
        session
            .close(UnixNanos::from(6_000))
            .expect("close session");

        let backend = memory.lock().expect("memory backend");
        let captured = backend
            .scan_seq(2)
            .expect("scan")
            .expect("captured entry present");

        assert_eq!(captured.payload_type.as_str(), "TestAuditMessage");
        assert_eq!(captured.topic.as_ref(), topic.as_str());
        assert_eq!(captured.payload.as_ref(), &[11]);
    }

    #[rstest]
    fn marker_registry_factory_receives_enabled_classes() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();
        let seen_classes: Arc<Mutex<Vec<Vec<DataClass>>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_for_factory = Arc::clone(&seen_classes);
        let mut config = make_config(tmp.path().to_path_buf());
        config.data_markers = Some(DataMarkerConfig {
            classes: vec![DataMarkerClass::Trade, DataMarkerClass::Quote],
            safety_flush_interval: Duration::from_secs(1),
            channel_capacity: 128,
            high_fidelity: Vec::new(),
        });
        let options =
            EventStoreLifecycleOptions::new().with_marker_registry_factory(move |classes| {
                seen_for_factory
                    .lock()
                    .expect("seen classes")
                    .push(classes.to_vec());
                DataMarkerExtractorRegistry::default_registry(classes)
            });

        let mut store =
            EventStoreLifecycle::boot_with_options(Some(config), instance_id, clock_rc, options)
                .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        store.seal(UnixNanos::from(1_000));

        let seen = seen_classes.lock().expect("seen classes");
        assert_eq!(seen.as_slice(), &[vec![DataClass::Trade, DataClass::Quote]]);
    }

    #[rstest]
    fn markers_disabled_installs_no_file_and_no_cost() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = EventStoreLifecycle::boot(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
        )
        .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();

        assert!(
            store
                .session
                .as_ref()
                .expect("session")
                .marker_capture
                .is_none()
        );

        let quote = quote_ethusdt_binance();
        msgbus::publish_quote(MStr::from("data.quotes.BINANCE.ETHUSDT-PERP"), &quote);
        store.seal(UnixNanos::from(500));

        let marker_path = tmp
            .path()
            .join(instance_id.to_string())
            .join(format!("{run_id}.markers.redb"));
        assert!(!marker_path.exists());
    }

    /// Fired clock events do not pass through normal message bus publish/send calls.
    /// `TimeEventHandler::run` must still hit the installed tap so timer-driven
    /// strategy logic has a durable trigger record.
    #[rstest]
    fn bus_tap_captures_time_event_handler_run() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = EventStoreLifecycle::boot(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
        )
        .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();

        let event = TimeEvent::new(
            Ustr::from("strategy.heartbeat"),
            UUID4::new(),
            UnixNanos::from(100),
            UnixNanos::from(99),
        );
        let callback = TimeEventCallback::from(|_: TimeEvent| {});
        TimeEventHandler::new(event, callback).run();

        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            let hwm = store
                .session
                .as_ref()
                .map_or(0, EventStoreSession::high_watermark);

            if hwm >= 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "captured TimeEvent did not commit within deadline (hwm={hwm})",
            );
            thread::sleep(Duration::from_millis(2));
        }

        drop(store);

        let sealed = RedbBackend::open_sealed(tmp.path(), &instance_id.to_string(), &run_id)
            .expect("open sealed");
        let captured = sealed
            .scan_seq(2)
            .expect("scan")
            .expect("captured entry present");

        assert_eq!(captured.payload_type.as_str(), PAYLOAD_TYPE_TIME_EVENT);
        assert_eq!(captured.topic, MessagingSwitchboard::time_event_topic());
    }

    /// `EventStoreLifecycle::seal` must clear the bus tap so a publish issued after the
    /// run closes cannot reach the sealed writer. Without the clear, the dropped
    /// adapter would still receive captures and `Arc::try_unwrap` inside close would
    /// fail with multiple owners.
    #[rstest]
    fn seal_clears_bus_tap_so_post_seal_dispatches_do_not_capture() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = EventStoreLifecycle::boot(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
        )
        .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();

        store.seal(UnixNanos::from(0));

        // Post-seal dispatch: any tap that survived would either capture into the
        // dropped writer (panic via the channel close path) or hold the adapter Arc
        // and fail the close try_unwrap. The session is already gone, so this just
        // exercises the cleared-tap path through msgbus dispatch.
        let endpoint = MStr::<Endpoint>::from("test.post.seal.endpoint");
        let payload: u32 = 99;
        msgbus::send_any_value(endpoint, &payload);

        let sealed = RedbBackend::open_sealed(tmp.path(), &instance_id.to_string(), &run_id)
            .expect("open sealed");
        // RunStarted at seq=1, RunEnded at seq=2; no captured u32 entry must exist
        assert!(
            sealed.scan_seq(3).expect("scan").is_none(),
            "no entry must land after seal",
        );
    }

    /// Production code reaches the bus tap with [`TradingCommand`] wrapped around the
    /// inner command (the wrapper's `TypeId`, not `SubmitOrder`'s). The envelope
    /// dispatcher in [`default_registry`] must unwrap the variant, stamp the inner
    /// `payload_type` (`SubmitOrder`), and commit the same indices the bare-type encoder
    /// would have produced.
    #[rstest]
    fn bus_tap_captures_trading_command_envelope_with_inner_payload_type() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = EventStoreLifecycle::boot(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
        )
        .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();

        let trader_id = TraderId::from("TRADER-001");
        let strategy_id = StrategyId::from("S-001");
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let client_order_id = ClientOrderId::from("O-20260510-000002");
        let order_init = OrderInitializedSpec::builder()
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .quantity(Quantity::from("1"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        let submit_order = SubmitOrder::new(
            trader_id,
            Some(ClientId::from("BINANCE")),
            strategy_id,
            instrument_id,
            client_order_id,
            order_init,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(3),
            None, // correlation_id
        );
        let command = TradingCommand::SubmitOrder(submit_order.clone());

        let endpoint = MStr::<Endpoint>::from("test.exec.engine.envelope");
        msgbus::send_trading_command(endpoint, command);

        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            let hwm = store
                .session
                .as_ref()
                .map_or(0, EventStoreSession::high_watermark);

            if hwm >= 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "captured TradingCommand did not commit within deadline (hwm={hwm})",
            );
            thread::sleep(Duration::from_millis(2));
        }

        drop(store);

        let sealed = RedbBackend::open_sealed(tmp.path(), &instance_id.to_string(), &run_id)
            .expect("open sealed");
        let captured = sealed
            .scan_seq(2)
            .expect("scan")
            .expect("captured entry present");
        assert_eq!(
            captured.payload_type.as_str(),
            "SubmitOrder",
            "wrapper envelope must stamp the inner payload_type",
        );
        assert_eq!(captured.topic.as_ref(), endpoint.as_str());

        let by_client = sealed
            .lookup(IndexKind::ClientOrderId, client_order_id.as_str())
            .expect("lookup")
            .expect("indexed");
        assert_eq!(by_client, 2);

        // Round-trip the captured payload back through the inner-type decoder so the
        // bytes-equal-bare invariant is checked at the integration layer too: a mutation
        // that wrote the wrapper-typed bytes instead of the inner would fail here.
        let decoded: SubmitOrder =
            rmp_serde::from_slice(&captured.payload).expect("decode captured SubmitOrder");
        assert_eq!(decoded, submit_order);
    }

    /// `publish_order_event` reaches the bus tap with `OrderEventAny::Filled(...)`; the
    /// envelope dispatcher must unwrap to `OrderFilled`, stamp `OrderFilled` as the
    /// `payload_type`, and commit both the `client_order_id` and `venue_order_id` indices.
    #[rstest]
    fn bus_tap_captures_order_event_any_envelope_with_inner_payload_type() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = EventStoreLifecycle::boot(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
        )
        .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();

        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let client_order_id = ClientOrderId::from("O-20260510-000003");
        let venue_order_id = VenueOrderId::from("V-99");
        let filled = OrderFilledSpec::builder()
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .venue_order_id(venue_order_id)
            .account_id(AccountId::from("BINANCE-001"))
            .trade_id(TradeId::from("T-1"))
            .last_qty(Quantity::from("1"))
            .last_px(Price::from("100.00"))
            .currency(Currency::USDT())
            .ts_event(UnixNanos::from(10))
            .ts_init(UnixNanos::from(11))
            .commission(Money::new(0.10, Currency::USDT()))
            .build();
        let event = OrderEventAny::Filled(filled);

        let topic: MStr<msgbus::Topic> = MStr::from("events.order.ETHUSDT-PERP.BINANCE");
        msgbus::publish_order_event(topic, &event);

        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            let hwm = store
                .session
                .as_ref()
                .map_or(0, EventStoreSession::high_watermark);

            if hwm >= 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "captured OrderEventAny did not commit within deadline (hwm={hwm})",
            );
            thread::sleep(Duration::from_millis(2));
        }

        drop(store);

        let sealed = RedbBackend::open_sealed(tmp.path(), &instance_id.to_string(), &run_id)
            .expect("open sealed");
        let captured = sealed
            .scan_seq(2)
            .expect("scan")
            .expect("captured entry present");
        assert_eq!(
            captured.payload_type.as_str(),
            "OrderFilled",
            "wrapper envelope must stamp the inner payload_type",
        );
        assert_eq!(captured.topic.as_ref(), topic.as_str());

        let by_client = sealed
            .lookup(IndexKind::ClientOrderId, client_order_id.as_str())
            .expect("lookup")
            .expect("indexed");
        let by_venue = sealed
            .lookup(IndexKind::VenueOrderId, venue_order_id.as_str())
            .expect("lookup")
            .expect("indexed");
        assert_eq!(by_client, 2);
        assert_eq!(by_venue, 2);

        // Round-trip the captured payload back through the inner-type decoder so a
        // mutation that wrote the wrapper-typed bytes instead of the inner would fail
        // here rather than only at the unit-level bytes-equal-bare check.
        let decoded: OrderFilled =
            rmp_serde::from_slice(&captured.payload).expect("decode captured OrderFilled");
        assert_eq!(decoded, filled);
    }

    /// `send_data_command` reaches the bus tap with the [`DataCommand`] wrapper. The
    /// envelope dispatcher must unwrap to the request/subscription category, stamp that
    /// category as the `payload_type`, and write bytes that decode as the inner command
    /// enum.
    #[rstest]
    fn bus_tap_captures_data_command_envelopes_with_category_payload_types() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = EventStoreLifecycle::boot(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
        )
        .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();

        let request = RequestCommand::Quotes(RequestQuotes::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            None,
            None,
            None,
            Some(ClientId::from("BINANCE")),
            UUID4::new(),
            UnixNanos::from(20),
            None,
        ));
        let subscribe = SubscribeCommand::Quotes(SubscribeQuotes::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            Some(ClientId::from("BINANCE")),
            Some(Venue::from("BINANCE")),
            UUID4::new(),
            UnixNanos::from(21),
            Some(UUID4::new()),
            None,
        ));

        let request_endpoint = MStr::<Endpoint>::from("test.data.engine.request");
        msgbus::send_data_command(request_endpoint, DataCommand::Request(request.clone()));

        let subscribe_endpoint = MStr::<Endpoint>::from("test.data.engine.subscribe");
        msgbus::send_data_command(
            subscribe_endpoint,
            DataCommand::Subscribe(subscribe.clone()),
        );

        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            let hwm = store
                .session
                .as_ref()
                .map_or(0, EventStoreSession::high_watermark);

            if hwm >= 3 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "captured DataCommand entries did not commit within deadline (hwm={hwm})",
            );
            thread::sleep(Duration::from_millis(2));
        }

        drop(store);

        let sealed = RedbBackend::open_sealed(tmp.path(), &instance_id.to_string(), &run_id)
            .expect("open sealed");
        let captured_request = sealed
            .scan_seq(2)
            .expect("scan request")
            .expect("captured request present");
        assert_eq!(captured_request.payload_type.as_str(), "RequestCommand");
        assert_eq!(captured_request.topic.as_ref(), request_endpoint.as_str());

        let decoded_request: RequestCommand =
            rmp_serde::from_slice(&captured_request.payload).expect("decode RequestCommand");
        match (decoded_request, request) {
            (RequestCommand::Quotes(decoded), RequestCommand::Quotes(expected)) => {
                assert_eq!(decoded.request_id, expected.request_id);
                assert_eq!(decoded.instrument_id, expected.instrument_id);
                assert_eq!(decoded.client_id, expected.client_id);
                assert_eq!(decoded.ts_init, expected.ts_init);
            }
            other => panic!("expected RequestCommand::Quotes round trip, was {other:?}"),
        }

        let captured_subscribe = sealed
            .scan_seq(3)
            .expect("scan subscribe")
            .expect("captured subscribe present");
        assert_eq!(captured_subscribe.payload_type.as_str(), "SubscribeCommand");
        assert_eq!(
            captured_subscribe.topic.as_ref(),
            subscribe_endpoint.as_str()
        );

        let decoded_subscribe: SubscribeCommand =
            rmp_serde::from_slice(&captured_subscribe.payload).expect("decode SubscribeCommand");
        match (decoded_subscribe, subscribe) {
            (SubscribeCommand::Quotes(decoded), SubscribeCommand::Quotes(expected)) => {
                assert_eq!(decoded.command_id, expected.command_id);
                assert_eq!(decoded.instrument_id, expected.instrument_id);
                assert_eq!(decoded.client_id, expected.client_id);
                assert_eq!(decoded.venue, expected.venue);
                assert_eq!(decoded.ts_init, expected.ts_init);
                assert_eq!(decoded.correlation_id, expected.correlation_id);
            }
            other => panic!("expected SubscribeCommand::Quotes round trip, was {other:?}"),
        }
    }

    // `send_response` dispatches through a correlation handler rather than an endpoint
    // or pub/sub topic. The bus tap must still capture the `DataResponse` envelope and
    // stamp the inner response category as the payload type.
    #[rstest]
    fn bus_tap_captures_data_response_sent_through_correlation_handler() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = EventStoreLifecycle::boot(
            Some(make_config(tmp.path().to_path_buf())),
            instance_id,
            clock_rc,
        )
        .expect("boot store");
        store
            .open(
                instance_id,
                &RegisteredComponents::default(),
                Environment::Backtest,
            )
            .expect("open run");
        let run_id = store.run_id().expect("run open").to_string();

        let correlation_id = UUID4::new();
        let handler_called = Rc::new(RefCell::new(false));
        let handler_called_clone = handler_called.clone();
        msgbus::register_response_handler(
            &correlation_id,
            msgbus::ShareableMessageHandler::from_typed(move |_resp: &QuotesResponse| {
                *handler_called_clone.borrow_mut() = true;
            }),
        );

        let response = QuotesResponse::new(
            correlation_id,
            ClientId::from("BINANCE"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            vec![],
            None,
            None,
            UnixNanos::from(30),
            None,
        );
        msgbus::send_response(&correlation_id, &DataResponse::Quotes(response.clone()));

        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            let hwm = store
                .session
                .as_ref()
                .map_or(0, EventStoreSession::high_watermark);

            if hwm >= 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "captured DataResponse did not commit within deadline (hwm={hwm})",
            );
            thread::sleep(Duration::from_millis(2));
        }

        assert!(*handler_called.borrow());
        drop(store);

        let sealed = RedbBackend::open_sealed(tmp.path(), &instance_id.to_string(), &run_id)
            .expect("open sealed");
        let captured = sealed
            .scan_seq(2)
            .expect("scan")
            .expect("captured response present");
        assert_eq!(captured.payload_type.as_str(), "QuotesResponse");
        assert_eq!(captured.topic, MessagingSwitchboard::data_response_topic());

        let decoded: QuotesResponse =
            rmp_serde::from_slice(&captured.payload).expect("decode QuotesResponse");
        assert_eq!(decoded.correlation_id, response.correlation_id);
        assert_eq!(decoded.client_id, response.client_id);
        assert_eq!(decoded.instrument_id, response.instrument_id);
        assert_eq!(decoded.ts_init, response.ts_init);
        assert!(decoded.data.is_empty());
    }
}
