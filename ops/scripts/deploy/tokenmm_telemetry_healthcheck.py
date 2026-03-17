from __future__ import annotations

import argparse
import json
import shutil
import sqlite3
from dataclasses import asdict
from dataclasses import dataclass
from datetime import UTC
from datetime import datetime
from pathlib import Path


@dataclass(frozen=True)
class HealthState:
    root_usage_pct: float
    telemetry_dir_gb: float
    shipper_lag_minutes: float


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate TokenMM telemetry disk guardrails.")
    parser.add_argument(
        "--telemetry-dir",
        type=Path,
        default=Path("/var/lib/nautilus/telemetry/tokenmm"),
    )
    parser.add_argument(
        "--state-db-path",
        type=Path,
        default=Path("/var/lib/nautilus/telemetry/tokenmm/shipper_state.sqlite"),
    )
    parser.add_argument("--root-path", type=Path, default=Path("/"))
    parser.add_argument("--max-telemetry-dir-gb", type=float, required=True)
    parser.add_argument("--max-root-usage-pct", type=float, required=True)
    parser.add_argument("--max-shipper-lag-minutes", type=float, required=True)
    return parser.parse_args()


def _root_usage_pct(path: Path) -> float:
    usage = shutil.disk_usage(path)
    return (usage.used / usage.total) * 100.0


def _dir_size_gb(path: Path) -> float:
    if not path.exists():
        return 0.0
    total = 0
    for candidate in path.rglob("*"):
        if candidate.is_file():
            total += candidate.stat().st_size
    return total / (1024**3)


def _shipper_lag_minutes(path: Path) -> float:
    if not path.exists():
        return 0.0

    conn = sqlite3.connect(path)
    try:
        row = conn.execute("SELECT MIN(updated_at), MAX(updated_at) FROM shipper_cursor").fetchone()
    finally:
        conn.close()

    if row is None or row[0] is None:
        return 0.0

    updated_at = str(row[0] or row[1])
    ts = datetime.fromisoformat(updated_at.replace("Z", "+00:00"))
    return max(0.0, (datetime.now(UTC) - ts).total_seconds() / 60.0)


def main() -> int:
    args = _parse_args()
    state = HealthState(
        root_usage_pct=_root_usage_pct(args.root_path),
        telemetry_dir_gb=_dir_size_gb(args.telemetry_dir),
        shipper_lag_minutes=_shipper_lag_minutes(args.state_db_path),
    )

    failures: list[str] = []
    if state.telemetry_dir_gb > args.max_telemetry_dir_gb:
        failures.append(
            f"telemetry dir {args.telemetry_dir} is {state.telemetry_dir_gb:.2f}GiB > {args.max_telemetry_dir_gb:.2f}GiB",
        )
    if state.root_usage_pct > args.max_root_usage_pct:
        failures.append(
            f"root usage is {state.root_usage_pct:.2f}% > {args.max_root_usage_pct:.2f}%",
        )
    if state.shipper_lag_minutes > args.max_shipper_lag_minutes:
        failures.append(
            f"shipper lag from shipper_state.sqlite is {state.shipper_lag_minutes:.2f}m > {args.max_shipper_lag_minutes:.2f}m",
        )

    print(json.dumps(asdict(state), sort_keys=True))
    if failures:
        for failure in failures:
            print(failure)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
