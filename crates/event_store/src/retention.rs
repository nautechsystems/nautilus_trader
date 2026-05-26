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

//! Non-destructive retention planning for sealed event-store run files.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use nautilus_system::event_store::RetentionMode;

use crate::{EventStore, EventStoreError, RedbBackend, RunManifest, RunStatus, SnapshotAnchor};

/// A retention decision for one sealed run file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionRun {
    /// The manifest stored in the run file.
    pub manifest: RunManifest,
    /// The on-disk run file path.
    pub path: PathBuf,
    /// The latest snapshot anchor state observed for the run.
    pub snapshot_anchor: SnapshotAnchorStatus,
}

impl RetentionRun {
    /// Creates a retention planning record for a sealed run file.
    #[must_use]
    pub fn new(
        manifest: RunManifest,
        path: impl Into<PathBuf>,
        snapshot_anchor: SnapshotAnchorStatus,
    ) -> Self {
        Self {
            manifest,
            path: path.into(),
            snapshot_anchor,
        }
    }

    /// Returns the run id recorded in the manifest.
    #[must_use]
    pub fn run_id(&self) -> &str {
        self.manifest.run_id.as_str()
    }

    /// Returns whether this run can serve as a conservative restore point.
    #[must_use]
    pub fn is_known_good_restore_point(&self) -> bool {
        !matches!(
            self.manifest.status,
            RunStatus::Running | RunStatus::Quarantined
        ) && matches!(&self.snapshot_anchor, SnapshotAnchorStatus::Valid(_))
    }
}

/// Snapshot-anchor state used by the retention planner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotAnchorStatus {
    /// The run has no recorded snapshot anchor.
    Missing,
    /// The run has a snapshot anchor that matches the sealed manifest.
    Valid(SnapshotAnchor),
    /// The run has an anchor, but retention must not rely on it.
    Invalid(String),
}

/// A non-destructive retention plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetentionPlan {
    /// Sealed runs visible to the planner, sorted by manifest start time.
    pub sealed_runs: Vec<RetentionRun>,
    /// Whole run files the selected policy may reclaim later.
    pub reclaim_candidates: Vec<RetentionRun>,
}

/// Lists sealed redb run files and computes the non-destructive retention plan.
///
/// # Errors
///
/// Returns [`EventStoreError`] when the run directory cannot be listed, a manifest cannot
/// be decoded, or a sealed run file cannot be opened to inspect its snapshot anchor.
pub fn plan_redb_retention(
    base_dir: &Path,
    instance_id: &str,
    mode: RetentionMode,
) -> Result<RetentionPlan, EventStoreError> {
    Ok(plan_retention(
        list_redb_sealed_runs(base_dir, instance_id)?,
        mode,
    ))
}

/// Lists sealed redb run files with their latest snapshot-anchor status.
///
/// `Running` manifests are excluded so a later destructive phase cannot reclaim a live or
/// crash-recovery-domain run by accident.
///
/// # Errors
///
/// Returns [`EventStoreError`] when manifest listing or sealed-run anchor inspection fails.
pub fn list_redb_sealed_runs(
    base_dir: &Path,
    instance_id: &str,
) -> Result<Vec<RetentionRun>, EventStoreError> {
    let manifests = RedbBackend::list_runs(base_dir, instance_id)?;
    let mut runs = Vec::new();

    for manifest in manifests {
        if !manifest.is_sealed() {
            continue;
        }

        let path = base_dir
            .join(instance_id)
            .join(format!("{}.redb", manifest.run_id));
        let reader = RedbBackend::open_sealed(base_dir, instance_id, manifest.run_id.as_str())?;
        let snapshot_anchor = match reader.latest_snapshot_anchor() {
            Ok(anchor) => snapshot_anchor_status(&manifest, anchor),
            Err(EventStoreError::Corrupted(msg)) => SnapshotAnchorStatus::Invalid(msg),
            Err(e) => return Err(e),
        };

        runs.push(RetentionRun::new(manifest, path, snapshot_anchor));
    }

    Ok(runs)
}

/// Computes reclaim candidates from sealed runs without deleting anything.
#[must_use]
pub fn plan_retention(mut sealed_runs: Vec<RetentionRun>, mode: RetentionMode) -> RetentionPlan {
    sealed_runs.retain(|run| run.manifest.is_sealed());
    sealed_runs.sort_by_key(|run| run.manifest.start_ts_init);

    let reclaim_candidates = match mode {
        RetentionMode::Full => Vec::new(),
        RetentionMode::Bounded { keep_last } => bounded_reclaim_candidates(&sealed_runs, keep_last),
        RetentionMode::SnapshotAnchored => snapshot_anchored_reclaim_candidates(&sealed_runs),
    };

    RetentionPlan {
        sealed_runs,
        reclaim_candidates,
    }
}

fn bounded_reclaim_candidates(sealed_runs: &[RetentionRun], keep_last: usize) -> Vec<RetentionRun> {
    let Some(latest_restore_point) = latest_known_good_restore_point(sealed_runs) else {
        return Vec::new();
    };

    let keep_last = keep_last.min(sealed_runs.len());
    let mut retained = BTreeSet::new();
    retained.insert(latest_restore_point);

    if keep_last > 0 {
        for index in sealed_runs.len() - keep_last..sealed_runs.len() {
            retained.insert(index);
        }
    }

    sealed_runs
        .iter()
        .enumerate()
        .filter(|(index, _)| !retained.contains(index))
        .map(|(_, run)| run.clone())
        .collect()
}

fn snapshot_anchored_reclaim_candidates(sealed_runs: &[RetentionRun]) -> Vec<RetentionRun> {
    let Some(latest_restore_point) = latest_known_good_restore_point(sealed_runs) else {
        return Vec::new();
    };

    sealed_runs[..latest_restore_point].to_vec()
}

fn latest_known_good_restore_point(sealed_runs: &[RetentionRun]) -> Option<usize> {
    sealed_runs
        .iter()
        .rposition(RetentionRun::is_known_good_restore_point)
}

fn snapshot_anchor_status(
    manifest: &RunManifest,
    anchor: Option<SnapshotAnchor>,
) -> SnapshotAnchorStatus {
    let Some(anchor) = anchor else {
        return SnapshotAnchorStatus::Missing;
    };

    if anchor.high_watermark <= manifest.high_watermark {
        return SnapshotAnchorStatus::Valid(anchor);
    }

    SnapshotAnchorStatus::Invalid(format!(
        "snapshot anchor high_watermark {} exceeds manifest high_watermark {}",
        anchor.high_watermark, manifest.high_watermark,
    ))
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::RegisteredComponents;

    #[rstest]
    fn snapshot_anchor_status_rejects_anchor_past_manifest_watermark() {
        let status = snapshot_anchor_status(
            &manifest_with_high_watermark(1),
            Some(SnapshotAnchor::new(2, "cache://snapshots/2", "blake3:abc")),
        );

        match status {
            SnapshotAnchorStatus::Invalid(msg) => {
                assert!(
                    msg.contains("exceeds manifest high_watermark"),
                    "msg was: {msg}",
                );
            }
            other => panic!("expected Invalid, was {other:?}"),
        }
    }

    fn manifest_with_high_watermark(high_watermark: u64) -> RunManifest {
        RunManifest {
            run_id: "run-1".to_string(),
            parent_run_id: None,
            instance_id: "trader-001".to_string(),
            binary_hash: "deadbeef".to_string(),
            schema_version: 1,
            crate_versions: "feedface".to_string(),
            feature_flags: Vec::new(),
            adapter_versions: IndexMap::new(),
            config_hash: "cafebabe".to_string(),
            registered_components: RegisteredComponents::default(),
            seed: None,
            start_ts_init: UnixNanos::from(1),
            end_ts_init: None,
            high_watermark,
            status: RunStatus::Ended,
        }
    }
}
