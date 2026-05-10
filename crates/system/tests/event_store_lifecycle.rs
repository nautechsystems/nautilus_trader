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

//! Kernel-level integration tests for the event-store run lifecycle (Phase 7).
//!
//! Exercises the SPEC contract end-to-end through [`NautilusKernel`]: kernel boot
//! recovers crashed predecessors and a kernel that drops without explicit teardown
//! still seals the run via [`Drop`].

#![cfg(feature = "event_store")]

use std::{path::PathBuf, time::Duration};

use indexmap::IndexMap;
use nautilus_core::UUID4;
use nautilus_event_store::{RedbBackend, RunStatus};
use nautilus_system::{
    EventStoreConfig, NautilusKernelBuilder, RetentionMode, RunIdentity, recover_predecessors,
};
use rstest::rstest;
use tempfile::TempDir;

fn config_with(base_dir: PathBuf) -> EventStoreConfig {
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
fn kernel_drop_after_start_seals_run_as_ended() {
    // Imperative `engine.run()` followed by drop is the dominant backtest pattern;
    // BacktestEngine::end() never calls finalize_stop, and many callers skip
    // dispose(). The kernel's Drop impl is the last-chance seal site, so a normal
    // backtest exit must seal the run as Ended without leaving Running on disk.
    let tmp = TempDir::new().expect("tempdir");
    let instance_id = UUID4::new();

    let run_id = {
        let mut kernel = NautilusKernelBuilder::default()
            .with_instance_id(instance_id)
            .with_event_store_config(config_with(tmp.path().to_path_buf()))
            .build()
            .expect("kernel");

        kernel.start();
        kernel
            .event_store()
            .run_id()
            .expect("run open after start")
            .to_string()
    };

    let manifests = RedbBackend::list_runs(tmp.path(), &instance_id.to_string()).expect("list");
    let manifest = manifests
        .into_iter()
        .find(|m| m.run_id == run_id)
        .expect("manifest present");
    assert_eq!(
        manifest.status,
        RunStatus::Ended,
        "kernel Drop must seal the run on graceful exit",
    );
    assert!(
        manifest.high_watermark >= 2,
        "RunStarted at seq=1 plus RunEnded at seq=2; was {}",
        manifest.high_watermark,
    );

    // A second-boot recovery sweep must not chain to a run that closed cleanly.
    let outcome =
        recover_predecessors(tmp.path(), &instance_id.to_string()).expect("recovery sweep");
    assert!(
        outcome.recovered.is_empty(),
        "Ended runs are not predecessors to recover, was {:?}",
        outcome.recovered,
    );
    assert!(outcome.parent_run_id.is_none());
}
