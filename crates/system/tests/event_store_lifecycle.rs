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

use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use indexmap::IndexMap;
use nautilus_common::cache::Cache;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_event_store::{
    AppendEntry, EventStore, EventStoreEntry, Headers, PAYLOAD_TYPE_ACCOUNT_STATE, RedbBackend,
    RegisteredComponents, RunManifest, RunStatus, SnapshotAnchor, Topic, compute_entry_hash,
    compute_snapshot_content_hash, encode_account_state,
};
use nautilus_execution::engine::{
    ExecutionEngine, config::ExecutionEngineConfig, stubs::StubExecutionClient,
};
use nautilus_model::{
    accounts::CashAccount,
    enums::{OmsType, OrderSide, OrderType},
    events::{AccountState, account::stubs::cash_account_state_million_usd},
    identifiers::{
        AccountId, ClientId, ClientOrderId, PositionId, StrategyId, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{CurrencyPair, InstrumentAny, stubs::audusd_sim},
    orders::{Order, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    stubs::TestDefault,
    types::Quantity,
};
use nautilus_system::{
    EventStoreConfig, NautilusKernelBuilder, RetentionMode, RunIdentity, recover_predecessors,
};
use rstest::rstest;
use tempfile::TempDir;
use ustr::Ustr;

static KERNEL_TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock_kernel_test() -> MutexGuard<'static, ()> {
    KERNEL_TEST_LOCK.lock().expect("kernel test lock")
}

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

fn running_manifest(config: &EventStoreConfig, instance_id: UUID4, run_id: &str) -> RunManifest {
    RunManifest {
        run_id: run_id.to_string(),
        parent_run_id: None,
        instance_id: instance_id.to_string(),
        binary_hash: config.identity.binary_hash.clone(),
        schema_version: config.identity.schema_version,
        crate_versions: config.identity.crate_versions.clone(),
        feature_flags: config.identity.feature_flags.clone(),
        adapter_versions: config.identity.adapter_versions.clone(),
        config_hash: config.identity.config_hash.clone(),
        registered_components: RegisteredComponents::default(),
        seed: config.identity.seed,
        start_ts_init: UnixNanos::from(1),
        end_ts_init: None,
        high_watermark: 0,
        status: RunStatus::Running,
    }
}

fn append_account_state(seq: u64, state: &AccountState) -> AppendEntry {
    let encoded = encode_account_state(state).expect("encode account state");
    let topic = Topic::from("events.account.SIM");
    let ts = UnixNanos::from(seq);
    let headers = Headers::empty();
    let hash = compute_entry_hash(
        seq,
        ts,
        ts,
        topic.as_ref(),
        PAYLOAD_TYPE_ACCOUNT_STATE,
        &encoded.payload,
        &headers,
    );
    let entry = EventStoreEntry::new(
        hash,
        seq,
        headers,
        topic,
        Ustr::from(PAYLOAD_TYPE_ACCOUNT_STATE),
        encoded.payload,
        ts,
        ts,
    );

    AppendEntry::without_indices(entry)
}

fn setup_netting_snapshot_engine(
    execution_engine: &mut ExecutionEngine,
    instrument: &CurrencyPair,
) {
    let stub_client = StubExecutionClient::new(
        ClientId::from("STUB"),
        AccountId::test_default(),
        Venue::test_default(),
        OmsType::Netting,
        None,
    );
    execution_engine
        .register_client(Box::new(stub_client))
        .expect("register stub client");
    execution_engine
        .cache()
        .borrow_mut()
        .add_instrument(instrument.clone().into())
        .expect("add instrument");
    execution_engine
        .cache()
        .borrow_mut()
        .add_account(CashAccount::default().into())
        .expect("add account");
}

#[expect(clippy::too_many_arguments)]
fn process_filled_order(
    execution_engine: &mut ExecutionEngine,
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument: &CurrencyPair,
    client_order_id: &str,
    venue_order_id: &str,
    trade_id: &str,
    side: OrderSide,
    quantity: u64,
    position_id: PositionId,
) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument.id)
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(side)
        .quantity(Quantity::from(quantity))
        .build();

    execution_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(ClientId::from("STUB")), true)
        .expect("add order");
    execution_engine.process(&TestOrderEventStubs::submitted(
        &order,
        AccountId::test_default(),
    ));
    execution_engine.process(&TestOrderEventStubs::accepted(
        &order,
        AccountId::test_default(),
        VenueOrderId::from(venue_order_id),
    ));

    let accepted_order = execution_engine
        .cache()
        .borrow()
        .order_owned(&order.client_order_id())
        .expect("accepted order");
    let instrument_any: InstrumentAny = instrument.clone().into();
    execution_engine.process(&TestOrderEventStubs::filled(
        &accepted_order,
        &instrument_any,
        Some(TradeId::new(trade_id)),
        Some(position_id),
        None,
        None,
        None,
        None,
        None,
        Some(AccountId::test_default()),
    ));
}

#[rstest]
fn kernel_drop_after_start_seals_run_as_ended() {
    let _guard = lock_kernel_test();

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

#[rstest]
fn kernel_start_installs_snapshot_anchorer_for_execution_snapshots() {
    let _guard = lock_kernel_test();
    let tmp = TempDir::new().expect("tempdir");
    let instance_id = UUID4::new();
    let config = config_with(tmp.path().to_path_buf());
    let instrument = audusd_sim();
    let trader_id = TraderId::test_default();
    let strategy_id = StrategyId::test_default();
    let position_id = PositionId::new(format!("{}-{strategy_id}", instrument.id));

    let mut kernel = NautilusKernelBuilder::default()
        .with_instance_id(instance_id)
        .with_exec_engine_config(ExecutionEngineConfig {
            snapshot_positions: true,
            ..Default::default()
        })
        .with_event_store_config(config.clone())
        .build()
        .expect("kernel");

    {
        let mut exec_engine = kernel.exec_engine.borrow_mut();
        setup_netting_snapshot_engine(&mut exec_engine, &instrument);
    }

    kernel.start();
    let run_id = kernel
        .event_store()
        .run_id()
        .expect("run open after start")
        .to_string();

    {
        let mut exec_engine = kernel.exec_engine.borrow_mut();
        process_filled_order(
            &mut exec_engine,
            trader_id,
            strategy_id,
            &instrument,
            "O-KERNEL-ANCHOR-1",
            "V-KERNEL-ANCHOR-1",
            "T-KERNEL-ANCHOR-1",
            OrderSide::Buy,
            100_000,
            position_id,
        );
        process_filled_order(
            &mut exec_engine,
            trader_id,
            strategy_id,
            &instrument,
            "O-KERNEL-ANCHOR-2",
            "V-KERNEL-ANCHOR-2",
            "T-KERNEL-ANCHOR-2",
            OrderSide::Sell,
            150_000,
            position_id,
        );
    }

    let snapshot = {
        let cache = kernel.cache.borrow();
        let frames = cache
            .position_snapshot_bytes(&position_id)
            .expect("position snapshot");
        assert_eq!(frames.len(), 1);
        frames[0].clone()
    };

    kernel.dispose();

    let reader = RedbBackend::open_sealed(&config.base_dir, &instance_id.to_string(), &run_id)
        .expect("open sealed run");
    let anchor = reader
        .latest_snapshot_anchor()
        .expect("latest snapshot anchor")
        .expect("anchor present");
    let durable_high_watermark = reader.high_watermark().expect("high watermark");

    assert_eq!(
        anchor.blob_ref,
        format!("cache://position-snapshots/{}/0", position_id.as_str()),
    );
    assert_eq!(
        anchor.content_hash,
        compute_snapshot_content_hash(&snapshot),
    );
    assert!(anchor.high_watermark >= 1);
    assert!(
        anchor.high_watermark <= durable_high_watermark,
        "anchor high_watermark {} exceeded durable high_watermark {}",
        anchor.high_watermark,
        durable_high_watermark,
    );
    assert!(
        reader
            .scan_seq(anchor.high_watermark)
            .expect("anchor high-watermark seq")
            .is_some(),
        "anchor must point to an existing durable event",
    );
}

#[rstest]
fn kernel_start_restores_parent_cache_snapshot_and_replays_tail() {
    let _guard = lock_kernel_test();
    let tmp = TempDir::new().expect("tempdir");
    let instance_id = UUID4::new();
    let config = config_with(tmp.path().to_path_buf());
    let parent_run_id = "parent-run";
    let instrument = audusd_sim();
    let instrument_any = InstrumentAny::CurrencyPair(instrument.clone());
    let position_id = PositionId::new("P-KERNEL-RESTORE-1");
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100_000))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument_any,
        Some(TradeId::new("T-KERNEL-RESTORE-1")),
        Some(position_id),
        None,
        None,
        None,
        None,
        None,
        Some(AccountId::test_default()),
    );
    let position = Position::new(&instrument_any, fill.into());
    let mut snapshot_cache = Cache::default();
    let snapshot_ref = snapshot_cache
        .snapshot_position(&position)
        .expect("snapshot position");
    let anchored_state = cash_account_state_million_usd("100 USD", "0 USD", "100 USD");
    let replayed_state = cash_account_state_million_usd("200 USD", "0 USD", "200 USD");

    {
        let mut backend = RedbBackend::new(config.base_dir.clone());
        backend
            .open_run(running_manifest(&config, instance_id, parent_run_id))
            .expect("open parent run");
        backend
            .append_batch(&[append_account_state(1, &anchored_state)])
            .expect("append anchored state");
        backend
            .record_snapshot_anchor(SnapshotAnchor::new(
                1,
                snapshot_ref.blob_ref.clone(),
                compute_snapshot_content_hash(snapshot_ref.blob.as_ref()),
            ))
            .expect("record snapshot anchor");
        backend
            .append_batch(&[append_account_state(2, &replayed_state)])
            .expect("append replay tail");
    }

    let mut kernel = NautilusKernelBuilder::default()
        .with_instance_id(instance_id)
        .with_event_store_config(config)
        .build()
        .expect("kernel");
    kernel
        .cache
        .borrow_mut()
        .add(&snapshot_ref.blob_ref, snapshot_ref.blob.clone())
        .expect("seed cache-owned snapshot blob");

    kernel.start();

    {
        let cache = kernel.cache.borrow();
        let frames = cache
            .position_snapshot_bytes(&position.id)
            .expect("restored position snapshot");
        let account = cache
            .account_owned(&replayed_state.account_id)
            .expect("replayed account");

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].as_slice(), snapshot_ref.blob.as_ref());
        assert_eq!(account.events(), vec![replayed_state]);
    }

    assert_eq!(kernel.event_store().parent_run_id(), Some(parent_run_id));
}

#[rstest]
#[case::missing_blob(false, false)]
#[case::hash_mismatch(true, true)]
fn kernel_start_restore_failure_does_not_open_new_run(
    #[case] seed_blob: bool,
    #[case] bad_hash: bool,
) {
    let _guard = lock_kernel_test();
    let tmp = TempDir::new().expect("tempdir");
    let instance_id = UUID4::new();
    let config = config_with(tmp.path().to_path_buf());
    let parent_run_id = "parent-run";
    let instrument = audusd_sim();
    let instrument_any = InstrumentAny::CurrencyPair(instrument.clone());
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100_000))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument_any,
        Some(TradeId::new("T-KERNEL-RESTORE-FAILURE-1")),
        Some(PositionId::new("P-KERNEL-RESTORE-FAILURE-1")),
        None,
        None,
        None,
        None,
        None,
        Some(AccountId::test_default()),
    );
    let position = Position::new(&instrument_any, fill.into());
    let mut snapshot_cache = Cache::default();
    let snapshot_ref = snapshot_cache
        .snapshot_position(&position)
        .expect("snapshot position");
    let anchored_state = cash_account_state_million_usd("100 USD", "0 USD", "100 USD");
    let content_hash = if bad_hash {
        "blake3:bad".to_string()
    } else {
        compute_snapshot_content_hash(snapshot_ref.blob.as_ref())
    };

    {
        let mut backend = RedbBackend::new(config.base_dir.clone());
        backend
            .open_run(running_manifest(&config, instance_id, parent_run_id))
            .expect("open parent run");
        backend
            .append_batch(&[append_account_state(1, &anchored_state)])
            .expect("append anchored state");
        backend
            .record_snapshot_anchor(SnapshotAnchor::new(
                1,
                snapshot_ref.blob_ref.clone(),
                content_hash,
            ))
            .expect("record snapshot anchor");
    }

    let mut kernel = NautilusKernelBuilder::default()
        .with_instance_id(instance_id)
        .with_event_store_config(config.clone())
        .build()
        .expect("kernel");

    if seed_blob {
        kernel
            .cache
            .borrow_mut()
            .add(&snapshot_ref.blob_ref, snapshot_ref.blob)
            .expect("seed cache-owned snapshot blob");
    }

    kernel.start();

    let manifests =
        RedbBackend::list_runs(&config.base_dir, &instance_id.to_string()).expect("list runs");

    assert_eq!(kernel.event_store().parent_run_id(), Some(parent_run_id));
    assert!(kernel.event_store().run_id().is_none());
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].run_id, parent_run_id);
}
