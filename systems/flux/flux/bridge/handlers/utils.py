from __future__ import annotations

import json
from datetime import datetime
from typing import Any

from flux.bridge.handlers.types import CorrelationContext
from flux.bridge.handlers.types import JSONRow
from flux.common.config import validate_identifier_part


def decode_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return str(value)


def first_text(*values: Any) -> str:
    for value in values:
        text = decode_text(value).strip()
        if text:
            return text
    return ""


def load_json_payload(raw_payload: Any) -> Any:
    if raw_payload is None:
        return None
    if isinstance(raw_payload, dict | list):
        return raw_payload
    if isinstance(raw_payload, int | float | bool):
        return raw_payload

    text = decode_text(raw_payload).strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return text


def as_dict(payload: Any) -> dict[str, Any]:
    if isinstance(payload, dict):
        return dict(payload)
    if isinstance(payload, bytes | str):
        parsed = load_json_payload(payload)
        if isinstance(parsed, dict):
            return dict(parsed)
        return {"value": decode_text(payload)}
    if isinstance(payload, list):
        return {"rows": payload}
    return {"value": payload}


def as_rows(payload: Any) -> list[dict[str, Any]]:
    if isinstance(payload, dict):
        for key in ("rows", "trades", "alerts", "data", "items", "values"):
            value = payload.get(key)
            if isinstance(value, list):
                return [dict(row) for row in value if isinstance(row, dict)]
        return [dict(payload)]
    if isinstance(payload, list):
        return [dict(row) for row in payload if isinstance(row, dict)]
    if isinstance(payload, bytes | str):
        return as_rows(load_json_payload(payload))
    return []


def coerce_ts_ms(value: Any) -> int | None:
    if value is None:
        return None

    text = decode_text(value).strip()
    try:
        ts = float(text)
    except (TypeError, ValueError):
        try:
            iso = text
            if iso.endswith("Z"):
                iso = f"{iso[:-1]}+00:00"
            ts = datetime.fromisoformat(iso).timestamp()
        except (TypeError, ValueError):
            return None

    if ts <= 0:
        return None
    if ts < 1_000_000_000_000:
        return int(ts * 1000)
    if ts >= 1_000_000_000_000_000_000:
        return int(ts / 1_000_000)
    if ts >= 1_000_000_000_000_000:
        return int(ts / 1_000)
    return int(ts)


def normalize_exchange(exchange: Any) -> str:
    return first_text(exchange).lower()


def normalize_symbol_parts(
    *,
    base: Any = None,
    quote: Any = None,
    symbol: Any = None,
) -> tuple[str, str]:
    base_text = first_text(base).upper()
    quote_text = first_text(quote).upper()
    if (
        base_text
        and quote_text
        and base_text.endswith(quote_text)
        and len(base_text) > len(quote_text)
    ):
        base_text = base_text[: -len(quote_text)]
    if base_text and quote_text:
        return base_text, quote_text

    symbol_text = first_text(symbol).upper()
    symbol_text = symbol_text.split(".", maxsplit=1)[0].replace("-LINEAR", "")
    if not symbol_text:
        return "", ""

    cleaned = symbol_text.replace("-", "_").replace("/", "_")
    if "_" in cleaned:
        left, right = cleaned.split("_", 1)
        return left, right

    known_quotes = (
        "USDT",
        "USDC",
        "PUSD",
        "USDE",
        "USD",
        "BTC",
        "ETH",
        "EUR",
        "GBP",
        "JPY",
        "BNB",
    )
    for candidate in known_quotes:
        if cleaned.endswith(candidate) and len(cleaned) > len(candidate):
            return cleaned[: -len(candidate)], candidate
    return cleaned, ""


def normalize_ts_ms(row: dict[str, Any], fallback: int) -> int:
    ts_ms = coerce_ts_ms(
        row.get("ts_ms")
        or row.get("timestamp")
        or row.get("ts")
        or row.get("ts_event")
        or row.get("time")
        or row.get("datetime")
        or row.get("observed_ts"),
    )
    if ts_ms is None:
        ts_ms = fallback
    return ts_ms


def strategy_id_for_row(row: dict[str, Any], context: CorrelationContext) -> str:
    candidate = first_text(row.get("strategy_id"), row.get("external_strategy_id"))
    if candidate:
        try:
            return validate_identifier_part(candidate, "strategy_id")
        except ValueError:
            pass
    return context.strategy_id


def with_correlation(
    row: dict[str, Any],
    context: CorrelationContext,
    *,
    ts_ms: int,
    strategy_id: str | None = None,
) -> JSONRow:
    out = dict(row)
    out["strategy_id"] = strategy_id or context.strategy_id
    out["topic"] = context.topic
    out["entry_id"] = context.entry_id
    out["ts_ms"] = int(ts_ms)
    return out
