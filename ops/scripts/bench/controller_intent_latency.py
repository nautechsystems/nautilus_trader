#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import sys
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


REPO_ROOT = _repo_root()
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from flux.execution.transport import TRANSPORT_KIND
from flux.execution.transport import TRANSPORT_SCHEMA_VERSION


ADDED_OVERHEAD_P50_BUDGET_US = 100.0
ADDED_OVERHEAD_P99_BUDGET_US = 750.0
QUEUE_BACKLOG_AGE_P99_BUDGET_US = 2_000.0
DROPPED_INTENTS_BUDGET = 0


@dataclass(frozen=True, slots=True)
class OperationLatencySamples:
    direct_path_us: tuple[float, ...]
    controller_path_us: tuple[float, ...]

    def __post_init__(self) -> None:
        direct = tuple(float(value) for value in self.direct_path_us)
        controller = tuple(float(value) for value in self.controller_path_us)
        object.__setattr__(self, "direct_path_us", direct)
        object.__setattr__(self, "controller_path_us", controller)
        if not direct or not controller:
            raise ValueError("latency samples must be non-empty")
        if len(direct) != len(controller):
            raise ValueError("direct and controller latency samples must have matching counts")

    @property
    def added_overhead_us(self) -> tuple[float, ...]:
        return tuple(
            controller - direct
            for direct, controller in zip(self.direct_path_us, self.controller_path_us, strict=True)
        )


BASELINE_OPERATION_SAMPLES = {
    "submit": OperationLatencySamples(
        direct_path_us=(150, 160, 170, 180, 190),
        controller_path_us=(215, 230, 245, 260, 290),
    ),
    "cancel": OperationLatencySamples(
        direct_path_us=(90, 100, 110, 120, 130),
        controller_path_us=(150, 160, 175, 190, 205),
    ),
    "replace": OperationLatencySamples(
        direct_path_us=(180, 190, 200, 210, 220),
        controller_path_us=(255, 270, 285, 300, 360),
    ),
}

CANARY_OPERATION_SAMPLES = {
    "submit": OperationLatencySamples(
        direct_path_us=(150, 160, 170, 180, 195),
        controller_path_us=(225, 235, 250, 285, 320),
    ),
    "cancel": OperationLatencySamples(
        direct_path_us=(90, 100, 110, 120, 135),
        controller_path_us=(155, 165, 180, 210, 230),
    ),
    "replace": OperationLatencySamples(
        direct_path_us=(185, 195, 205, 215, 225),
        controller_path_us=(275, 295, 305, 330, 380),
    ),
}

SCENARIOS = {
    "baseline": {
        "operation_samples": BASELINE_OPERATION_SAMPLES,
        "queue_backlog_age_us": (200, 350, 500, 900, 1_500),
        "dropped_intents": 0,
    },
    "canary": {
        "operation_samples": CANARY_OPERATION_SAMPLES,
        "queue_backlog_age_us": (450, 700, 850, 1_250, 1_900),
        "dropped_intents": 0,
    },
}


def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Emit deterministic controller-intent latency summaries for platform-cut budget gates.",
    )
    parser.add_argument(
        "--scenario",
        default="baseline",
        choices=tuple(sorted(SCENARIOS)),
        help="Built-in benchmark scenario to summarize.",
    )
    parser.add_argument(
        "--check-budgets",
        action="store_true",
        help="Return non-zero when any latency budget is exceeded.",
    )
    return parser.parse_args(argv)


def _nearest_rank_percentile(values: tuple[float, ...], percentile: int) -> float:
    ordered = tuple(sorted(float(value) for value in values))
    if not ordered:
        raise ValueError("percentiles require at least one value")
    rank = max(1, math.ceil((percentile / 100) * len(ordered)))
    return float(ordered[min(rank - 1, len(ordered) - 1)])


def _metric_summary(values: tuple[float, ...]) -> dict[str, float | int]:
    return {
        "count": len(values),
        "p50": _nearest_rank_percentile(values, 50),
        "p99": _nearest_rank_percentile(values, 99),
    }


def build_latency_report(
    *,
    scenario: str,
    operation_samples: Mapping[str, OperationLatencySamples],
    queue_backlog_age_us: tuple[int | float, ...],
    dropped_intents: int,
) -> dict[str, object]:
    operations: dict[str, object] = {}
    for operation_name, samples in operation_samples.items():
        operations[operation_name] = {
            "direct_path_us": _metric_summary(samples.direct_path_us),
            "controller_path_us": _metric_summary(samples.controller_path_us),
            "added_overhead_us": {
                **_metric_summary(samples.added_overhead_us),
                "budget": {
                    "p50": ADDED_OVERHEAD_P50_BUDGET_US,
                    "p99": ADDED_OVERHEAD_P99_BUDGET_US,
                },
            },
        }

    queue_summary = {
        **_metric_summary(tuple(float(value) for value in queue_backlog_age_us)),
        "budget": {"p99": QUEUE_BACKLOG_AGE_P99_BUDGET_US},
    }
    dropped_summary = {
        "count": int(dropped_intents),
        "budget": {"count": DROPPED_INTENTS_BUDGET},
    }

    report = {
        "scenario": scenario,
        "transport": {
            "kind": TRANSPORT_KIND,
            "schema_version": TRANSPORT_SCHEMA_VERSION,
        },
        "operations": operations,
        "queue_backlog_age_us": queue_summary,
        "dropped_intents": dropped_summary,
    }
    violations = collect_budget_violations(report)
    report["budget_check"] = {
        "passed": not violations,
        "violations": violations,
    }
    return report


def build_report_for_scenario(scenario: str) -> dict[str, object]:
    scenario_payload = SCENARIOS[scenario]
    return build_latency_report(
        scenario=scenario,
        operation_samples=scenario_payload["operation_samples"],
        queue_backlog_age_us=scenario_payload["queue_backlog_age_us"],
        dropped_intents=scenario_payload["dropped_intents"],
    )


def collect_budget_violations(report: Mapping[str, object]) -> list[str]:
    violations: list[str] = []

    operations = report["operations"]
    assert isinstance(operations, Mapping)
    for operation_name, operation_report in operations.items():
        assert isinstance(operation_report, Mapping)
        overhead_report = operation_report["added_overhead_us"]
        assert isinstance(overhead_report, Mapping)
        p50 = float(overhead_report["p50"])
        p99 = float(overhead_report["p99"])
        if p50 > ADDED_OVERHEAD_P50_BUDGET_US:
            violations.append(
                f"{operation_name} added_overhead_us p50 {p50} exceeded budget {ADDED_OVERHEAD_P50_BUDGET_US}",
            )
        if p99 > ADDED_OVERHEAD_P99_BUDGET_US:
            violations.append(
                f"{operation_name} added_overhead_us p99 {p99} exceeded budget {ADDED_OVERHEAD_P99_BUDGET_US}",
            )

    queue_backlog_report = report["queue_backlog_age_us"]
    assert isinstance(queue_backlog_report, Mapping)
    queue_p99 = float(queue_backlog_report["p99"])
    if queue_p99 > QUEUE_BACKLOG_AGE_P99_BUDGET_US:
        violations.append(
            f"queue_backlog_age_us p99 {queue_p99} exceeded budget {QUEUE_BACKLOG_AGE_P99_BUDGET_US}",
        )

    dropped_report = report["dropped_intents"]
    assert isinstance(dropped_report, Mapping)
    dropped = int(dropped_report["count"])
    if dropped > DROPPED_INTENTS_BUDGET:
        violations.append(f"dropped_intents {dropped} exceeded budget {DROPPED_INTENTS_BUDGET}")

    return violations


def assert_latency_budgets(report: Mapping[str, object]) -> None:
    violations = collect_budget_violations(report)
    if violations:
        raise ValueError("; ".join(violations))


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)
    report = build_report_for_scenario(args.scenario)
    print(json.dumps(report, indent=2, sort_keys=True))
    if args.check_budgets:
        try:
            assert_latency_budgets(report)
        except ValueError:
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
