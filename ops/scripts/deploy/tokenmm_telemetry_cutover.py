from __future__ import annotations

import argparse
import shutil
import sqlite3
import subprocess
import time
from datetime import UTC
from datetime import datetime
from pathlib import Path


DEFAULT_TELEMETRY_DIR = Path("/var/lib/nautilus/telemetry/tokenmm")
SOURCE_FILES = (
    "balance_snapshots.sqlite",
    "fills.sqlite",
    "markouts.sqlite",
    "orders.sqlite",
    "portfolio_inventory.sqlite",
    "quote_cycles.sqlite",
    "shipper_state.sqlite",
)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Backfill TokenMM telemetry and rotate local SQLite.")
    parser.add_argument("--telemetry-dir", type=Path, default=DEFAULT_TELEMETRY_DIR)
    parser.add_argument("--state-db-path", type=Path, default=DEFAULT_TELEMETRY_DIR / "shipper_state.sqlite")
    parser.add_argument("--wait-for-catchup", action="store_true")
    parser.add_argument("--delete-local-after-cutover", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--max-wait-seconds", type=int, default=900)
    parser.add_argument("--poll-seconds", type=int, default=15)
    return parser.parse_args()


def _run(cmd: list[str], *, dry_run: bool) -> None:
    print("$", " ".join(cmd))
    if not dry_run:
        subprocess.run(cmd, check=True)


def _source_db_paths(telemetry_dir: Path) -> list[Path]:
    return [telemetry_dir / name for name in SOURCE_FILES if (telemetry_dir / name).exists()]


def _read_cursor_map(state_db: Path) -> dict[str, int]:
    if not state_db.exists():
        return {}
    conn = sqlite3.connect(state_db)
    try:
        rows = conn.execute("SELECT table_name, last_rowid FROM shipper_cursor").fetchall()
    finally:
        conn.close()
    return {str(table): int(last_rowid) for table, last_rowid in rows}


def _max_rowid(path: Path, table_name: str) -> int:
    conn = sqlite3.connect(path)
    try:
        row = conn.execute(f"SELECT COALESCE(MAX(rowid), 0) FROM {table_name}").fetchone()
    finally:
        conn.close()
    return int(row[0] if row else 0)


def _pending_rows(telemetry_dir: Path, state_db: Path) -> int:
    cursor_map = _read_cursor_map(state_db)
    table_by_file = {
        "balance_snapshots.sqlite": ("flux_balance_snapshot", "flux_balance_snapshot_row"),
        "fills.sqlite": ("execution_fill",),
        "markouts.sqlite": (),
        "orders.sqlite": ("order_action",),
        "portfolio_inventory.sqlite": ("portfolio_inventory_snapshot",),
        "quote_cycles.sqlite": ("quote_cycle",),
    }
    pending = 0
    for filename, tables in table_by_file.items():
        db_path = telemetry_dir / filename
        if not db_path.exists():
            continue
        for table in tables:
            pending += max(0, _max_rowid(db_path, table) - cursor_map.get(table, 0))
    return pending


def _wait_for_catchup(args: argparse.Namespace) -> None:
    deadline = time.time() + args.max_wait_seconds
    while True:
        pending = _pending_rows(args.telemetry_dir, args.state_db_path)
        print(f"pending_rows={pending}")
        if pending == 0:
            return
        if time.time() >= deadline:
            raise RuntimeError("tokenmm-telemetry-shipper did not catch up before timeout")
        time.sleep(args.poll_seconds)


def _rotate_or_delete(db_path: Path, *, delete_local_after_cutover: bool, dry_run: bool) -> None:
    if delete_local_after_cutover:
        print(f"delete {db_path}")
        if not dry_run:
            db_path.unlink(missing_ok=True)
        return

    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    rotated = db_path.with_suffix(db_path.suffix + f".cutover-{timestamp}")
    print(f"move {db_path} -> {rotated}")
    if not dry_run:
        shutil.move(str(db_path), str(rotated))


def main() -> int:
    args = _parse_args()
    _run(["sudo", "systemctl", "start", "flux@tokenmm-telemetry-shipper.service"], dry_run=args.dry_run)
    if args.wait_for_catchup:
        _wait_for_catchup(args)

    _run(["sudo", "systemctl", "stop", "flux-tokenmm.target"], dry_run=args.dry_run)
    _run(["sudo", "systemctl", "stop", "flux@tokenmm-telemetry-shipper.service"], dry_run=args.dry_run)

    for db_path in _source_db_paths(args.telemetry_dir):
        _rotate_or_delete(
            db_path,
            delete_local_after_cutover=args.delete_local_after_cutover,
            dry_run=args.dry_run,
        )

    _run(["sudo", "systemctl", "start", "flux-tokenmm.target"], dry_run=args.dry_run)
    _run(["sudo", "systemctl", "start", "flux@tokenmm-telemetry-shipper.service"], dry_run=args.dry_run)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
