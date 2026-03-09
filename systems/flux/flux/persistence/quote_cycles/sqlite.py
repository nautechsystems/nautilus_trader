from __future__ import annotations

import sqlite3
from typing import Any
from typing import NamedTuple

from nautilus_trader.persistence._action_intent import DECISION_CONTEXT_JSON_DEFAULT_LITERAL
from nautilus_trader.persistence._action_intent import encode_json_literal

from flux.persistence.quote_cycles.schema import INSERT_QUOTE_CYCLE_SQL
from flux.persistence.quote_cycles.schema import QUOTE_CYCLE_SCHEMA_SQL


class QuoteCycleRow(NamedTuple):
    trader_id: str
    strategy_id: str
    instrument_id: str
    run_id: str
    quote_cycle_id: str
    quote_cycle_seq: int
    quote_cycle_event: str
    reason_code: str
    trigger_source: str | None
    trigger_instrument_id: str | None
    trigger_md_ts_event_ns: int | None
    trigger_md_ts_init_ns: int | None
    ts_cycle_start_ns: int | None
    ts_cycle_end_ns: int | None
    state_from: str | None
    state_to: str | None
    cancel_count: int | None
    place_count: int | None
    bid_levels: int | None
    ask_levels: int | None
    decision_context_json: str


def connect(path: str) -> sqlite3.Connection:
    conn = sqlite3.connect(path, timeout=5.0)
    conn.execute("PRAGMA journal_mode=WAL;")
    conn.execute("PRAGMA synchronous=NORMAL;")
    return conn


def ensure_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(QUOTE_CYCLE_SCHEMA_SQL)


def quote_cycle_to_row(data: dict[str, Any], *, trader_id: str) -> QuoteCycleRow | None:
    if data.get("event") != "quote_cycle":
        return None

    quote_cycle_id = _required_text(data.get("quote_cycle_id"))
    run_id = _required_text(data.get("run_id"))
    strategy_id = _required_text(data.get("strategy_id"))
    instrument_id = _required_text(data.get("instrument_id"))
    quote_cycle_event = _required_text(data.get("quote_cycle_event"))
    reason_code = _required_text(data.get("reason_code"))
    quote_cycle_seq = _required_int(data.get("quote_cycle_seq"))
    if not all((quote_cycle_id, run_id, strategy_id, instrument_id, quote_cycle_event, reason_code)):
        return None
    if quote_cycle_seq is None:
        return None

    return QuoteCycleRow(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=instrument_id,
        run_id=run_id,
        quote_cycle_id=quote_cycle_id,
        quote_cycle_seq=quote_cycle_seq,
        quote_cycle_event=quote_cycle_event,
        reason_code=reason_code,
        trigger_source=_optional_text(data.get("trigger_source")),
        trigger_instrument_id=_optional_text(data.get("trigger_instrument_id")),
        trigger_md_ts_event_ns=_optional_int(data.get("trigger_md_ts_event_ns")),
        trigger_md_ts_init_ns=_optional_int(data.get("trigger_md_ts_init_ns")),
        ts_cycle_start_ns=_optional_int(data.get("ts_cycle_start_ns")),
        ts_cycle_end_ns=_optional_int(data.get("ts_cycle_end_ns")),
        state_from=_optional_text(data.get("from_state")),
        state_to=_optional_text(data.get("to_state")),
        cancel_count=_optional_int(data.get("cancel_count")),
        place_count=_optional_int(data.get("place_count")),
        bid_levels=_optional_int(data.get("bid_levels")),
        ask_levels=_optional_int(data.get("ask_levels")),
        decision_context_json=encode_json_literal(
            data.get("decision_context_json"),
            fallback=DECISION_CONTEXT_JSON_DEFAULT_LITERAL,
        ),
    )


def insert_many(
    conn: sqlite3.Connection,
    rows: list[QuoteCycleRow],
) -> tuple[int, int]:
    if not rows:
        return (0, 0)

    with conn:
        before = conn.total_changes
        conn.executemany(INSERT_QUOTE_CYCLE_SQL, rows)
        inserted = conn.total_changes - before

    return inserted, len(rows) - inserted


def _required_text(value: Any) -> str | None:
    text = _optional_text(value)
    return text if text is not None else None


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _required_int(value: Any) -> int | None:
    return _optional_int(value)


def _optional_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None

