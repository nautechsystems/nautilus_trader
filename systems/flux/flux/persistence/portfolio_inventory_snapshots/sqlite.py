from __future__ import annotations

import hashlib
import json
import sqlite3
import time
from collections.abc import Mapping
from datetime import UTC
from datetime import datetime
from typing import Any

from flux.persistence.portfolio_inventory_snapshots.schema import (
    INSERT_PORTFOLIO_INVENTORY_SNAPSHOT_SQL,
)
from flux.persistence.portfolio_inventory_snapshots.schema import (
    PORTFOLIO_INVENTORY_SNAPSHOT_SCHEMA_SQL,
)


def connect(path: str) -> sqlite3.Connection:
    conn = sqlite3.connect(path, timeout=5.0)
    conn.execute("PRAGMA journal_mode=WAL;")
    conn.execute("PRAGMA synchronous=NORMAL;")
    return conn


def ensure_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(PORTFOLIO_INVENTORY_SNAPSHOT_SCHEMA_SQL)
    columns = _table_columns(conn)
    for statement in _alter_statements(columns):
        conn.execute(statement)


class PortfolioInventorySnapshotWriter:
    def __init__(self, *, db_path: str, unchanged_heartbeat_ms: int = 60_000) -> None:
        self._conn = connect(db_path)
        ensure_schema(self._conn)
        self._unchanged_heartbeat_ms = max(1, int(unchanged_heartbeat_ms))
        self._last_hash_by_key: dict[tuple[str, str], str] = {}
        self._last_ts_ms_by_key: dict[tuple[str, str], int] = {}

    def close(self) -> None:
        self._conn.close()

    def maybe_persist(self, *, payload: Mapping[str, Any], ts_ms: int) -> bool:
        portfolio_id = _text(payload.get("portfolio_id"))
        base_currency = _upper_text(payload.get("base_currency"))
        if portfolio_id is None or base_currency is None:
            return False

        canonical_payload = {
            "portfolio_id": portfolio_id,
            "base_currency": base_currency,
            "global_qty_base": _text(payload.get("global_qty_base")),
            "global_qty": _text(payload.get("global_qty")),
            "aggregation_mode": _text(payload.get("aggregation_mode")) or "strict",
            "global_qty_base_complete": bool(payload.get("global_qty_base_complete", False)),
            "global_qty_complete": bool(payload.get("global_qty_complete", False)),
            "degraded": bool(payload.get("degraded", False)),
            "missing_required": _as_list(payload.get("missing_required")),
            "stale_required": _as_list(payload.get("stale_required")),
            "null_qty_required": _as_list(payload.get("null_qty_required")),
            "components": _as_list(payload.get("components")),
            "usable_component_count": _int(payload.get("usable_component_count"), default=0),
            "expected_component_count": _int(payload.get("expected_component_count"), default=0),
            "stale_after_ms": _int(payload.get("stale_after_ms"), default=0),
        }
        canonical_json = _canonical_json(canonical_payload)
        snapshot_hash = hashlib.sha256(canonical_json.encode("ascii", errors="ignore")).hexdigest()
        snapshot_id = hashlib.sha256(
            f"{portfolio_id}\x1f{base_currency}\x1f{ts_ms}\x1f{canonical_json}".encode(
                "ascii",
                errors="ignore",
            ),
        ).hexdigest()
        key = (portfolio_id, base_currency)
        last_hash = self._last_hash_by_key.get(key)
        last_ts_ms = self._last_ts_ms_by_key.get(key)
        unchanged = last_hash == snapshot_hash
        heartbeat_due = last_ts_ms is None or ts_ms - last_ts_ms >= self._unchanged_heartbeat_ms
        if unchanged and not heartbeat_due:
            return False

        created_at = _utc_now()
        ts_ingest_ns = time.time_ns()
        with self._conn:
            self._conn.execute(
                INSERT_PORTFOLIO_INVENTORY_SNAPSHOT_SQL,
                (
                    portfolio_id,
                    base_currency,
                    snapshot_id,
                    snapshot_hash,
                    _text(payload.get("global_qty_base")),
                    _text(payload.get("global_qty")),
                    _text(payload.get("aggregation_mode")) or "strict",
                    int(bool(payload.get("global_qty_base_complete", False))),
                    int(bool(payload.get("global_qty_complete", False))),
                    int(bool(payload.get("degraded", False))),
                    _canonical_json(_as_list(payload.get("missing_required"))),
                    _canonical_json(_as_list(payload.get("stale_required"))),
                    _canonical_json(_as_list(payload.get("null_qty_required"))),
                    _canonical_json(_as_list(payload.get("components"))),
                    _int(payload.get("usable_component_count"), default=0),
                    _int(payload.get("expected_component_count"), default=0),
                    _int(payload.get("stale_after_ms"), default=0),
                    int(ts_ms),
                    ts_ingest_ns,
                    created_at,
                ),
            )

        self._last_hash_by_key[key] = snapshot_hash
        self._last_ts_ms_by_key[key] = int(ts_ms)
        return True


def _canonical_json(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def _utc_now() -> str:
    return datetime.now(UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def _text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _upper_text(value: Any) -> str | None:
    text = _text(value)
    return text.upper() if text is not None else None


def _as_list(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        return list(value)
    return [value]


def _int(value: Any, *, default: int) -> int:
    if value is None:
        return default
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def _table_columns(conn: sqlite3.Connection) -> set[str]:
    rows = conn.execute("PRAGMA table_info(portfolio_inventory_snapshot)").fetchall()
    return {row[1] for row in rows}


def _alter_statements(columns: set[str]) -> list[str]:
    additions = {
        "global_qty_base": "ALTER TABLE portfolio_inventory_snapshot ADD COLUMN global_qty_base TEXT",
        "aggregation_mode": (
            "ALTER TABLE portfolio_inventory_snapshot "
            "ADD COLUMN aggregation_mode TEXT NOT NULL DEFAULT 'strict'"
        ),
        "global_qty_base_complete": (
            "ALTER TABLE portfolio_inventory_snapshot "
            "ADD COLUMN global_qty_base_complete INTEGER NOT NULL DEFAULT 0"
        ),
        "global_qty_complete": (
            "ALTER TABLE portfolio_inventory_snapshot "
            "ADD COLUMN global_qty_complete INTEGER NOT NULL DEFAULT 0"
        ),
        "stale_required_json": (
            "ALTER TABLE portfolio_inventory_snapshot "
            "ADD COLUMN stale_required_json TEXT NOT NULL DEFAULT '[]'"
        ),
        "null_qty_required_json": (
            "ALTER TABLE portfolio_inventory_snapshot "
            "ADD COLUMN null_qty_required_json TEXT NOT NULL DEFAULT '[]'"
        ),
        "usable_component_count": (
            "ALTER TABLE portfolio_inventory_snapshot "
            "ADD COLUMN usable_component_count INTEGER NOT NULL DEFAULT 0"
        ),
        "expected_component_count": (
            "ALTER TABLE portfolio_inventory_snapshot "
            "ADD COLUMN expected_component_count INTEGER NOT NULL DEFAULT 0"
        ),
        "stale_after_ms": (
            "ALTER TABLE portfolio_inventory_snapshot "
            "ADD COLUMN stale_after_ms INTEGER NOT NULL DEFAULT 0"
        ),
    }
    return [sql for column_name, sql in additions.items() if column_name not in columns]
