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

//! The per-run manifest stored alongside captured entries.

use indexmap::IndexMap;
use nautilus_core::UnixNanos;
pub use nautilus_system::RegisteredComponents;
use serde::{Deserialize, Serialize};

use crate::wire;

/// The id of a captured run: `<start_ts_init>-<short_uuid>`, sortable by start time.
///
/// The runtime constructs this from the kernel's start timestamp plus a fresh `UUID4` so the
/// representation stays stable across processes and platforms.
pub type RunId = String;

/// Lifecycle state of a captured run.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunStatus {
    /// The run is open and accepting writes.
    Running,
    /// The run was sealed by graceful shutdown after a `RunEnded` entry.
    Ended,
    /// The run was sealed on boot after the writer found no `RunEnded` entry.
    CrashedRecovered,
    /// The run failed an integrity check and is unsafe to replay.
    Quarantined,
}

/// Per-run manifest persisted alongside captured entries.
///
/// Every field is recorded at run start; `end_ts_init`, `high_watermark`, and `status` are
/// updated when the run is sealed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunManifest {
    /// The id of this run.
    pub run_id: RunId,
    /// The id of the predecessor run that this run resumes from, if any.
    pub parent_run_id: Option<RunId>,
    /// The id of the trading instance owning this run.
    pub instance_id: String,
    /// A hex-encoded hash of the trader binary.
    pub binary_hash: String,
    /// Bumps when the entry payload schema changes.
    pub schema_version: u32,
    /// A hex-encoded hash of `Cargo.lock` or an equivalent crate version manifest.
    pub crate_versions: String,
    /// The active Cargo features for the trader binary.
    pub feature_flags: Vec<String>,
    /// Per-adapter version stamp keyed by adapter name.
    pub adapter_versions: IndexMap<String, String>,
    /// A hex-encoded hash of the kernel configuration.
    pub config_hash: String,
    /// Registered actor, strategy, and algorithm identities along with subscription bindings.
    pub registered_components: RegisteredComponents,
    /// The deterministic seed, populated when the run executes under a seeded mode.
    pub seed: Option<u64>,
    /// The first `ts_init` observed by the writer for this run.
    #[serde(with = "wire::nanos_as_u64")]
    pub start_ts_init: UnixNanos,
    /// The last `ts_init` observed by the writer for this run, populated on seal.
    #[serde(with = "wire::opt_nanos_as_u64")]
    pub end_ts_init: Option<UnixNanos>,
    /// The largest `seq` durably acknowledged by the backend at end of run.
    pub high_watermark: u64,
    /// The lifecycle state of this run.
    pub status: RunStatus,
}

impl RunManifest {
    /// Returns `true` once `status` is anything other than [`RunStatus::Running`].
    #[must_use]
    pub const fn is_sealed(&self) -> bool {
        !matches!(self.status, RunStatus::Running)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn manifest_with(status: RunStatus) -> RunManifest {
        RunManifest {
            run_id: "1700000000-abcd1234".to_string(),
            parent_run_id: None,
            instance_id: "trader-001".to_string(),
            binary_hash: "deadbeef".to_string(),
            schema_version: 1,
            crate_versions: "feedface".to_string(),
            feature_flags: vec!["live".to_string()],
            adapter_versions: IndexMap::new(),
            config_hash: "cafebabe".to_string(),
            registered_components: RegisteredComponents::default(),
            seed: None,
            start_ts_init: UnixNanos::from(0),
            end_ts_init: None,
            high_watermark: 0,
            status,
        }
    }

    #[rstest]
    #[case(RunStatus::Running, false)]
    #[case(RunStatus::Ended, true)]
    #[case(RunStatus::CrashedRecovered, true)]
    #[case(RunStatus::Quarantined, true)]
    fn is_sealed_matches_status(#[case] status: RunStatus, #[case] expected: bool) {
        assert_eq!(manifest_with(status).is_sealed(), expected);
    }
}
