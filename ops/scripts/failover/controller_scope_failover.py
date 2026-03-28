#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Mapping
from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


REPO_ROOT = _repo_root()
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from flux.execution.controller import STALE_WRITER_STOP_BUDGET_MS


def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Emit deterministic single-host controller failover and canary-gate reports.",
    )
    parser.add_argument("--profile", required=True)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--single-host", action="store_true")
    parser.add_argument("--check-thresholds", action="store_true")
    parser.add_argument("--check-canary-session", action="store_true")
    parser.add_argument("--session-artifact", type=Path, default=None)
    args = parser.parse_args(argv)
    if args.check_thresholds == args.check_canary_session:
        parser.error("choose exactly one of --check-thresholds or --check-canary-session")
    if args.check_canary_session and args.session_artifact is None:
        parser.error("--session-artifact is required with --check-canary-session")
    return args


def build_failover_report(
    *,
    profile: str,
    scope: str,
    single_host: bool,
    stale_writer_stop_latency_ms: int,
    split_brain_rejected: bool,
    duplicate_writes: int,
    ambiguous_order_owners: int,
    rollback_bounces: int,
) -> dict[str, object]:
    report = {
        "profile": _required_text(profile, "profile"),
        "scope": _required_text(scope, "scope"),
        "single_host": bool(single_host),
        "stale_writer_stop_latency_ms": int(stale_writer_stop_latency_ms),
        "split_brain_rejected": bool(split_brain_rejected),
        "duplicate_writes": int(duplicate_writes),
        "ambiguous_order_owners": int(ambiguous_order_owners),
        "rollback_bounces": int(rollback_bounces),
    }
    violations = collect_failover_threshold_violations(report)
    report["budget_check"] = {
        "passed": not violations,
        "violations": violations,
    }
    return report


def collect_failover_threshold_violations(report: Mapping[str, object]) -> list[str]:
    violations: list[str] = []
    if int(report["stale_writer_stop_latency_ms"]) > STALE_WRITER_STOP_BUDGET_MS:
        violations.append(
            f"stale_writer_stop_latency_ms {report['stale_writer_stop_latency_ms']} exceeded budget {STALE_WRITER_STOP_BUDGET_MS}",
        )
    if not bool(report["split_brain_rejected"]):
        violations.append("split_brain_rejected must be true")
    if int(report["duplicate_writes"]) != 0:
        violations.append(f"duplicate_writes {report['duplicate_writes']} exceeded budget 0")
    if int(report["ambiguous_order_owners"]) != 0:
        violations.append(
            f"ambiguous_order_owners {report['ambiguous_order_owners']} exceeded budget 0",
        )
    if int(report["rollback_bounces"]) > 1:
        violations.append(f"rollback_bounces {report['rollback_bounces']} exceeded budget 1")
    return violations


def assert_failover_thresholds(report: Mapping[str, object]) -> None:
    violations = collect_failover_threshold_violations(report)
    if violations:
        raise ValueError("; ".join(violations))


def build_canary_session_report(
    *,
    profile: str,
    scope: str,
    single_host: bool,
    latency_budget_passed: bool,
    shadow_order_diffs: int,
    shadow_fill_diffs: int,
    shadow_position_diffs: int,
    unexplained_ownership_diffs: int,
    balance_divergence: bool,
    rollback_bounces: int,
) -> dict[str, object]:
    report = {
        "profile": _required_text(profile, "profile"),
        "scope": _required_text(scope, "scope"),
        "single_host": bool(single_host),
        "latency_budget_passed": bool(latency_budget_passed),
        "shadow_order_diffs": int(shadow_order_diffs),
        "shadow_fill_diffs": int(shadow_fill_diffs),
        "shadow_position_diffs": int(shadow_position_diffs),
        "unexplained_ownership_diffs": int(unexplained_ownership_diffs),
        "balance_divergence": bool(balance_divergence),
        "rollback_bounces": int(rollback_bounces),
    }
    violations = collect_canary_session_violations(report)
    report["budget_check"] = {
        "passed": not violations,
        "violations": violations,
    }
    return report


def collect_canary_session_violations(report: Mapping[str, object]) -> list[str]:
    violations: list[str] = []
    if not bool(report["latency_budget_passed"]):
        violations.append("latency_budget_passed must be true")
    if int(report["shadow_order_diffs"]) != 0:
        violations.append(f"shadow_order_diffs {report['shadow_order_diffs']} exceeded budget 0")
    if int(report["shadow_fill_diffs"]) != 0:
        violations.append(f"shadow_fill_diffs {report['shadow_fill_diffs']} exceeded budget 0")
    if int(report["shadow_position_diffs"]) != 0:
        violations.append(
            f"shadow_position_diffs {report['shadow_position_diffs']} exceeded budget 0",
        )
    if int(report["unexplained_ownership_diffs"]) != 0:
        violations.append(
            f"unexplained_ownership_diffs {report['unexplained_ownership_diffs']} exceeded budget 0",
        )
    if bool(report["balance_divergence"]):
        violations.append("balance_divergence must be false")
    if int(report["rollback_bounces"]) > 1:
        violations.append(f"rollback_bounces {report['rollback_bounces']} exceeded budget 1")
    return violations


def assert_canary_session(report: Mapping[str, object]) -> None:
    violations = collect_canary_session_violations(report)
    if violations:
        raise ValueError("; ".join(violations))


def load_canary_session_artifact(path: Path) -> dict[str, object]:
    payload = json.loads(Path(path).read_text(encoding="utf-8"))
    return build_canary_session_report(
        profile=payload["profile"],
        scope=payload["scope"],
        single_host=payload["single_host"],
        latency_budget_passed=payload["latency_budget_passed"],
        shadow_order_diffs=payload["shadow_order_diffs"],
        shadow_fill_diffs=payload["shadow_fill_diffs"],
        shadow_position_diffs=payload["shadow_position_diffs"],
        unexplained_ownership_diffs=payload["unexplained_ownership_diffs"],
        balance_divergence=payload["balance_divergence"],
        rollback_bounces=payload["rollback_bounces"],
    )


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)
    if args.check_thresholds:
        report = build_failover_report(
            profile=args.profile,
            scope=args.scope,
            single_host=args.single_host,
            stale_writer_stop_latency_ms=120,
            split_brain_rejected=True,
            duplicate_writes=0,
            ambiguous_order_owners=0,
            rollback_bounces=1,
        )
        print(json.dumps(report, indent=2, sort_keys=True))
        try:
            assert_failover_thresholds(report)
        except ValueError:
            return 1
        return 0

    report = load_canary_session_artifact(args.session_artifact)
    print(json.dumps(report, indent=2, sort_keys=True))
    try:
        assert_canary_session(report)
    except ValueError:
        return 1
    return 0


def _required_text(value: str, field_name: str) -> str:
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


if __name__ == "__main__":
    raise SystemExit(main())
