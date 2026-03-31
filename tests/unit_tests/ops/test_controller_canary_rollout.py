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


def test_canary_session_artifact_passes_when_shadow_parity_and_rollback_gates_are_clean(
    tmp_path: Path,
    capsys,
) -> None:
    module = _load_failover_module()
    session_path = tmp_path / "session.json"
    session_path.write_text(
        json.dumps(
            {
                "profile": "equities",
                "scope": "ibkr.hedge.main",
                "single_host": True,
                "latency_budget_passed": True,
                "shadow_order_diffs": 0,
                "shadow_fill_diffs": 0,
                "shadow_position_diffs": 0,
                "unexplained_ownership_diffs": 0,
                "balance_divergence": False,
                "rollback_bounces": 1,
            },
            sort_keys=True,
        ),
        encoding="utf-8",
    )

    exit_code = module.main(
        [
            "--profile",
            "equities",
            "--scope",
            "ibkr.hedge.main",
            "--single-host",
            "--check-canary-session",
            "--session-artifact",
            str(session_path),
        ]
    )

    assert exit_code == 0
    assert json.loads(capsys.readouterr().out) == {
        "profile": "equities",
        "scope": "ibkr.hedge.main",
        "single_host": True,
        "latency_budget_passed": True,
        "shadow_order_diffs": 0,
        "shadow_fill_diffs": 0,
        "shadow_position_diffs": 0,
        "unexplained_ownership_diffs": 0,
        "balance_divergence": False,
        "rollback_bounces": 1,
        "budget_check": {
            "passed": True,
            "violations": [],
        },
    }


def test_canary_session_artifact_rejects_unexplained_ownership_diffs_and_balance_divergence(
    tmp_path: Path,
) -> None:
    module = _load_failover_module()
    session_path = tmp_path / "bad-session.json"
    session_path.write_text(
        json.dumps(
            {
                "profile": "equities",
                "scope": "ibkr.hedge.main",
                "single_host": True,
                "latency_budget_passed": True,
                "shadow_order_diffs": 0,
                "shadow_fill_diffs": 1,
                "shadow_position_diffs": 0,
                "unexplained_ownership_diffs": 2,
                "balance_divergence": True,
                "rollback_bounces": 2,
            },
            sort_keys=True,
        ),
        encoding="utf-8",
    )

    with pytest.raises(
        ValueError,
        match="shadow_fill_diffs|unexplained_ownership_diffs|balance_divergence|rollback_bounces",
    ):
        module.assert_canary_session(module.load_canary_session_artifact(session_path))


def test_canary_session_artifact_rejects_requested_target_mismatch(tmp_path: Path) -> None:
    module = _load_failover_module()
    session_path = tmp_path / "wrong-scope-session.json"
    session_path.write_text(
        json.dumps(
            {
                "profile": "equities",
                "scope": "ibkr.hedge.alt",
                "single_host": True,
                "latency_budget_passed": True,
                "shadow_order_diffs": 0,
                "shadow_fill_diffs": 0,
                "shadow_position_diffs": 0,
                "unexplained_ownership_diffs": 0,
                "balance_divergence": False,
                "rollback_bounces": 1,
            },
            sort_keys=True,
        ),
        encoding="utf-8",
    )

    report = module.load_canary_session_artifact(session_path)

    with pytest.raises(ValueError, match="requested scope"):
        module.assert_canary_session_target(
            report,
            profile="equities",
            scope="ibkr.hedge.main",
            single_host=True,
        )


def test_canary_session_cli_rejects_artifact_context_mismatch(tmp_path: Path) -> None:
    module = _load_failover_module()
    session_path = tmp_path / "wrong-scope-session.json"
    session_path.write_text(
        json.dumps(
            {
                "profile": "equities",
                "scope": "ibkr.hedge.secondary",
                "single_host": True,
                "latency_budget_passed": True,
                "shadow_order_diffs": 0,
                "shadow_fill_diffs": 0,
                "shadow_position_diffs": 0,
                "unexplained_ownership_diffs": 0,
                "balance_divergence": False,
                "rollback_bounces": 1,
            },
            sort_keys=True,
        ),
        encoding="utf-8",
    )

    exit_code = module.main(
        [
            "--profile",
            "equities",
            "--scope",
            "ibkr.hedge.main",
            "--single-host",
            "--check-canary-session",
            "--session-artifact",
            str(session_path),
        ]
    )

    assert exit_code == 1
