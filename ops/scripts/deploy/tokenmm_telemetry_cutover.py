from __future__ import annotations

import argparse
import json
import shutil
import sqlite3
import subprocess
import time
from datetime import UTC
from datetime import datetime
from pathlib import Path

from nautilus_trader.persistence.shipper.quote_cycle_archive import archive_rotated_quote_cycle_db
from nautilus_trader.persistence.shipper.s3_archive import archive_rotated_sqlite_database


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
    parser.add_argument("--archive-staging-dir", type=Path, default=DEFAULT_TELEMETRY_DIR / "archive-staging")
    parser.add_argument("--archive-s3-bucket", default="")
    parser.add_argument("--archive-s3-prefix", default="nautilus/telemetry/tokenmm")
    parser.add_argument("--athena-database", default="nautilus_telemetry")
    parser.add_argument("--athena-workgroup", default="primary")
    parser.add_argument("--source-profile", default="tokenmm")
    parser.add_argument("--archive-quote-cycles", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--max-wait-seconds", type=int, default=900)
    parser.add_argument("--poll-seconds", type=int, default=15)
    return parser.parse_args()


def _run(cmd: list[str], *, dry_run: bool) -> None:
    print("$", " ".join(cmd))
    if not dry_run:
        subprocess.run(cmd, check=True)


def _run_capture_output(cmd: list[str], *, dry_run: bool) -> str:
    print("$", " ".join(cmd))
    if dry_run:
        return "{}"
    completed = subprocess.run(
        cmd,
        check=True,
        text=True,
        capture_output=True,
    )
    return completed.stdout


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


def _rotate_for_cutover(db_path: Path, *, dry_run: bool) -> Path:
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    rotated = db_path.with_suffix(db_path.suffix + f".cutover-{timestamp}")
    print(f"move {db_path} -> {rotated}")
    if not dry_run:
        shutil.move(str(db_path), str(rotated))
    return rotated


def _delete_rotated_db(rotated_path: Path, *, dry_run: bool) -> None:
    print(f"delete {rotated_path}")
    if not dry_run:
        rotated_path.unlink(missing_ok=True)


def _archive_rotated_db(rotated_path: Path, args: argparse.Namespace) -> None:
    if rotated_path.name.startswith("quote_cycles.sqlite"):
        if not args.archive_quote_cycles:
            return
        result = archive_rotated_quote_cycle_db(
            db_path=rotated_path,
            staging_root=args.archive_staging_dir,
            source_profile=args.source_profile,
            bucket=args.archive_s3_bucket or "dry-run-bucket",
            prefix=args.archive_s3_prefix,
            athena_database=args.athena_database,
        )
        results = result
    else:
        results = archive_rotated_sqlite_database(
            db_path=rotated_path,
            staging_root=args.archive_staging_dir,
            source_profile=args.source_profile,
            bucket=args.archive_s3_bucket or "dry-run-bucket",
            prefix=args.archive_s3_prefix,
            athena_database=args.athena_database,
        )

    for result in results:
        print(f"staged parquet {result.parquet_path} -> s3://{args.archive_s3_bucket}/{result.s3_key}")
        if not args.archive_s3_bucket:
            print("archive_s3_bucket not configured; staged locally only")
            continue
        _run(
            [
                "aws",
                "s3",
                "cp",
                str(result.parquet_path),
                f"s3://{args.archive_s3_bucket}/{result.s3_key}",
            ],
            dry_run=args.dry_run,
        )
        _run_athena_query(
            query_string=result.athena_ddl,
            args=args,
        )
        _run_athena_query(
            query_string=result.athena_partition_sql,
            args=args,
        )


def _run_athena_query(*, query_string: str, args: argparse.Namespace) -> None:
    query_id = _start_athena_query(
        query_string=query_string,
        args=args,
    )
    if not query_id:
        return
    _poll_athena_query(query_id=query_id, args=args)


def _start_athena_query(*, query_string: str, args: argparse.Namespace) -> str:
    payload = _run_capture_output(
        [
            "aws",
            "athena",
            "start-query-execution",
            "--work-group",
            args.athena_workgroup,
            "--query-string",
            query_string,
            "--result-configuration",
            (
                "OutputLocation="
                f"s3://{args.archive_s3_bucket}/{args.archive_s3_prefix.rstrip('/')}/athena-query-results/"
            ),
        ],
        dry_run=args.dry_run,
    )
    if args.dry_run:
        return ""
    try:
        data = json.loads(payload)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"Failed to parse Athena start-query-execution response: {payload}") from exc
    query_execution_id = data.get("QueryExecutionId")
    if not query_execution_id:
        raise RuntimeError(f"Missing QueryExecutionId from Athena response: {payload}")
    return str(query_execution_id)


def _poll_athena_query(*, query_id: str, args: argparse.Namespace) -> None:
    deadline = time.time() + args.max_wait_seconds
    while True:
        payload = _run_capture_output(
            [
                "aws",
                "athena",
                "get-query-execution",
                "--query-execution-id",
                query_id,
            ],
            dry_run=args.dry_run,
        )
        if args.dry_run:
            return
        status = json.loads(payload).get("QueryExecution", {}).get("Status", {})
        state = str(status.get("State", "")).upper()
        reason = status.get("StateChangeReason")
        if state == "SUCCEEDED":
            return
        if state in {"FAILED", "CANCELLED"}:
            raise RuntimeError(f"Athena query {query_id} failed with state={state} reason={reason}")
        if time.time() >= deadline:
            raise RuntimeError(f"Athena query {query_id} did not finish before timeout (state={state})")
        time.sleep(args.poll_seconds)


def main() -> int:
    args = _parse_args()
    _run(["sudo", "systemctl", "start", "flux@tokenmm-telemetry-shipper.service"], dry_run=args.dry_run)
    if args.wait_for_catchup:
        _wait_for_catchup(args)

    _run(["sudo", "systemctl", "stop", "flux-tokenmm.target"], dry_run=args.dry_run)
    _run(["sudo", "systemctl", "stop", "flux@tokenmm-telemetry-shipper.service"], dry_run=args.dry_run)

    for db_path in _source_db_paths(args.telemetry_dir):
        rotated = _rotate_for_cutover(db_path, dry_run=args.dry_run)
        if db_path.name != "shipper_state.sqlite":
            _archive_rotated_db(rotated, args)
        if args.delete_local_after_cutover:
            _delete_rotated_db(rotated, dry_run=args.dry_run)

    _run(["sudo", "systemctl", "start", "flux-tokenmm.target"], dry_run=args.dry_run)
    _run(["sudo", "systemctl", "start", "flux@tokenmm-telemetry-shipper.service"], dry_run=args.dry_run)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
