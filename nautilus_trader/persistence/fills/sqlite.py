# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import sqlite3
from typing import Any
from typing import NamedTuple

import msgspec

from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.model.enums import liquidity_side_to_str
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.enums import order_type_to_str
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.persistence._action_intent import ActionIntentRecord
from nautilus_trader.persistence._execution_timing import ExecutionTimingRecord
from nautilus_trader.persistence.fills.schema import EXECUTION_FILL_INDEXES_SQL
from nautilus_trader.persistence.fills.schema import EXECUTION_FILL_TABLE_SQL
from nautilus_trader.persistence.fills.schema import INSERT_EXECUTION_FILL_SQL


class ExecutionFillRow(NamedTuple):
    trader_id: str
    event_id: str
    strategy_id: str
    account_id: str
    instrument_id: str
    trade_id: str
    client_order_id: str
    venue_order_id: str
    position_id: str | None
    order_side: str
    order_type: str
    last_qty: str
    last_px: str
    currency: str
    commission: str
    liquidity_side: str
    ts_event: int
    ts_init: int
    reconciliation: int
    info_json: str
    run_id: str | None
    quote_cycle_id: str | None
    reason_code: str | None
    level_index: int | None
    target_px: str | None
    cancel_px: str | None
    match_tol: str | None
    ts_market_data_event_ns: int | None
    ts_market_data_recv_ns: int | None
    ts_decision_ns: int | None
    ts_submit_local_ns: int | None
    ts_command_init_ns: int | None
    ts_risk_recv_ns: int | None
    ts_risk_forward_ns: int | None
    ts_exec_recv_ns: int | None
    ts_exec_forward_ns: int | None
    ts_client_submit_ns: int | None
    ts_adapter_submit_start_ns: int | None
    ts_ingest_ns: int
    ts_submit_gateway_send_ns: int | None
    ts_cancel_gateway_send_ns: int | None
    ts_open_order_recv_ns: int | None
    ts_order_status_recv_ns: int | None
    ts_exec_details_recv_ns: int | None
    last_qty_base: str | None = None
    last_qty_venue: str | None = None
    qty_conversion_status: str | None = None
    qty_conversion_source: str | None = None


def connect(path: str) -> sqlite3.Connection:
    """
    Return a SQLite connection configured for write-heavy append workloads.
    """
    conn = sqlite3.connect(path, timeout=5.0)
    conn.execute("PRAGMA journal_mode=WAL;")
    conn.execute("PRAGMA synchronous=NORMAL;")
    return conn


def ensure_schema(conn: sqlite3.Connection) -> None:
    """
    Ensure the execution fill schema exists and backfill missing columns.
    """
    conn.executescript(EXECUTION_FILL_TABLE_SQL)
    columns = _table_columns(conn, "execution_fill")
    for statement in _alter_statements(columns):
        conn.execute(statement)
    conn.executescript(EXECUTION_FILL_INDEXES_SQL)


def _encode_info_json(
    info: dict[str, Any],
    on_info_encode_error: Any | None = None,
) -> str:
    try:
        return msgspec.json.encode(info, enc_hook=msgspec_encoding_hook).decode("utf-8")
    except Exception:
        if on_info_encode_error is not None:
            on_info_encode_error()
        return "{}"


def fill_to_row(
    fill: OrderFilled,
    *,
    info_override: dict[str, Any] | None = None,
    client_order_id_override: str | None = None,
    action_intent: ActionIntentRecord | None = None,
    execution_timing: ExecutionTimingRecord | None = None,
    ts_ingest_ns: int = 0,
    last_qty_base: str | None = None,
    last_qty_venue: str | None = None,
    qty_conversion_status: str | None = None,
    qty_conversion_source: str | None = None,
    on_info_encode_error: Any | None = None,
) -> ExecutionFillRow:
    """
    Convert an `OrderFilled` event to a primitive SQLite row.
    """
    info = fill.info if info_override is None else info_override
    info_json = _encode_info_json(info, on_info_encode_error=on_info_encode_error)
    client_order_id = fill.client_order_id.value if client_order_id_override is None else client_order_id_override
    ib_latency = _extract_ib_latency(info)
    persisted_last_qty = str(fill.last_qty)

    return ExecutionFillRow(
        fill.trader_id.value,
        fill.id.value,
        fill.strategy_id.value,
        fill.account_id.value,
        fill.instrument_id.value,
        fill.trade_id.value,
        client_order_id,
        fill.venue_order_id.value,
        fill.position_id.value if fill.position_id else None,
        order_side_to_str(fill.order_side),
        order_type_to_str(fill.order_type),
        persisted_last_qty,
        str(fill.last_px),
        fill.currency.code,
        str(fill.commission),
        liquidity_side_to_str(fill.liquidity_side),
        int(fill.ts_event),
        int(fill.ts_init),
        int(bool(fill.reconciliation)),
        info_json,
        action_intent.run_id if action_intent is not None else None,
        action_intent.quote_cycle_id if action_intent is not None else None,
        action_intent.reason_code if action_intent is not None else None,
        action_intent.level_index if action_intent is not None else None,
        action_intent.target_px if action_intent is not None else None,
        action_intent.cancel_px if action_intent is not None else None,
        action_intent.match_tol if action_intent is not None else None,
        action_intent.ts_market_data_event_ns if action_intent is not None else None,
        action_intent.ts_market_data_recv_ns if action_intent is not None else None,
        action_intent.ts_decision_ns if action_intent is not None else None,
        action_intent.ts_submit_local_ns if action_intent is not None else None,
        execution_timing.ts_command_init_ns if execution_timing is not None else None,
        execution_timing.ts_risk_recv_ns if execution_timing is not None else None,
        execution_timing.ts_risk_forward_ns if execution_timing is not None else None,
        execution_timing.ts_exec_recv_ns if execution_timing is not None else None,
        execution_timing.ts_exec_forward_ns if execution_timing is not None else None,
        execution_timing.ts_client_submit_ns if execution_timing is not None else None,
        execution_timing.ts_adapter_submit_start_ns if execution_timing is not None else None,
        int(ts_ingest_ns),
        ib_latency.get("ts_submit_gateway_send_ns"),
        ib_latency.get("ts_cancel_gateway_send_ns"),
        ib_latency.get("ts_open_order_recv_ns"),
        ib_latency.get("ts_order_status_recv_ns"),
        ib_latency.get("ts_exec_details_recv_ns"),
        last_qty_base,
        last_qty_venue,
        qty_conversion_status,
        qty_conversion_source,
    )


def insert_fills(
    conn: sqlite3.Connection,
    rows: list[ExecutionFillRow],
) -> tuple[int, int]:
    """
    Insert fill rows with idempotency (`ON CONFLICT DO NOTHING`).
    """
    if not rows:
        return (0, 0)

    with conn:
        before = conn.total_changes
        conn.executemany(INSERT_EXECUTION_FILL_SQL, rows)
        inserted = conn.total_changes - before

    return inserted, len(rows) - inserted


def _extract_ib_latency(info: dict[str, Any]) -> dict[str, int | None]:
    payload = info.get("ib_latency")
    if not isinstance(payload, dict):
        return {
            "ts_submit_gateway_send_ns": None,
            "ts_cancel_gateway_send_ns": None,
            "ts_open_order_recv_ns": None,
            "ts_order_status_recv_ns": None,
            "ts_exec_details_recv_ns": None,
        }
    return {
        "ts_submit_gateway_send_ns": _optional_int(payload.get("ts_submit_gateway_send_ns")),
        "ts_cancel_gateway_send_ns": _optional_int(payload.get("ts_cancel_gateway_send_ns")),
        "ts_open_order_recv_ns": _optional_int(payload.get("ts_open_order_recv_ns")),
        "ts_order_status_recv_ns": _optional_int(payload.get("ts_order_status_recv_ns")),
        "ts_exec_details_recv_ns": _optional_int(payload.get("ts_exec_details_recv_ns")),
    }


def _optional_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _table_columns(conn: sqlite3.Connection, table_name: str) -> set[str]:
    rows = conn.execute(f"PRAGMA table_info({table_name})").fetchall()
    return {row[1] for row in rows}


def _alter_statements(columns: set[str]) -> list[str]:
    statements: list[str] = []
    additions = {
        "run_id": "ALTER TABLE execution_fill ADD COLUMN run_id TEXT",
        "quote_cycle_id": "ALTER TABLE execution_fill ADD COLUMN quote_cycle_id TEXT",
        "reason_code": "ALTER TABLE execution_fill ADD COLUMN reason_code TEXT",
        "level_index": "ALTER TABLE execution_fill ADD COLUMN level_index INTEGER",
        "target_px": "ALTER TABLE execution_fill ADD COLUMN target_px TEXT",
        "cancel_px": "ALTER TABLE execution_fill ADD COLUMN cancel_px TEXT",
        "match_tol": "ALTER TABLE execution_fill ADD COLUMN match_tol TEXT",
        "ts_market_data_event_ns": "ALTER TABLE execution_fill ADD COLUMN ts_market_data_event_ns INTEGER",
        "ts_market_data_recv_ns": "ALTER TABLE execution_fill ADD COLUMN ts_market_data_recv_ns INTEGER",
        "ts_decision_ns": "ALTER TABLE execution_fill ADD COLUMN ts_decision_ns INTEGER",
        "ts_submit_local_ns": "ALTER TABLE execution_fill ADD COLUMN ts_submit_local_ns INTEGER",
        "ts_command_init_ns": "ALTER TABLE execution_fill ADD COLUMN ts_command_init_ns INTEGER",
        "ts_risk_recv_ns": "ALTER TABLE execution_fill ADD COLUMN ts_risk_recv_ns INTEGER",
        "ts_risk_forward_ns": "ALTER TABLE execution_fill ADD COLUMN ts_risk_forward_ns INTEGER",
        "ts_exec_recv_ns": "ALTER TABLE execution_fill ADD COLUMN ts_exec_recv_ns INTEGER",
        "ts_exec_forward_ns": "ALTER TABLE execution_fill ADD COLUMN ts_exec_forward_ns INTEGER",
        "ts_client_submit_ns": "ALTER TABLE execution_fill ADD COLUMN ts_client_submit_ns INTEGER",
        "ts_adapter_submit_start_ns": "ALTER TABLE execution_fill ADD COLUMN ts_adapter_submit_start_ns INTEGER",
        "ts_ingest_ns": "ALTER TABLE execution_fill ADD COLUMN ts_ingest_ns INTEGER NOT NULL DEFAULT 0",
        "ts_submit_gateway_send_ns": "ALTER TABLE execution_fill ADD COLUMN ts_submit_gateway_send_ns INTEGER",
        "ts_cancel_gateway_send_ns": "ALTER TABLE execution_fill ADD COLUMN ts_cancel_gateway_send_ns INTEGER",
        "ts_open_order_recv_ns": "ALTER TABLE execution_fill ADD COLUMN ts_open_order_recv_ns INTEGER",
        "ts_order_status_recv_ns": "ALTER TABLE execution_fill ADD COLUMN ts_order_status_recv_ns INTEGER",
        "ts_exec_details_recv_ns": "ALTER TABLE execution_fill ADD COLUMN ts_exec_details_recv_ns INTEGER",
        "last_qty_base": "ALTER TABLE execution_fill ADD COLUMN last_qty_base TEXT",
        "last_qty_venue": "ALTER TABLE execution_fill ADD COLUMN last_qty_venue TEXT",
        "qty_conversion_status": "ALTER TABLE execution_fill ADD COLUMN qty_conversion_status TEXT",
        "qty_conversion_source": "ALTER TABLE execution_fill ADD COLUMN qty_conversion_source TEXT",
    }
    for column_name, sql in additions.items():
        if column_name not in columns:
            statements.append(sql)
    return statements
