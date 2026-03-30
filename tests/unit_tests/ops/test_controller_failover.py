from __future__ import annotations

import importlib
import importlib.util
import json
import sys
from pathlib import Path
from types import ModuleType

import pytest


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def _load_failover_module() -> ModuleType:
    path = _repo_root() / "ops/scripts/failover/controller_scope_failover.py"
    assert path.exists(), "controller failover script should exist"

    spec = importlib.util.spec_from_file_location("task6_controller_scope_failover", path)
    assert spec is not None
    assert spec.loader is not None

    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _load_leases_module() -> ModuleType:
    path = _repo_root() / "systems/flux/flux/execution/leases.py"
    assert path.exists(), "controller lease module should exist"
    return importlib.import_module("flux.execution.leases")


def test_single_host_failover_threshold_report_is_locked(capsys) -> None:
    module = _load_failover_module()

    exit_code = module.main(
        [
            "--profile",
            "equities",
            "--scope",
            "ibkr.hedge.main",
            "--single-host",
            "--check-thresholds",
        ]
    )

    assert exit_code == 0
    assert json.loads(capsys.readouterr().out) == {
        "profile": "equities",
        "scope": "ibkr.hedge.main",
        "single_host": True,
        "stale_writer_stop_latency_ms": 120,
        "split_brain_rejected": True,
        "duplicate_writes": 0,
        "ambiguous_order_owners": 0,
        "rollback_bounces": 1,
        "budget_check": {
            "passed": True,
            "violations": [],
        },
    }


def test_multi_box_failover_threshold_report_is_locked(capsys) -> None:
    module = _load_failover_module()

    exit_code = module.main(
        [
            "--profile",
            "equities",
            "--scope",
            "ibkr.hedge.main",
            "--multi-box",
            "--check-thresholds",
        ]
    )

    assert exit_code == 0
    assert json.loads(capsys.readouterr().out) == {
        "profile": "equities",
        "scope": "ibkr.hedge.main",
        "single_host": False,
        "multi_box": True,
        "replicated_ownership_log": True,
        "partition_stale_writer_rejected": True,
        "stale_writer_stop_latency_ms": 120,
        "split_brain_rejected": True,
        "duplicate_writes": 0,
        "ambiguous_order_owners": 0,
        "rollback_bounces": 1,
        "budget_check": {
            "passed": True,
            "violations": [],
        },
    }


def test_assert_failover_thresholds_rejects_duplicate_writes_and_slow_stale_writer() -> None:
    module = _load_failover_module()

    report = module.build_failover_report(
        profile="equities",
        scope="ibkr.hedge.main",
        single_host=True,
        stale_writer_stop_latency_ms=400,
        split_brain_rejected=False,
        duplicate_writes=1,
        ambiguous_order_owners=1,
        rollback_bounces=2,
    )

    with pytest.raises(
        ValueError,
        match="stale_writer_stop_latency_ms|split_brain_rejected|duplicate_writes|ambiguous_order_owners|rollback_bounces",
    ):
        module.assert_failover_thresholds(report)


def test_multi_box_lease_handoff_exposes_epoch_and_rejects_stale_writer(tmp_path: Path) -> None:
    from flux.execution.leases import LocalControllerLeaseStore
    from flux.execution.leases import StaleControllerWriterError

    store = LocalControllerLeaseStore(root_dir=tmp_path / "leases")

    first = store.acquire(
        controller_scope_id="acct.execution.main",
        owner_id="writer-a",
        now_ms=1_000,
        lease_ttl_ms=100,
    )
    replacement = store.acquire(
        controller_scope_id="acct.execution.main",
        owner_id="writer-b",
        now_ms=1_101,
        lease_ttl_ms=100,
    )

    assert first.lease_epoch == 1
    assert replacement.lease_epoch == 2
    with pytest.raises(StaleControllerWriterError, match="different lease token"):
        store.assert_can_write(
            controller_scope_id="acct.execution.main",
            lease_token=first.lease_token,
            now_ms=1_101,
        )


def test_multi_box_replica_lease_handoff_rejects_stale_writer(tmp_path: Path) -> None:
    leases = _load_leases_module()
    shared_replica = tmp_path / "replica"
    store_a = leases.LocalControllerLeaseStore(
        root_dir=tmp_path / "host-a",
        replica_root_dirs=(shared_replica,),
    )
    store_b = leases.LocalControllerLeaseStore(
        root_dir=tmp_path / "host-b",
        replica_root_dirs=(shared_replica,),
    )

    first = store_a.acquire(
        controller_scope_id="acct.execution.main",
        owner_id="controller-a",
        now_ms=1_000,
        lease_ttl_ms=250,
    )
    replacement = store_b.acquire(
        controller_scope_id="acct.execution.main",
        owner_id="controller-b",
        now_ms=1_251,
        lease_ttl_ms=250,
    )

    assert replacement.lease_epoch == first.lease_epoch + 1
    with pytest.raises(leases.StaleControllerWriterError, match="different lease token|lease epoch"):
        store_a.assert_can_write(
            controller_scope_id="acct.execution.main",
            lease_token=first.lease_token,
            now_ms=1_251,
        )


def test_replicated_controller_leases_reject_writes_when_replica_state_diverges(
    tmp_path: Path,
) -> None:
    leases = _load_leases_module()
    store = leases.ReplicatedControllerLeaseStore(
        root_dirs=(
            tmp_path / "leases-a",
            tmp_path / "leases-b",
        ),
    )

    lease = store.acquire(
        controller_scope_id="acct.execution.main",
        owner_id="controller-a",
        now_ms=1_000,
        lease_ttl_ms=250,
    )
    store.refresh(
        controller_scope_id="acct.execution.main",
        lease_token=lease.lease_token,
        now_ms=1_100,
    )

    replica_b = leases.LocalControllerLeaseStore(root_dir=tmp_path / "leases-b")
    replica_b.release(
        controller_scope_id="acct.execution.main",
        lease_token=lease.lease_token,
    )

    with pytest.raises(leases.StaleControllerWriterError, match="replica state diverged"):
        store.assert_can_write(
            controller_scope_id="acct.execution.main",
            lease_token=lease.lease_token,
            now_ms=1_150,
        )
