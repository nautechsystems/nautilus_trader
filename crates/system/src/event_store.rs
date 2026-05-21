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

//! Kernel-facing seam for run-lifecycle event-sourcing.
//!
//! The [`KernelEventStore`] trait is the surface [`crate::kernel::NautilusKernel`] uses to wire
//! a durable event-sourcing session into its boot, snapshot, and seal flow. The concrete
//! implementation lives in `nautilus-event-store` so that crate can be developed and versioned
//! independently of `nautilus-system`; callers inject an implementation through the builder
//! (see [`crate::builder::NautilusKernelBuilder::with_event_store`]).

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use indexmap::IndexMap;
use nautilus_common::{cache::Cache, clock::Clock, enums::Environment};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::engine::SnapshotAnchorer;
use serde::{Deserialize, Serialize};

/// Factory closure invoked by the kernel to construct an injected event-store implementation.
///
/// Receives the kernel's instance id and clock so the resulting [`KernelEventStore`]
/// implementation scans the same on-disk run directory the kernel later passes to
/// `restore_parent_cache`/`open`, and stamps lifecycle timestamps against the same time
/// source the kernel uses.
pub type EventStoreFactory = Box<
    dyn FnOnce(UUID4, Rc<RefCell<dyn Clock>>) -> anyhow::Result<Box<dyn KernelEventStore>>
        + 'static,
>;

/// The component manifest captured into the event-store `RunStarted` entry.
///
/// Replay binds actors, strategies, algorithms, subscriptions, and command endpoints from
/// this manifest without consulting external configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredComponents {
    /// Registered actor ids and their config hashes.
    pub actors: IndexMap<String, String>,
    /// Registered strategy ids and their config hashes.
    pub strategies: IndexMap<String, String>,
    /// Registered algorithm ids and their config hashes.
    pub algorithms: IndexMap<String, String>,
    /// Subscription bindings active at run start.
    pub subscriptions: Vec<String>,
    /// Endpoint registrations active at run start.
    pub endpoints: Vec<String>,
}

/// Kernel-facing seam for event-sourcing lifecycle integration.
///
/// `NautilusKernel` drives the open/restore/seal sequence through this trait so the concrete
/// event-store machinery (writers, readers, bus tap, redb backend) lives outside
/// `nautilus-system`. Implementations are typically built by the caller and injected via
/// [`crate::builder::NautilusKernelBuilder::with_event_store`].
pub trait KernelEventStore: Debug {
    /// Restores cache state from a configured replay source or recovered parent run.
    ///
    /// Implementations may open a sealed replay source, validate its snapshot anchor, and
    /// replay the tail directly into `cache`. The kernel calls this once before [`Self::open`].
    ///
    /// # Errors
    ///
    /// Returns an error when the source reader, snapshot restore, decode, or cache apply
    /// step fails.
    fn restore_parent_cache(&mut self, instance_id: UUID4, cache: &mut Cache)
    -> anyhow::Result<()>;

    /// Opens a fresh run for the current kernel session.
    ///
    /// `components` carries the registered manifest written to the run's `RunStarted` entry.
    /// `environment` selects the clock source the implementation uses to stamp publish
    /// timestamps. Idempotency across reset/rerun is the implementation's responsibility.
    ///
    /// # Errors
    ///
    /// Returns an error when opening the new run, spawning the writer, or blocking on the
    /// initial entry ack fails.
    fn open(
        &mut self,
        instance_id: UUID4,
        components: &RegisteredComponents,
        environment: Environment,
    ) -> anyhow::Result<()>;

    /// Returns a snapshot anchorer for the currently open run, when capture is active.
    ///
    /// The execution engine installs the returned callback so position snapshots commit a
    /// matching anchor entry against the durable high-watermark.
    fn snapshot_anchorer(&self) -> Option<SnapshotAnchorer>;

    /// Seals the open run by writing the terminal entry and updating the manifest.
    ///
    /// Idempotent: a closed or absent session is a no-op. Halted sessions defer the seal to
    /// the next-boot recovery sweep.
    fn seal(&mut self, ts_init: UnixNanos);

    /// Returns the run id of the currently open run, when capture is active.
    fn run_id(&self) -> Option<&str>;

    /// Returns the configured replay source or recovered parent run id, when present.
    fn parent_run_id(&self) -> Option<&str>;

    /// Returns whether the current config enables event-store replay.
    ///
    /// Event-store replay restores cache state and opens a child run for inspection. The kernel
    /// promotes this config state to runtime state only after restore and open both succeed.
    fn is_event_store_replay_configured(&self) -> bool {
        false
    }

    /// Returns whether the implementation has signaled a fail-stop condition.
    fn is_halted(&self) -> bool;
}
