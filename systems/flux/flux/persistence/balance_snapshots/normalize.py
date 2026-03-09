from __future__ import annotations

import hashlib
import json
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any


def _utc_now() -> str:
    from datetime import UTC
    from datetime import datetime

    return datetime.now(UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z")


@dataclass(frozen=True, slots=True)
class FluxBalanceSnapshotHeader:
    trader_id: str
    strategy_id: str
    snapshot_id: str
    topic: str
    snapshot_hash: str
    ts_event_ns: int | None
    ts_ms: int
    ts_ingest_ns: int
    account_count: int
    position_count: int
    payload_json: str
    created_at: str


@dataclass(frozen=True, slots=True)
class FluxBalanceSnapshotRow:
    trader_id: str
    strategy_id: str
    snapshot_id: str
    row_key: str
    kind: str
    exchange: str | None
    account_id: str | None
    account: str | None
    asset: str | None
    instrument_id: str | None
    side: str | None
    signed_qty: str | None
    quantity: str | None
    free: str | None
    locked: str | None
    total: str | None
    avg_px_open: str | None
    avg_px_close: str | None
    realized_pnl: str | None
    ts_ms: int
    row_json: str
    created_at: str


@dataclass(frozen=True, slots=True)
class FluxBalanceSnapshotRecord:
    snapshot: FluxBalanceSnapshotHeader
    rows: tuple[FluxBalanceSnapshotRow, ...]


def normalize_balance_snapshot(
    *,
    trader_id: str,
    topic: str,
    payload: Mapping[str, Any],
    ts_ingest_ns: int,
) -> FluxBalanceSnapshotRecord | None:
    strategy_id = _text(payload.get("strategy_id"))
    if strategy_id is None:
        return None

    payload_json = _canonical_json(payload)
    snapshot_hash = hashlib.sha256(
        f"{strategy_id}\x1f{topic}\x1f{payload_json}".encode("ascii", errors="ignore"),
    ).hexdigest()
    ts_event_ns = _optional_int(payload.get("ts_event"))
    ts_ms = _required_ts_ms(payload.get("ts_ms"), ts_event_ns)
    snapshot_id = hashlib.sha256(
        f"{strategy_id}\x1f{topic}\x1f{ts_ms}\x1f{payload_json}".encode("ascii", errors="ignore"),
    ).hexdigest()
    created_at = _utc_now()

    rows = [
        *_cash_rows(
            trader_id=trader_id,
            strategy_id=strategy_id,
            snapshot_id=snapshot_id,
            payload=payload,
            ts_ms=ts_ms,
            created_at=created_at,
        ),
        *_position_rows(
            trader_id=trader_id,
            strategy_id=strategy_id,
            snapshot_id=snapshot_id,
            payload=payload,
            ts_ms=ts_ms,
            created_at=created_at,
        ),
    ]

    if not rows:
        rows = _fallback_rows(
            trader_id=trader_id,
            strategy_id=strategy_id,
            snapshot_id=snapshot_id,
            payload=payload,
            ts_ms=ts_ms,
            created_at=created_at,
        )

    return FluxBalanceSnapshotRecord(
        snapshot=FluxBalanceSnapshotHeader(
            trader_id=trader_id,
            strategy_id=strategy_id,
            snapshot_id=snapshot_id,
            topic=topic,
            snapshot_hash=snapshot_hash,
            ts_event_ns=ts_event_ns,
            ts_ms=ts_ms,
            ts_ingest_ns=ts_ingest_ns,
            account_count=len(payload.get("accounts") or []),
            position_count=len(payload.get("positions") or []),
            payload_json=payload_json,
            created_at=created_at,
        ),
        rows=tuple(rows),
    )


def _cash_rows(
    *,
    trader_id: str,
    strategy_id: str,
    snapshot_id: str,
    payload: Mapping[str, Any],
    ts_ms: int,
    created_at: str,
) -> list[FluxBalanceSnapshotRow]:
    rows: list[FluxBalanceSnapshotRow] = []
    accounts = payload.get("accounts")
    if not isinstance(accounts, list):
        return rows

    for account in accounts:
        rows.extend(
            _cash_rows_for_account(
                trader_id=trader_id,
                strategy_id=strategy_id,
                snapshot_id=snapshot_id,
                account=account,
                ts_ms=ts_ms,
                created_at=created_at,
            ),
        )
    return rows


def _cash_rows_for_account(
    *,
    trader_id: str,
    strategy_id: str,
    snapshot_id: str,
    account: Mapping[str, Any],
    ts_ms: int,
    created_at: str,
) -> list[FluxBalanceSnapshotRow]:
    account_id = _text(account.get("account_id"))
    events = account.get("events")
    if not isinstance(events, list):
        return []

    rows: list[FluxBalanceSnapshotRow] = []
    for event in events:
        if not isinstance(event, Mapping):
            continue
        rows.extend(
            _cash_rows_for_event(
                trader_id=trader_id,
                strategy_id=strategy_id,
                snapshot_id=snapshot_id,
                event=event,
                account_id=account_id,
                ts_ms=ts_ms,
                created_at=created_at,
            ),
        )
    return rows


def _cash_rows_for_event(
    *,
    trader_id: str,
    strategy_id: str,
    snapshot_id: str,
    event: Mapping[str, Any],
    account_id: str | None,
    ts_ms: int,
    created_at: str,
) -> list[FluxBalanceSnapshotRow]:
    event_account_id = _text(event.get("account_id")) or account_id
    exchange = _exchange_from_account_id(event_account_id)
    balances = event.get("balances")
    if not isinstance(balances, list):
        return []

    rows: list[FluxBalanceSnapshotRow] = []
    for balance in balances:
        if not isinstance(balance, Mapping):
            continue
        row = _cash_row_from_balance(
            trader_id=trader_id,
            strategy_id=strategy_id,
            snapshot_id=snapshot_id,
            event_account_id=event_account_id,
            exchange=exchange,
            balance=balance,
            ts_ms=ts_ms,
            created_at=created_at,
        )
        if row is not None:
            rows.append(row)
    return rows


def _cash_row_from_balance(
    *,
    trader_id: str,
    strategy_id: str,
    snapshot_id: str,
    event_account_id: str | None,
    exchange: str | None,
    balance: Mapping[str, Any],
    ts_ms: int,
    created_at: str,
) -> FluxBalanceSnapshotRow | None:
    asset = _upper_text(balance.get("currency") or balance.get("asset") or balance.get("coin"))
    if asset is None:
        return None

    free = _text(balance.get("free"))
    locked = _text(balance.get("locked"))
    total = _text(balance.get("total"))
    row_payload = {
        "account_id": event_account_id,
        "exchange": exchange,
        "asset": asset,
        "free": free,
        "locked": locked,
        "total": total,
        "ts_ms": ts_ms,
    }
    account_text = event_account_id.lower() if event_account_id is not None else None
    row_key_account = event_account_id or "default"
    return FluxBalanceSnapshotRow(
        trader_id=trader_id,
        strategy_id=strategy_id,
        snapshot_id=snapshot_id,
        row_key=f"{exchange or 'unknown'}:{row_key_account}:{asset}",
        kind="cash",
        exchange=exchange,
        account_id=event_account_id,
        account=account_text,
        asset=asset,
        instrument_id=None,
        side=None,
        signed_qty=None,
        quantity=None,
        free=free,
        locked=locked,
        total=total,
        avg_px_open=None,
        avg_px_close=None,
        realized_pnl=None,
        ts_ms=ts_ms,
        row_json=_canonical_json(row_payload),
        created_at=created_at,
    )


def _position_rows(
    *,
    trader_id: str,
    strategy_id: str,
    snapshot_id: str,
    payload: Mapping[str, Any],
    ts_ms: int,
    created_at: str,
) -> list[FluxBalanceSnapshotRow]:
    rows: list[FluxBalanceSnapshotRow] = []
    positions = payload.get("positions")
    if not isinstance(positions, list):
        return rows

    for position in positions:
        if not isinstance(position, Mapping):
            continue
        instrument_id = _text(position.get("instrument_id"))
        if instrument_id is None:
            continue
        side = _text(position.get("side"))
        position_id = _text(position.get("position_id")) or side or instrument_id
        exchange = _exchange_from_instrument_id(instrument_id)
        asset = _upper_text(position.get("asset"))
        row_payload = dict(position)
        row_payload.setdefault("ts_ms", ts_ms)
        rows.append(
            FluxBalanceSnapshotRow(
                trader_id=trader_id,
                strategy_id=strategy_id,
                snapshot_id=snapshot_id,
                row_key=f"{exchange or 'unknown'}:{instrument_id}:{position_id}",
                kind="position",
                exchange=exchange,
                account_id=_text(position.get("account_id")),
                account=_lower_text(position.get("account")),
                asset=asset,
                instrument_id=instrument_id,
                side=side,
                signed_qty=_text(position.get("signed_qty")),
                quantity=_text(position.get("quantity")),
                free=None,
                locked=None,
                total=_text(position.get("total")),
                avg_px_open=_text(position.get("avg_px_open")),
                avg_px_close=_text(position.get("avg_px_close")),
                realized_pnl=_text(position.get("realized_pnl")),
                ts_ms=ts_ms,
                row_json=_canonical_json(row_payload),
                created_at=created_at,
            ),
        )
    return rows


def _fallback_rows(
    *,
    trader_id: str,
    strategy_id: str,
    snapshot_id: str,
    payload: Mapping[str, Any],
    ts_ms: int,
    created_at: str,
) -> list[FluxBalanceSnapshotRow]:
    from flux.api.payloads import build_balances_rows

    rows: list[FluxBalanceSnapshotRow] = []
    for row in build_balances_rows(raw_snapshot=payload, strategy_id=strategy_id):
        if not isinstance(row, Mapping):
            continue
        exchange = _lower_text(row.get("exchange") or row.get("venue"))
        instrument_id = _text(row.get("instrument_id"))
        asset = _upper_text(row.get("asset") or row.get("coin") or row.get("base"))
        side = _text(row.get("side"))
        kind = "position" if instrument_id or side or row.get("kind") == "position" else "cash"
        account_id = _text(row.get("account_id"))
        account = _lower_text(row.get("account"))
        row_key = _text(row.get("row_id")) or f"{kind}:{len(rows)}"
        rows.append(
            FluxBalanceSnapshotRow(
                trader_id=trader_id,
                strategy_id=strategy_id,
                snapshot_id=snapshot_id,
                row_key=row_key,
                kind=kind,
                exchange=exchange,
                account_id=account_id,
                account=account,
                asset=asset,
                instrument_id=instrument_id,
                side=side,
                signed_qty=_text(row.get("signed_qty")),
                quantity=_text(row.get("quantity")),
                free=_text(row.get("free")),
                locked=_text(row.get("locked")),
                total=_text(row.get("total")),
                avg_px_open=_text(row.get("avg_px_open")),
                avg_px_close=_text(row.get("avg_px_close")),
                realized_pnl=_text(row.get("realized_pnl")),
                ts_ms=_required_ts_ms(row.get("ts_ms"), None) or ts_ms,
                row_json=_canonical_json(row),
                created_at=created_at,
            ),
        )
    return rows


def _canonical_json(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def _exchange_from_account_id(account_id: str | None) -> str | None:
    if account_id is None:
        return None
    for separator in ("-", ":"):
        if separator in account_id:
            return account_id.split(separator, maxsplit=1)[0].strip().lower() or None
    return account_id.strip().lower() or None


def _exchange_from_instrument_id(instrument_id: str) -> str | None:
    if "." not in instrument_id:
        return None
    return instrument_id.split(".", maxsplit=1)[1].strip().lower() or None


def _required_ts_ms(value: Any, ts_event_ns: int | None) -> int:
    parsed = _optional_int(value)
    if parsed is not None:
        return parsed
    if ts_event_ns is not None:
        return ts_event_ns // 1_000_000
    return 0


def _optional_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _upper_text(value: Any) -> str | None:
    text = _text(value)
    return text.upper() if text is not None else None


def _lower_text(value: Any) -> str | None:
    text = _text(value)
    return text.lower() if text is not None else None
