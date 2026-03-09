from __future__ import annotations

import logging
import socket
import sqlite3
import time
from contextlib import suppress
from dataclasses import dataclass
from datetime import UTC
from datetime import datetime
from datetime import timedelta
from pathlib import Path
from typing import Any
from typing import Protocol

from nautilus_trader.flux.persistence.balance_snapshots.schema import (
    FLUX_BALANCE_SNAPSHOT_COLUMN_NAMES,
)
from nautilus_trader.flux.persistence.balance_snapshots.schema import (
    FLUX_BALANCE_SNAPSHOT_ROW_COLUMN_NAMES,
)
from nautilus_trader.flux.persistence.portfolio_inventory_snapshots.schema import (
    PORTFOLIO_INVENTORY_SNAPSHOT_COLUMN_NAMES,
)
from nautilus_trader.flux.persistence.quote_cycles.schema import QUOTE_CYCLE_COLUMN_NAMES
from nautilus_trader.persistence.fills.schema import EXECUTION_FILL_COLUMN_NAMES
from nautilus_trader.persistence.orders.schema import ORDER_ACTION_COLUMN_NAMES
from nautilus_trader.persistence.shipper.config import TelemetryShipperConfig


class TelemetrySink(Protocol):
    def insert_rows(self, table_name: str, rows: list[dict[str, Any]]) -> int: ...


@dataclass(frozen=True, slots=True)
class TableShipResult:
    shipped: int = 0
    deduped: int = 0
    pruned: int = 0
    last_rowid: int = 0


@dataclass(frozen=True, slots=True)
class _TelemetryTableSpec:
    name: str
    source_table_name: str
    columns: tuple[str, ...]
    db_path: str


SOURCE_IDENTITY_COLUMNS: dict[str, tuple[str, ...]] = {
    "flux_balance_snapshot": ("trader_id", "snapshot_id"),
    "flux_balance_snapshot_row": ("trader_id", "snapshot_id", "row_key"),
    "execution_fill": ("trader_id", "event_id"),
    "order_action": ("trader_id", "event_id"),
    "portfolio_inventory_snapshot": ("portfolio_id", "base_currency", "snapshot_id"),
    "quote_cycle": ("trader_id", "quote_cycle_id"),
}


class SQLiteToPostgresTelemetryShipper:
    def __init__(
        self,
        *,
        config: TelemetryShipperConfig,
        sink: TelemetrySink,
        source_host: str | None = None,
        logger: logging.Logger | None = None,
    ) -> None:
        self._config = config
        self._sink = sink
        self._source_host = source_host or socket.gethostname()
        self._logger = logger or logging.getLogger("nautilus.telemetry.shipper")
        self._state_path = Path(config.state_db_path)
        self._state_path.parent.mkdir(parents=True, exist_ok=True)
        self._state_conn = sqlite3.connect(self._state_path)
        self._state_conn.execute(
            """
            CREATE TABLE IF NOT EXISTS shipper_cursor (
              table_name TEXT PRIMARY KEY,
              last_rowid INTEGER NOT NULL DEFAULT 0,
              last_identity TEXT NOT NULL DEFAULT '',
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
            )
            """,
        )
        with suppress(sqlite3.OperationalError):
            self._state_conn.execute(
                "ALTER TABLE shipper_cursor ADD COLUMN last_identity TEXT NOT NULL DEFAULT ''",
            )
        self._state_conn.commit()

    def close(self) -> None:
        self._state_conn.close()

    def configured_table_names(self) -> tuple[str, ...]:
        return tuple(spec.name for spec in self._table_specs())

    def ship_once(self) -> dict[str, TableShipResult]:
        results: dict[str, TableShipResult] = {}
        for spec in self._table_specs():
            cursor, last_identity = self._load_cursor(spec.name)
            rows, cursor = self._load_rows_with_cursor_reset(
                spec=spec,
                after_rowid=cursor,
                last_identity=last_identity,
            )
            if rows:
                payloads = [self._payload_from_row(spec.name, row) for row in rows]
                inserted = self._sink.insert_rows(spec.name, payloads)
                last_rowid = int(rows[-1]["rowid"])
                self._store_cursor(spec.name, last_rowid, _row_identity(spec.name, rows[-1]))
                pruned = self._prune_old_rows(spec=spec, shipped_through_rowid=last_rowid)
                results[spec.name] = TableShipResult(
                    shipped=inserted,
                    deduped=len(rows) - inserted,
                    pruned=pruned,
                    last_rowid=last_rowid,
                )
                continue

            pruned = self._prune_old_rows(spec=spec, shipped_through_rowid=cursor)
            results[spec.name] = TableShipResult(pruned=pruned, last_rowid=cursor)
        return results

    def run_forever(self) -> None:
        while True:
            try:
                self.ship_once()
            except KeyboardInterrupt:  # pragma: no cover
                raise
            except Exception:
                self._logger.exception("Telemetry shipper cycle failed")
            time.sleep(self._config.poll_interval_ms / 1000.0)

    def _table_specs(self) -> tuple[_TelemetryTableSpec, ...]:
        specs: list[_TelemetryTableSpec] = []
        if self._config.balance_snapshots_db_path:
            specs.extend(
                [
                    _TelemetryTableSpec(
                        name="flux_balance_snapshot",
                        source_table_name="flux_balance_snapshot",
                        columns=FLUX_BALANCE_SNAPSHOT_COLUMN_NAMES,
                        db_path=self._config.balance_snapshots_db_path,
                    ),
                    _TelemetryTableSpec(
                        name="flux_balance_snapshot_row",
                        source_table_name="flux_balance_snapshot_row",
                        columns=FLUX_BALANCE_SNAPSHOT_ROW_COLUMN_NAMES,
                        db_path=self._config.balance_snapshots_db_path,
                    ),
                ],
            )
        if self._config.fills_db_path:
            specs.append(
                _TelemetryTableSpec(
                    name="execution_fill",
                    source_table_name="execution_fill",
                    columns=EXECUTION_FILL_COLUMN_NAMES,
                    db_path=self._config.fills_db_path,
                ),
            )
        if self._config.orders_db_path:
            specs.append(
                _TelemetryTableSpec(
                    name="order_action",
                    source_table_name="order_action",
                    columns=ORDER_ACTION_COLUMN_NAMES,
                    db_path=self._config.orders_db_path,
                ),
            )
        if self._config.quote_cycles_db_path:
            specs.append(
                _TelemetryTableSpec(
                    name="quote_cycle",
                    source_table_name="quote_cycle",
                    columns=QUOTE_CYCLE_COLUMN_NAMES,
                    db_path=self._config.quote_cycles_db_path,
                ),
            )
        if self._config.portfolio_inventory_db_path:
            specs.append(
                _TelemetryTableSpec(
                    name="portfolio_inventory_snapshot",
                    source_table_name="portfolio_inventory_snapshot",
                    columns=PORTFOLIO_INVENTORY_SNAPSHOT_COLUMN_NAMES,
                    db_path=self._config.portfolio_inventory_db_path,
                ),
            )
        return tuple(specs)

    def _load_rows_with_cursor_reset(
        self,
        *,
        spec: _TelemetryTableSpec,
        after_rowid: int,
        last_identity: str,
    ) -> tuple[list[sqlite3.Row], int]:
        rows = self._load_rows(spec=spec, after_rowid=after_rowid)
        if rows:
            return rows, after_rowid
        if after_rowid <= 0:
            return rows, after_rowid

        max_rowid = self._max_rowid(spec=spec)
        if max_rowid is None or max_rowid > after_rowid:
            return rows, after_rowid
        current_identity = self._identity_at_rowid(spec=spec, rowid=max_rowid)
        if current_identity == "" or current_identity == last_identity:
            return rows, after_rowid

        self._store_cursor(spec.name, 0, "")
        self._logger.warning(
            "Resetting telemetry shipper cursor for %s after rowid restart (cursor=%s max_rowid=%s)",
            spec.name,
            after_rowid,
            max_rowid,
        )
        return self._load_rows(spec=spec, after_rowid=0), 0

    def _load_rows(self, *, spec: _TelemetryTableSpec, after_rowid: int) -> list[sqlite3.Row]:
        db_path = Path(spec.db_path)
        if not db_path.exists():
            return []

        conn = sqlite3.connect(db_path)
        conn.row_factory = sqlite3.Row
        try:
            query = f"SELECT rowid, {', '.join(spec.columns)} FROM {spec.source_table_name} WHERE rowid > ? ORDER BY rowid ASC LIMIT ?"  # noqa: S608
            return conn.execute(
                query,
                (after_rowid, self._config.max_batch_size),
            ).fetchall()
        except sqlite3.OperationalError as exc:
            raise RuntimeError(
                f"Failed reading telemetry source table `{spec.source_table_name}` from `{db_path}`",
            ) from exc
        finally:
            conn.close()

    def _max_rowid(self, *, spec: _TelemetryTableSpec) -> int | None:
        db_path = Path(spec.db_path)
        if not db_path.exists():
            return None

        conn = sqlite3.connect(db_path)
        try:
            query = f"SELECT MAX(rowid) FROM {spec.source_table_name}"  # noqa: S608
            row = conn.execute(query).fetchone()
        except sqlite3.OperationalError as exc:
            raise RuntimeError(
                f"Failed reading telemetry source max rowid for `{spec.source_table_name}` from `{db_path}`",
            ) from exc
        finally:
            conn.close()

        if row is None or row[0] is None:
            return None
        return int(row[0])

    def _identity_at_rowid(self, *, spec: _TelemetryTableSpec, rowid: int) -> str:
        db_path = Path(spec.db_path)
        if not db_path.exists():
            return ""

        conn = sqlite3.connect(db_path)
        conn.row_factory = sqlite3.Row
        try:
            query = f"SELECT rowid, {', '.join(spec.columns)} FROM {spec.source_table_name} WHERE rowid = ?"  # noqa: S608
            row = conn.execute(query, (rowid,)).fetchone()
        except sqlite3.OperationalError as exc:
            raise RuntimeError(
                f"Failed reading telemetry source row identity for `{spec.source_table_name}` from `{db_path}`",
            ) from exc
        finally:
            conn.close()

        if row is None:
            return ""
        return _row_identity(spec.name, row)

    def _payload_from_row(self, table_name: str, row: sqlite3.Row) -> dict[str, Any]:
        payload = dict(row)
        payload.pop("rowid", None)
        payload["source_profile"] = self._config.source_profile
        payload["source_host"] = self._source_host
        if table_name == "quote_cycle":
            payload["decision_context_json"] = payload.get("decision_context_json") or "null"
        return payload

    def _prune_old_rows(self, *, spec: _TelemetryTableSpec, shipped_through_rowid: int) -> int:
        if shipped_through_rowid <= 0:
            return 0

        db_path = Path(spec.db_path)
        if not db_path.exists():
            return 0

        cutoff = _utc_now_minus(hours=self._config.prune_retention_hours)
        conn = sqlite3.connect(db_path)
        try:
            with conn:
                delete_sql = f"DELETE FROM {spec.source_table_name} WHERE rowid <= ? AND created_at < ?"  # noqa: S608
                cursor = conn.execute(
                    delete_sql,
                    (shipped_through_rowid, cutoff),
                )
                return int(cursor.rowcount or 0)
        except sqlite3.OperationalError as exc:
            raise RuntimeError(
                f"Failed pruning telemetry source table `{spec.source_table_name}` from `{db_path}`",
            ) from exc
        finally:
            conn.close()

    def _load_cursor(self, table_name: str) -> tuple[int, str]:
        row = self._state_conn.execute(
            "SELECT last_rowid, last_identity FROM shipper_cursor WHERE table_name = ?",
            (table_name,),
        ).fetchone()
        if row is None:
            return (0, "")
        return (int(row[0]), str(row[1] or ""))

    def _store_cursor(self, table_name: str, last_rowid: int, last_identity: str) -> None:
        with self._state_conn:
            self._state_conn.execute(
                """
                INSERT INTO shipper_cursor (table_name, last_rowid, last_identity, updated_at)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(table_name) DO UPDATE SET
                  last_rowid = excluded.last_rowid,
                  last_identity = excluded.last_identity,
                  updated_at = excluded.updated_at
                """,
                (table_name, last_rowid, last_identity, _utc_now()),
            )


def _utc_now() -> str:
    return datetime.now(UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def _utc_now_minus(*, hours: int) -> str:
    return (
        datetime.now(UTC) - timedelta(hours=hours)
    ).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def _row_identity(table_name: str, row: sqlite3.Row) -> str:
    return "|".join(str(row[column]) for column in SOURCE_IDENTITY_COLUMNS[table_name])
