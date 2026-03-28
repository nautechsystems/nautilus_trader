from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path
from types import ModuleType

import pytest


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def _load_bench_module() -> ModuleType:
    path = _repo_root() / "ops/scripts/bench/controller_intent_latency.py"
    assert path.exists(), "controller latency benchmark script should exist"

    spec = importlib.util.spec_from_file_location("task2_controller_intent_latency", path)
    assert spec is not None
    assert spec.loader is not None

    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def test_baseline_benchmark_output_shape_and_thresholds_are_locked(capsys) -> None:
    module = _load_bench_module()

    exit_code = module.main(["--scenario", "baseline", "--check-budgets"])

    assert exit_code == 0
    assert json.loads(capsys.readouterr().out) == {
        "scenario": "baseline",
        "transport": {
            "kind": "uds",
            "schema_version": "v1",
        },
        "operations": {
            "submit": {
                "direct_path_us": {"count": 5, "p50": 170.0, "p99": 190.0},
                "controller_path_us": {"count": 5, "p50": 245.0, "p99": 290.0},
                "added_overhead_us": {
                    "count": 5,
                    "p50": 75.0,
                    "p99": 100.0,
                    "budget": {"p50": 100.0, "p99": 750.0},
                },
            },
            "cancel": {
                "direct_path_us": {"count": 5, "p50": 110.0, "p99": 130.0},
                "controller_path_us": {"count": 5, "p50": 175.0, "p99": 205.0},
                "added_overhead_us": {
                    "count": 5,
                    "p50": 65.0,
                    "p99": 75.0,
                    "budget": {"p50": 100.0, "p99": 750.0},
                },
            },
            "replace": {
                "direct_path_us": {"count": 5, "p50": 200.0, "p99": 220.0},
                "controller_path_us": {"count": 5, "p50": 285.0, "p99": 360.0},
                "added_overhead_us": {
                    "count": 5,
                    "p50": 85.0,
                    "p99": 140.0,
                    "budget": {"p50": 100.0, "p99": 750.0},
                },
            },
        },
        "queue_backlog_age_us": {
            "count": 5,
            "p50": 500.0,
            "p99": 1500.0,
            "budget": {"p99": 2000.0},
        },
        "dropped_intents": {
            "count": 0,
            "budget": {"count": 0},
        },
        "budget_check": {
            "passed": True,
            "violations": [],
        },
    }


def test_assert_latency_budgets_rejects_threshold_regressions() -> None:
    module = _load_bench_module()

    report = module.build_latency_report(
        scenario="regression",
        operation_samples={
            "submit": module.OperationLatencySamples(
                direct_path_us=(100, 110, 120, 130, 140),
                controller_path_us=(250, 300, 360, 950, 1_150),
            ),
            "cancel": module.OperationLatencySamples(
                direct_path_us=(90, 100, 110, 120, 130),
                controller_path_us=(160, 170, 180, 195, 210),
            ),
            "replace": module.OperationLatencySamples(
                direct_path_us=(180, 190, 200, 210, 220),
                controller_path_us=(260, 275, 290, 310, 360),
            ),
        },
        queue_backlog_age_us=(250, 500, 750, 2_500, 3_000),
        dropped_intents=1,
    )

    with pytest.raises(
        ValueError,
        match="submit added_overhead_us p50|submit added_overhead_us p99|queue_backlog_age_us p99|dropped_intents",
    ):
        module.assert_latency_budgets(report)
