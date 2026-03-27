from __future__ import annotations

from typing import Any

from flux.bridge.handlers.types import CorrelationContext
from flux.bridge.handlers.types import JSONRow
from flux.bridge.handlers.types import JSONValue
from flux.bridge.handlers.types import ReplaceHashJSONOp
from flux.bridge.handlers.types import SetJSONOp
from flux.bridge.handlers.types import WriteOp
from flux.bridge.handlers.utils import as_rows
from flux.bridge.handlers.utils import first_text
from flux.bridge.handlers.utils import normalize_exchange
from flux.bridge.handlers.utils import normalize_ts_ms
from flux.bridge.handlers.utils import with_correlation
from flux.common.keys import FluxRedisKeys


def _rows_from_payload(payload: Any) -> list[dict[str, Any]]:
    rows = as_rows(payload)
    expanded: list[dict[str, Any]] = []
    for row in rows:
        accounts = row.get("accounts")
        positions = row.get("positions")
        if isinstance(accounts, list):
            for account in accounts:
                if isinstance(account, dict):
                    expanded.append(dict(account))
        if isinstance(positions, list):
            for position in positions:
                if isinstance(position, dict):
                    out = dict(position)
                    out.setdefault("kind", "position")
                    expanded.append(out)
        if not isinstance(accounts, list) and not isinstance(positions, list):
            expanded.append(dict(row))
    return expanded


def _exchange_from_row(row: dict[str, Any]) -> str:
    account_id = first_text(row.get("account_id"))
    prefix = ""
    if "-" in account_id:
        prefix = account_id.split("-", maxsplit=1)[0]
    elif ":" in account_id:
        prefix = account_id.split(":", maxsplit=1)[0]
    return normalize_exchange(
        first_text(row.get("exchange"), row.get("venue"), row.get("source"), prefix, "unknown"),
    )


def _asset_from_row(row: dict[str, Any]) -> str:
    return first_text(
        row.get("asset"),
        row.get("coin"),
        row.get("base"),
        row.get("currency"),
        "UNKNOWN",
    ).upper()


def _account_from_row(row: dict[str, Any]) -> str:
    return first_text(
        row.get("account"),
        row.get("account_id"),
        row.get("balance_location"),
        row.get("scope"),
        "default",
    ).lower()


def transform_balances(payload: Any, context: CorrelationContext) -> list[WriteOp]:
    raw_rows = as_rows(payload)
    rows = _rows_from_payload(payload)
    has_explicit_snapshot = any(
        isinstance(row, dict)
        and (isinstance(row.get("accounts"), list) or isinstance(row.get("positions"), list))
        for row in raw_rows
    )
    if not rows and not has_explicit_snapshot:
        return []

    normalized_rows: list[JSONValue] = []
    mapping: dict[str, JSONRow] = {}

    for row in rows:
        exchange = _exchange_from_row(row)
        asset = _asset_from_row(row)
        account = _account_from_row(row)
        ts_ms = normalize_ts_ms(row, context.ts_ms)
        out = with_correlation(row, context, ts_ms=ts_ms)
        out["exchange"] = exchange
        out["asset"] = asset
        out["account"] = account
        normalized_rows.append(out)
        mapping[f"{exchange}:{asset}:{account}"] = out

    keys = FluxRedisKeys(strategy_id=context.strategy_id)
    return [
        SetJSONOp(key=keys.balances_snapshot(), value=normalized_rows),
        ReplaceHashJSONOp(key=keys.balances_rows(), mapping=mapping),
    ]
