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

from datetime import datetime
import json
from typing import Any

from nautilus_trader.flux.bridge.handlers.types import CorrelationContext
from nautilus_trader.flux.bridge.handlers.types import JSONRow


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

    try:
        ts = float(decode_text(value))
    except (TypeError, ValueError):
        try:
            ts = datetime.fromisoformat(decode_text(value).replace("Z", "+00:00")).timestamp()
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
    if base_text and quote_text and base_text.endswith(quote_text) and len(base_text) > len(quote_text):
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

    known_quotes = ("USDT", "USDC", "USD", "BTC", "ETH", "EUR", "GBP", "JPY", "BNB")
    for candidate in known_quotes:
        if cleaned.endswith(candidate) and len(cleaned) > len(candidate):
            return cleaned[: -len(candidate)], candidate
    return cleaned, ""


def normalize_ts_ms(row: dict[str, Any], fallback: int) -> int:
    ts_ms = coerce_ts_ms(
        row.get("ts_ms")
        or row.get("timestamp")
        or row.get("ts")
        or row.get("time")
        or row.get("datetime")
        or row.get("observed_ts")
    )
    if ts_ms is None:
        ts_ms = fallback
    return ts_ms


def with_correlation(row: dict[str, Any], context: CorrelationContext, *, ts_ms: int) -> JSONRow:
    out = dict(row)
    out["strategy_id"] = context.strategy_id
    out["topic"] = context.topic
    out["entry_id"] = context.entry_id
    out["ts_ms"] = int(ts_ms)
    return out
