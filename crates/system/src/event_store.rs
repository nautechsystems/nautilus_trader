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
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_common::{
    clock::Clock,
    enums::Environment,
    msgbus::{self, BusTap, Endpoint, MStr},
};
#[cfg(feature = "live")]
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_core::{
    UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_static},
};
use nautilus_event_store::{
    BusCaptureAdapter, CaptureError, EntryDraft, EventStore, EventStoreError, EventStoreWriter,
    HaltCallback, HaltReason, Headers, RedbBackend, RegisteredComponents, RunId, RunManifest,
    RunStatus, ScanDirection, Topic, WriterConfig, default_registry,
};
use ustr::Ustr;

const RUN_STARTED_TOPIC: &str = "run.lifecycle.RunStarted";
const RUN_STARTED_PAYLOAD_TYPE: &str = "RunStarted";
const RUN_ENDED_TOPIC: &str = "run.lifecycle.RunEnded";
const RUN_ENDED_PAYLOAD_TYPE: &str = "RunEnded";

/// How the supervisor (a future workstream) prunes sealed run files.
///
/// The kernel records the choice in the manifest's `feature_flags` and otherwise treats
/// every value identically: retention is implemented in Phase 12 and is out of scope for
/// the kernel boot path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RetentionMode {
    /// Keep every sealed run; never reclaim.
    #[default]
    Full,
    /// Keep at most `keep_last` sealed runs; the supervisor reclaims older files.
    Bounded {
        /// The number of sealed runs to retain.
        keep_last: usize,
    },
    /// Keep the manifest plus a snapshot anchor and the tail since the anchor; older
    /// entries reclaim once a newer anchor is durable.
    SnapshotAnchored,
}

/// Per-run identification data the kernel populates from build metadata.
///
/// Phase 7 records what is available at run start; cross-cutting workstreams refine
/// these values as they land. Defaults are placeholders so the kernel can boot before
/// the binary-hash and crate-versions wiring is finalized.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunIdentity {
    /// A hex-encoded hash of the trader binary.
    pub binary_hash: String,
    /// The entry payload schema version.
    pub schema_version: u32,
    /// A hex-encoded hash of `Cargo.lock` or an equivalent crate version manifest.
    pub crate_versions: String,
    /// The active Cargo features for the trader binary.
    pub feature_flags: Vec<String>,
    /// Per-adapter version stamp keyed by adapter name.
    pub adapter_versions: IndexMap<String, String>,
    /// A hex-encoded hash of the kernel configuration.
    pub config_hash: String,
    /// The deterministic seed, populated when the run executes under a seeded mode.
    pub seed: Option<u64>,
}

/// Configuration for the kernel-managed event store run lifecycle.
#[derive(Clone, Debug)]
pub struct EventStoreConfig {
    /// Root directory; the backend creates `<base_dir>/<instance_id>/<run_id>.redb`.
    pub base_dir: PathBuf,
    /// Stable identification for this trader instance and binary.
    pub identity: RunIdentity,
    /// How the supervisor reclaims sealed run files (out-of-scope in Phase 7).
    pub retention: RetentionMode,
    /// Capacity of the writer's bounded submit channel.
    pub channel_capacity: usize,
    /// Maximum entries collected before the writer forces a commit.
    pub max_batch_entries: usize,
    /// Maximum time a batch may accumulate before the writer forces a commit.
    pub max_batch_latency: Duration,
    /// Submit-side stall ceiling that triggers writer fail-stop.
    pub halt_threshold: Duration,
    /// Maximum time to wait for the `RunStarted` entry to durably commit before the
    /// kernel surfaces [`BootError::RunStartedTimeout`].
    pub run_started_timeout: Duration,
}

impl Default for EventStoreConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::new(),
            identity: RunIdentity::default(),
            retention: RetentionMode::default(),
            channel_capacity: nautilus_event_store::DEFAULT_CHANNEL_CAPACITY,
            max_batch_entries: nautilus_event_store::DEFAULT_MAX_BATCH_ENTRIES,
            max_batch_latency: nautilus_event_store::DEFAULT_MAX_BATCH_LATENCY,
            halt_threshold: nautilus_event_store::DEFAULT_HALT_THRESHOLD,
            run_started_timeout: Duration::from_secs(5),
        }
    }
}

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
    /// multiple submits race past the halt threshold) and flips the halted flag.
    #[must_use]
    pub fn callback(&self) -> HaltCallback {
        let halted = Arc::clone(&self.halted);
        let reason = Arc::clone(&self.reason);
        Arc::new(move |r| {
            if halted
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
                && let Ok(mut slot) = reason.lock()
            {
                *slot = Some(r);
            }
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

    /// Returns the parent run id (the most-recently-recovered predecessor).
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

        let Some(writer_arc) = self.writer.take() else {
            return Ok(());
        };
        let writer = Arc::try_unwrap(writer_arc).map_err(|_| {
            EventStoreError::Backend(
                "event store writer has multiple owners; clear the bus tap before close"
                    .to_string(),
            )
        })?;

        let run_ended = run_ended_draft(ts_init);
        writer.close(run_ended)?;
        Ok(())
    }
}

impl Drop for EventStoreSession {
    fn drop(&mut self) {
        // Drop without close: release adapter then writer so the writer thread exits
        // unsealed; the next boot recovers.
        self.adapter.take();
        self.writer.take();
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
    /// The writer signaled fail-stop after the kernel was already started.
    #[error("event store halted: {0:?}")]
    EventStoreHalted(HaltReason),
}

/// Kernel-facing wrapper that bundles every event-store concern: predecessor recovery,
/// the open run, the halt signal, and the seal-on-drop fail-safe.
///
/// One instance lives on [`crate::kernel::NautilusKernel`] behind the `event_store`
/// feature; the kernel calls [`KernelEventStore::open`] from `start()`,
/// [`KernelEventStore::seal`] from `finalize_stop()` / `dispose()`, and the wrapper's
/// [`Drop`] runs as the last-chance seal site for callers that skip both teardown paths
/// (e.g. imperative `engine.run(...)` followed by drop in `BacktestEngine`).
#[derive(Debug)]
pub struct KernelEventStore {
    config: Option<EventStoreConfig>,
    recovered: Vec<RecoveredRun>,
    parent_run_id: Option<String>,
    session: Option<EventStoreSession>,
    halt: HaltSignal,
    // Held so `Drop` can stamp the seal even when the kernel never called seal()
    // explicitly. Cloning the kernel's clock Rc keeps the wrapper independent of
    // its owner.
    clock: Rc<RefCell<dyn Clock>>,
}

impl KernelEventStore {
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

        let clock = Self::clock_for(environment);
        let start_ts_init = self.clock.borrow().timestamp_ns();
        let run_id = build_run_id(start_ts_init);
        let session = open_run(
            &config,
            &instance_id.to_string(),
            run_id,
            self.parent_run_id.clone(),
            start_ts_init,
            components,
            self.halt.clone(),
            clock,
        )?;
        log::info!(
            "Opened event-store run {} (parent_run_id={:?})",
            session.run_id(),
            session.parent_run_id(),
        );

        if let Some(adapter) = session.adapter() {
            install_bus_tap(Arc::clone(adapter), clock);
        }
        self.session = Some(session);
        Ok(())
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

    /// Returns the parent run id wired into the open run's manifest, when one was
    /// recovered.
    #[must_use]
    pub fn parent_run_id(&self) -> Option<&str> {
        self.parent_run_id.as_deref()
    }

    /// Returns the run id of the open session, when capture is active.
    #[must_use]
    pub fn run_id(&self) -> Option<&str> {
        self.session.as_ref().map(EventStoreSession::run_id)
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

impl Drop for KernelEventStore {
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
/// # Errors
///
/// Returns [`EventStoreError`] when the directory enumeration, open, or seal fails for
/// reasons other than the expected [`EventStoreError::CrashedPredecessor`] handshake
/// the backend uses to surface unsealed runs.
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
            Err(other) => return Err(other),
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
                Err(other) => return Err(other),
            }
        };

        backend.seal(final_status)?;
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
    let manifest = build_manifest(
        config,
        instance_id,
        run_id,
        parent_run_id,
        start_ts_init,
        components.clone(),
    );

    let mut backend = RedbBackend::new(config.base_dir.clone());
    backend.open_run(manifest.clone())?;

    let writer = Arc::new(EventStoreWriter::spawn(
        Box::new(backend),
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

    let adapter = Arc::new(BusCaptureAdapter::new(
        Arc::clone(&writer),
        Arc::new(default_registry()),
        halt_signal.callback(),
    ));

    Ok(EventStoreSession {
        writer: Some(writer),
        adapter: Some(adapter),
        manifest,
        halt_signal,
    })
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
/// Built and registered by [`KernelEventStore::open`]; cleared by
/// [`KernelEventStore::seal`] and the wrapper's [`Drop`]. The tap reads `ts_init` from
/// the kernel's `AtomicTime` at capture time so non-Phase-A headers carry a
/// writer-receive timestamp.
struct EventStoreBusTap {
    adapter: Arc<BusCaptureAdapter>,
    clock: &'static AtomicTime,
}

impl Debug for EventStoreBusTap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EventStoreBusTap))
            .field("halted", &self.adapter.is_halted())
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
        // the underlying interned string is the same; forensics scans match either way.
        let topic = Topic::from(*endpoint);
        self.capture(topic, message, ts_init);
    }
}

impl EventStoreBusTap {
    fn capture(&self, topic: Topic, message: &dyn Any, ts_init: UnixNanos) {
        match self
            .adapter
            .capture_any(topic, message, Headers::empty(), ts_init)
        {
            Ok(_) => {}
            // Submit failures fire the adapter's halt callback before returning; the
            // kernel's HaltSignal observes the failure through that path, so we only
            // log the surface here for forensic visibility.
            Err(CaptureError::Submit(e)) => {
                log::error!("Event store capture submit failed on {topic}: {e}");
            }
            Err(CaptureError::Halted) => {
                // Already signaled; suppress per-call noise
            }
            Err(CaptureError::Encode(e)) => {
                log::warn!("Event store encoder rejected message on {topic}: {e}");
            }
        }
    }
}

fn install_bus_tap(adapter: Arc<BusCaptureAdapter>, clock: &'static AtomicTime) {
    let tap: Rc<dyn BusTap> = Rc::new(EventStoreBusTap { adapter, clock });
    msgbus::set_bus_tap(tap);
}

#[cfg(test)]
mod tests {
    use nautilus_common::{clock::TestClock, messages::execution::SubmitOrder};
    use nautilus_core::time::get_atomic_clock_static;
    use nautilus_event_store::IndexKind;
    use nautilus_model::{
        enums::{OrderSide, OrderType, TimeInForce},
        events::OrderInitialized,
        identifiers::{ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId},
        types::Quantity,
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

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
            channel_capacity: 64,
            max_batch_entries: 1,
            max_batch_latency: Duration::from_millis(2),
            halt_threshold: Duration::from_secs(2),
            run_started_timeout: Duration::from_secs(2),
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
    fn kernel_event_store_open_seals_leftover_session_before_reopen() {
        // BacktestEngine::run -> reset -> run reuses the kernel. KernelEventStore::open
        // must seal any leftover session before opening a fresh one so RunStarted is
        // the first entry of every run. The UUID suffix in build_run_id keeps the
        // two ids distinct even though TestClock holds start_ts_init at zero.
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = KernelEventStore::boot(
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

    impl nautilus_event_store::EventStore for StallBackend {
        fn open_run(&mut self, manifest: RunManifest) -> Result<(), EventStoreError> {
            self.inner.lock().expect("inner").manifest = Some(manifest);
            Ok(())
        }

        fn append_batch(
            &mut self,
            _: &[nautilus_event_store::AppendEntry],
        ) -> Result<u64, EventStoreError> {
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
            _: nautilus_event_store::ScanDirection,
        ) -> Result<Vec<nautilus_event_store::EventStoreEntry>, EventStoreError> {
            Ok(Vec::new())
        }

        fn scan_seq(
            &self,
            _: u64,
        ) -> Result<Option<nautilus_event_store::EventStoreEntry>, EventStoreError> {
            Ok(None)
        }

        fn lookup(
            &self,
            _: nautilus_event_store::IndexKind,
            _: &str,
        ) -> Result<Option<u64>, EventStoreError> {
            Ok(None)
        }

        fn iter_index_keys(
            &self,
            _: nautilus_event_store::IndexKind,
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

        let mut store = KernelEventStore::boot(
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
        let order_init = OrderInitialized::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("1"),
            TimeInForce::Gtc,
            false,
            false,
            false,
            false,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
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

    /// `KernelEventStore::seal` must clear the bus tap so a publish issued after the
    /// run closes cannot reach the sealed writer. Without the clear, the dropped
    /// adapter would still receive captures and `Arc::try_unwrap` inside close would
    /// fail with multiple owners.
    #[rstest]
    fn seal_clears_bus_tap_so_post_seal_dispatches_do_not_capture() {
        let tmp = TempDir::new().expect("tempdir");
        let clock_rc: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let instance_id = UUID4::new();

        let mut store = KernelEventStore::boot(
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
}
