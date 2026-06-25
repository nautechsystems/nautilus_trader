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

use std::path::{Path, PathBuf};

use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use nautilus_event_store::{
    AppendEntry, EventStore, EventStoreEntry, Headers, RedbBackend, RegisteredComponents,
    RetentionMode, RetentionRun, RunManifest, RunStatus, SnapshotAnchor, SnapshotAnchorStatus,
    Topic, compute_entry_hash, plan_redb_retention, plan_retention,
};
use rstest::rstest;
use tempfile::TempDir;
use ustr::Ustr;

const INSTANCE_ID: &str = "trader-001";

#[rstest]
fn full_mode_returns_no_reclaim_candidates() {
    let plan = plan_retention(
        vec![
            planning_run("run-1", RunStatus::Ended, 1, valid_anchor(1)),
            planning_run("run-2", RunStatus::CrashedRecovered, 2, valid_anchor(1)),
        ],
        RetentionMode::Full,
    );

    assert!(plan.reclaim_candidates.is_empty());
}

#[rstest]
fn plan_retention_sorts_runs_before_selecting_candidates() {
    let plan = plan_retention(
        vec![
            planning_run("newest", RunStatus::Ended, 3, valid_anchor(1)),
            planning_run("oldest", RunStatus::Ended, 1, valid_anchor(1)),
            planning_run("middle", RunStatus::Ended, 2, valid_anchor(1)),
        ],
        RetentionMode::Bounded { keep_last: 1 },
    );

    assert_eq!(
        run_ids(&plan.sealed_runs),
        vec!["oldest", "middle", "newest"]
    );
    assert_eq!(run_ids(&plan.reclaim_candidates), vec!["oldest", "middle"]);
}

#[rstest]
fn redb_planner_lists_sealed_runs_and_never_selects_running_runs() {
    let tmp = TempDir::new().expect("tempdir");
    write_redb_run(tmp.path(), "run-1", 1, RunStatus::Ended, true);
    write_redb_run(tmp.path(), "run-2", 2, RunStatus::Ended, true);
    write_running_redb_run(tmp.path(), "run-running", 3);

    let plan = plan_redb_retention(
        tmp.path(),
        INSTANCE_ID,
        RetentionMode::Bounded { keep_last: 1 },
    )
    .expect("plan retention");

    assert_eq!(run_ids(&plan.sealed_runs), vec!["run-1", "run-2"]);
    assert_eq!(run_ids(&plan.reclaim_candidates), vec!["run-1"]);
}

#[rstest]
fn plan_retention_filters_running_runs_before_selecting_candidates() {
    let plan = plan_retention(
        vec![
            planning_run("old-good", RunStatus::Ended, 1, valid_anchor(1)),
            planning_run("running", RunStatus::Running, 2, valid_anchor(1)),
            planning_run("new-good", RunStatus::Ended, 3, valid_anchor(1)),
        ],
        RetentionMode::Bounded { keep_last: 1 },
    );

    assert_eq!(run_ids(&plan.sealed_runs), vec!["old-good", "new-good"]);
    assert_eq!(run_ids(&plan.reclaim_candidates), vec!["old-good"]);
}

#[rstest]
fn redb_planner_treats_corrupt_snapshot_anchor_as_invalid() {
    let tmp = TempDir::new().expect("tempdir");
    let path = write_redb_run(tmp.path(), "run-corrupt-anchor", 1, RunStatus::Ended, true);

    overwrite_snapshot_anchor(&path);

    let plan = plan_redb_retention(tmp.path(), INSTANCE_ID, RetentionMode::SnapshotAnchored)
        .expect("plan retention");

    assert_eq!(plan.sealed_runs.len(), 1);
    assert!(plan.reclaim_candidates.is_empty());

    match &plan.sealed_runs[0].snapshot_anchor {
        SnapshotAnchorStatus::Invalid(msg) => {
            assert!(msg.contains("decode snapshot anchor"), "msg was: {msg}");
        }
        other => panic!("expected invalid snapshot anchor, was {other:?}"),
    }
}

#[rstest]
fn bounded_mode_keeps_known_good_restore_point_when_latest_is_quarantined() {
    let plan = plan_retention(
        vec![
            planning_run("old-good", RunStatus::Ended, 1, valid_anchor(1)),
            planning_run(
                "restore-point",
                RunStatus::CrashedRecovered,
                2,
                valid_anchor(1),
            ),
            planning_run("quarantined", RunStatus::Quarantined, 3, valid_anchor(1)),
        ],
        RetentionMode::Bounded { keep_last: 1 },
    );

    assert_eq!(run_ids(&plan.reclaim_candidates), vec!["old-good"]);
}

#[rstest]
fn bounded_mode_returns_no_candidates_without_known_good_restore_point() {
    let plan = plan_retention(
        vec![
            planning_run(
                "missing-anchor",
                RunStatus::Ended,
                1,
                SnapshotAnchorStatus::Missing,
            ),
            planning_run("quarantined", RunStatus::Quarantined, 2, valid_anchor(1)),
        ],
        RetentionMode::Bounded { keep_last: 0 },
    );

    assert!(plan.reclaim_candidates.is_empty());
}

#[rstest]
fn snapshot_anchored_mode_reclaims_only_before_latest_valid_restore_point() {
    let plan = plan_retention(
        vec![
            planning_run("old-good", RunStatus::Ended, 1, valid_anchor(1)),
            planning_run("latest-anchor", RunStatus::Ended, 2, valid_anchor(1)),
            planning_run(
                "newer-missing",
                RunStatus::Ended,
                3,
                SnapshotAnchorStatus::Missing,
            ),
        ],
        RetentionMode::SnapshotAnchored,
    );

    assert_eq!(run_ids(&plan.reclaim_candidates), vec!["old-good"]);
}

#[rstest]
#[case::missing(SnapshotAnchorStatus::Missing)]
#[case::invalid(SnapshotAnchorStatus::Invalid(
    "snapshot anchor high_watermark 2 exceeds durable high_watermark 1".to_string(),
))]
fn snapshot_anchored_mode_handles_unusable_snapshot_anchors_conservatively(
    #[case] snapshot_anchor: SnapshotAnchorStatus,
) {
    let plan = plan_retention(
        vec![planning_run(
            "unusable-anchor",
            RunStatus::Ended,
            1,
            snapshot_anchor,
        )],
        RetentionMode::SnapshotAnchored,
    );

    assert!(plan.reclaim_candidates.is_empty());
}

fn planning_run(
    run_id: &str,
    status: RunStatus,
    start_ts_init: u64,
    snapshot_anchor: SnapshotAnchorStatus,
) -> RetentionRun {
    RetentionRun::new(
        manifest(run_id, start_ts_init, status),
        format!("/event-store/{INSTANCE_ID}/{run_id}.redb"),
        snapshot_anchor,
    )
}

fn valid_anchor(high_watermark: u64) -> SnapshotAnchorStatus {
    SnapshotAnchorStatus::Valid(SnapshotAnchor::new(
        high_watermark,
        format!("cache://snapshots/{high_watermark}"),
        "blake3:abc",
    ))
}

fn write_redb_run(
    base_dir: &Path,
    run_id: &str,
    start_ts_init: u64,
    status: RunStatus,
    record_anchor: bool,
) -> PathBuf {
    let mut backend = RedbBackend::new(base_dir);
    backend
        .open_run(manifest(run_id, start_ts_init, RunStatus::Running))
        .expect("open run");
    backend
        .append_batch(&[append_with(1, start_ts_init)])
        .expect("append");

    if record_anchor {
        backend
            .record_snapshot_anchor(SnapshotAnchor::new(1, "cache://snapshots/1", "blake3:abc"))
            .expect("record anchor");
    }

    let path = backend.current_path().expect("path").to_path_buf();
    backend.seal(status).expect("seal");
    path
}

fn write_running_redb_run(base_dir: &Path, run_id: &str, start_ts_init: u64) {
    let mut backend = RedbBackend::new(base_dir);
    backend
        .open_run(manifest(run_id, start_ts_init, RunStatus::Running))
        .expect("open run");
    backend
        .append_batch(&[append_with(1, start_ts_init)])
        .expect("append");
}

fn manifest(run_id: &str, start_ts_init: u64, status: RunStatus) -> RunManifest {
    RunManifest {
        run_id: run_id.to_string(),
        parent_run_id: None,
        instance_id: INSTANCE_ID.to_string(),
        binary_hash: "deadbeef".to_string(),
        schema_version: 1,
        crate_versions: "feedface".to_string(),
        feature_flags: Vec::new(),
        adapter_versions: IndexMap::new(),
        config_hash: "cafebabe".to_string(),
        registered_components: RegisteredComponents::default(),
        seed: None,
        start_ts_init: UnixNanos::from(start_ts_init),
        end_ts_init: None,
        high_watermark: 1,
        status,
    }
}

fn append_with(seq: u64, ts_init: u64) -> AppendEntry {
    AppendEntry::without_indices(build_entry(seq, Headers::empty(), ts_init))
}

fn build_entry(seq: u64, headers: Headers, ts_init: u64) -> EventStoreEntry {
    let topic: Topic = "exec.command.SubmitOrder".into();
    let payload_type = Ustr::from("SubmitOrder");
    let payload = Bytes::from_static(b"\x01\x02\x03\x04");
    let ts_publish = UnixNanos::from(ts_init + 1);
    let ts_init = UnixNanos::from(ts_init);
    let hash = compute_entry_hash(
        seq,
        ts_init,
        ts_publish,
        topic.as_ref(),
        payload_type.as_str(),
        &payload,
        &headers,
    );

    EventStoreEntry::new(
        hash,
        seq,
        headers,
        topic,
        payload_type,
        payload,
        ts_init,
        ts_publish,
    )
}

fn overwrite_snapshot_anchor(path: &Path) {
    let snapshot_anchor: redb::TableDefinition<&str, &[u8]> =
        redb::TableDefinition::new("snapshot_anchor");
    let db = redb::Database::create(path).expect("open redb");
    let txn = db.begin_write().expect("begin write");
    {
        let mut table = txn.open_table(snapshot_anchor).expect("open table");
        table
            .insert("latest", b"\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF".as_slice())
            .expect("overwrite latest snapshot anchor");
    }
    txn.commit().expect("commit overwrite");
}

fn run_ids(runs: &[RetentionRun]) -> Vec<&str> {
    runs.iter().map(RetentionRun::run_id).collect()
}
