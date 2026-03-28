from __future__ import annotations

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
