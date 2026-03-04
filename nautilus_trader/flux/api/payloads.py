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

from dataclasses import dataclass
from datetime import datetime
from decimal import Decimal
from decimal import InvalidOperation
import json
import time
from typing import Any
from typing import Mapping
from typing import Sequence


@dataclass(frozen=True, slots=True)
class ContractCatalogEntry:
    exchange: str
    symbol: str


@dataclass(frozen=True, slots=True)
class StrategyMetadata:
    strategy_class: str
    strategy_groups: str
    base_asset: str
    quote_asset: str

    def as_payload(self) -> dict[str, str]:
        return {
            "class": self.strategy_class,
            "strategy_groups": self.strategy_groups,
            "base_asset": self.base_asset,
            "quote_asset": self.quote_asset,
        }


def now_ms() -> int:
    return int(time.time() * 1_000)


def decode_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return str(value)


def load_json(value: Any) -> Any:
    if value is None:
        return None
    if isinstance(value, dict | list | int | float | bool):
        return value
    text = decode_text(value).strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return None


def as_list(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        return list(value)
    if isinstance(value, tuple):
        return list(value)
    return [value]


def safe_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def safe_float(value: Any) -> float | None:
    if value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def safe_bool(value: Any) -> bool | None:
    if value is None:
        return None
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        if value in (0, 0.0):
            return False
        if value in (1, 1.0):
            return True
    text = decode_text(value).strip().lower()
    if text in {"1", "true", "t", "yes", "y", "on", "enabled"}:
        return True
    if text in {"0", "false", "f", "no", "n", "off", "disabled"}:
        return False
    return None


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
        return int(ts * 1_000)
    if ts >= 1_000_000_000_000_000_000:
        return int(ts / 1_000_000)
    if ts >= 1_000_000_000_000_000:
        return int(ts / 1_000)
    return int(ts)


def normalize_symbol_parts(*, symbol: str) -> tuple[str, str]:
    text = decode_text(symbol).strip().upper()
    if not text:
        return "", ""

    if "/" in text:
        base, quote = text.split("/", maxsplit=1)
        return base, quote

    cleaned = text.replace("-", "_")
    if "_" in cleaned:
        base, quote = cleaned.split("_", maxsplit=1)
        return base, quote

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
    for quote in known_quotes:
        if cleaned.endswith(quote) and len(cleaned) > len(quote):
            return cleaned[: -len(quote)], quote
    return cleaned, ""


def strategy_id_from_row(row: Any, fallback: str) -> str:
    if not isinstance(row, dict):
        return fallback
    strategy_id = decode_text(row.get("strategy_id")).strip()
    return strategy_id or fallback


def extract_stream_rows(stream_entries: Any) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for entry in as_list(stream_entries):
        if not isinstance(entry, (list, tuple)) or len(entry) != 2:
            continue
        _, fields = entry
        if not isinstance(fields, dict):
            continue
        payload = fields.get("payload")
        if payload is None:
            payload = fields.get(b"payload")
        parsed = load_json(payload)
        if isinstance(parsed, dict):
            rows.append(dict(parsed))
            continue
        if isinstance(parsed, list):
            for item in parsed:
                if isinstance(item, dict):
                    rows.append(dict(item))
    return rows


def select_latest_strategy_row(rows: Sequence[dict[str, Any]], strategy_id: str) -> dict[str, Any]:
    for row in rows:
        if strategy_id_from_row(row, strategy_id) == strategy_id:
            return dict(row)
    return dict(rows[0]) if rows else {}


def _to_decimal(value: Any) -> Decimal | None:
    if value is None:
        return None
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError, TypeError):
        text = decode_text(value).strip()
        if not text:
            return None
        try:
            return Decimal(text)
        except (InvalidOperation, ValueError):
            return None


def _decimal_text(value: Decimal) -> str:
    text = format(value, "f")
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    if text == "-0":
        return "0"
    return text or "0"


def _is_position_row(row: dict[str, Any]) -> bool:
    kind = decode_text(row.get("kind")).strip().lower()
    if kind == "position":
        return True
    asset = decode_text(row.get("asset") or row.get("coin") or row.get("base")).strip().upper()
    instrument_text = decode_text(row.get("instrument_id") or row.get("symbol")).strip().upper()
    return "PERP" in asset or "PERP" in instrument_text or "LINEAR" in instrument_text


def _position_signed_qty(row: dict[str, Any]) -> Decimal | None:
    source_key = ""
    value: Any = None
    for key in ("signed_qty", "total", "free", "quantity", "qty", "size"):
        candidate = row.get(key)
        if candidate is None:
            continue
        value = candidate
        source_key = key
        break
    qty = _to_decimal(value)
    if qty is None:
        return None
    if source_key != "signed_qty":
        side = decode_text(row.get("side") or row.get("position_side")).strip().upper()
        if side == "SHORT" and qty > 0:
            qty = -qty
        elif side == "LONG" and qty < 0:
            qty = -qty
    return qty


def _aggregate_position_rows(rows: list[dict[str, Any]], strategy_id: str) -> list[dict[str, Any]]:
    non_positions: list[dict[str, Any]] = []
    grouped: dict[tuple[str, str, str], dict[str, Any]] = {}

    for row in rows:
        if not isinstance(row, dict):
            continue
        if not _is_position_row(row):
            non_positions.append(dict(row))
            continue

        sid = strategy_id_from_row(row, strategy_id)
        exchange = decode_text(row.get("exchange") or row.get("venue")).strip().lower()
        instrument = decode_text(
            row.get("instrument_id")
            or row.get("symbol")
            or row.get("asset")
            or row.get("coin")
            or row.get("base"),
        ).strip().upper()
        if not instrument:
            non_positions.append(dict(row))
            continue

        key = (sid, exchange, instrument)
        agg = grouped.get(key)
        if agg is None:
            agg = {
                "row": dict(row),
                "qty": Decimal("0"),
                "avg_num": Decimal("0"),
                "avg_den": Decimal("0"),
                "upnl": Decimal("0"),
                "has_upnl": False,
            }
            grouped[key] = agg

        qty = _position_signed_qty(row)
        if qty is not None:
            agg["qty"] += qty
            avg_px = _to_decimal(
                row.get("avg_px")
                or row.get("avg_price")
                or row.get("entry_price")
                or row.get("avg_px_open")
                or row.get("avg_px_close"),
            )
            if avg_px is not None and qty != 0:
                agg["avg_num"] += abs(qty) * avg_px
                agg["avg_den"] += abs(qty)

        upnl = _to_decimal(
            row.get("unrealized_pnl")
            or row.get("unrealizedPnl")
            or row.get("realized_pnl")
            or row.get("realizedPnl"),
        )
        if upnl is not None:
            agg["upnl"] += upnl
            agg["has_upnl"] = True

    merged_positions: list[dict[str, Any]] = []
    for (sid, exchange, instrument), agg in grouped.items():
        qty = agg["qty"]
        if qty == 0:
            continue
        side = "LONG" if qty > 0 else "SHORT"
        avg_px = agg["avg_num"] / agg["avg_den"] if agg["avg_den"] > 0 else None
        upnl = agg["upnl"] if agg["has_upnl"] else None

        row = dict(agg["row"])
        row["strategy_id"] = sid
        if exchange:
            row["exchange"] = exchange
        row.setdefault("kind", "position")
        row["instrument_id"] = decode_text(row.get("instrument_id") or instrument).strip() or instrument
        if not decode_text(row.get("asset")).strip():
            row["asset"] = instrument
        qty_text = _decimal_text(qty)
        row["signed_qty"] = qty_text
        row["quantity"] = _decimal_text(abs(qty))
        row["free"] = qty_text
        row["total"] = qty_text

        meta_parts = [side]
        if avg_px is not None:
            meta_parts.append(f"avg={_decimal_text(avg_px)}")
        if upnl is not None:
            meta_parts.append(f"uPnL={_decimal_text(upnl)}")
        row["locked"] = " ".join(meta_parts)
        row["side"] = side
        row["row_id"] = f"{sid}:pos:{exchange}:{instrument}"
        merged_positions.append(row)

    return merged_positions + non_positions


def build_balances_rows(*, raw_snapshot: Any, strategy_id: str) -> list[dict[str, Any]]:
    rows = as_list(raw_snapshot)
    out: list[dict[str, Any]] = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        current = dict(row)
        sid = strategy_id_from_row(current, strategy_id)
        current["strategy_id"] = sid

        accounts = current.get("accounts")
        if isinstance(accounts, list) and accounts:
            for index, account in enumerate(accounts):
                if isinstance(account, dict):
                    out.append({**account, "strategy_id": sid, "row_id": f"{sid}:acc:{index}"})
            continue

        events = current.get("events")
        if isinstance(events, list) and events:
            flattened = 0
            for event_index, event in enumerate(events):
                if not isinstance(event, dict):
                    continue
                event_balances = event.get("balances")
                if not isinstance(event_balances, list):
                    continue
                account_id = decode_text(event.get("account_id")).strip()
                venue = account_id.split("-", maxsplit=1)[0].lower() if account_id else ""
                for balance_index, balance in enumerate(event_balances):
                    if not isinstance(balance, dict):
                        continue
                    asset = decode_text(balance.get("currency")).strip().upper()
                    out.append(
                        {
                            "strategy_id": sid,
                            "exchange": venue,
                            "asset": asset,
                            "coin": asset,
                            "base": asset,
                            "free": balance.get("free"),
                            "locked": balance.get("locked"),
                            "total": balance.get("total"),
                            "row_id": f"{sid}:evt:{event_index}:{balance_index}",
                        },
                    )
                    flattened += 1
            if flattened > 0:
                continue

        out.append(current)

    filtered = [row for row in out if strategy_id_from_row(row, strategy_id) == strategy_id]
    return _aggregate_position_rows(filtered, strategy_id)


def build_legs_payload(
    *,
    contracts: Sequence[ContractCatalogEntry],
    market_rows: Mapping[str, dict[str, Any]],
    now_ms_value: int | None = None,
) -> dict[str, Any]:
    current_ts_ms = now_ms() if now_ms_value is None else int(now_ms_value)
    out: dict[str, Any] = {}
    for contract in contracts:
        row = market_rows.get(contract.exchange) or {}
        bid = safe_float(row.get("bid"))
        ask = safe_float(row.get("ask"))
        ts_ms = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
        age_ms = (current_ts_ms - ts_ms) if ts_ms else None
        out[contract.exchange] = {
            "exchange": contract.exchange,
            "symbol": contract.symbol,
            "bid": bid,
            "ask": ask,
            "mid": (bid + ask) / 2.0 if bid is not None and ask is not None else None,
            "ts_ms": ts_ms,
            "age_ms": age_ms,
            "state": row.get("state") or "",
        }
    return out


def build_signals_payload(
    *,
    strategy_id: str,
    metadata: StrategyMetadata,
    state: dict[str, Any],
    fv_row: dict[str, Any],
    params: dict[str, Any],
    balances: list[dict[str, Any]],
    legs: dict[str, Any],
) -> dict[str, Any]:
    parsed_bot_on = safe_bool(state.get("bot_on"))
    bot_on = bool(parsed_bot_on if parsed_bot_on is not None else state.get("bot_on", False))
    managed = safe_int(state.get("managed_orders")) or 0
    ts_ms = coerce_ts_ms(state.get("ts_ms") or state.get("ts_event"))

    md_health: dict[str, Any] = {
        "legs_count": len(legs),
        "stale_legs": sorted(
            exchange
            for exchange, row in legs.items()
            if safe_int(row.get("age_ms")) is None
        ),
    }
    if ts_ms is not None:
        md_health["strategy_state_age_ms"] = max(0, now_ms() - ts_ms)

    return {
        "id": strategy_id,
        "meta": metadata.as_payload(),
        "tradeable": bot_on,
        "blocked": not bot_on,
        "near_tradeable": False,
        "managed_orders": managed,
        "maker_v3": {
            "quote_snapshot": {
                "mode": "ON" if bot_on else "OFF",
                "reason": decode_text(state.get("state")),
                "ts_ms": ts_ms,
            },
        },
        "state": state,
        "legs": legs,
        "fv_row": fv_row,
        "balances_count": len(balances),
        "debug": {
            "md_health": md_health,
            "params_loaded": bool(params),
        },
    }


def build_params_payload(
    *,
    strategy_id: str,
    params: dict[str, Any],
    schema: Mapping[str, Mapping[str, Any]],
) -> dict[str, Any]:
    return {
        "strategy_id": strategy_id,
        "params": params,
        "schema": {str(name): dict(spec) for name, spec in schema.items()},
    }


def build_trades_rows(
    *,
    rows: Sequence[dict[str, Any]],
    strategy_id: str,
    limit: int,
    since_ms: int | None,
) -> list[dict[str, Any]]:
    filtered: list[dict[str, Any]] = []
    for row in rows:
        if strategy_id_from_row(row, strategy_id) != strategy_id:
            continue
        out = dict(row)
        ts_ms = coerce_ts_ms(out.get("ts_ms") or out.get("ts") or out.get("timestamp"))
        if ts_ms is not None:
            out["ts_ms"] = ts_ms
        if since_ms is not None and ts_ms is not None and ts_ms <= since_ms:
            continue
        filtered.append(out)
    filtered.sort(key=lambda item: coerce_ts_ms(item.get("ts_ms")) or 0, reverse=True)
    return filtered[: max(1, limit)]


def build_alerts_rows(
    *,
    rows: Sequence[dict[str, Any]],
    strategy_id: str,
    limit: int,
) -> list[dict[str, Any]]:
    filtered = [dict(row) for row in rows if strategy_id_from_row(row, strategy_id) == strategy_id]
    filtered.sort(
        key=lambda item: coerce_ts_ms(item.get("ts_ms") or item.get("ts") or item.get("timestamp")) or 0,
        reverse=True,
    )
    return filtered[: max(1, limit)]


def build_error(*, code: str, message: str, details: Mapping[str, Any] | None = None) -> dict[str, Any]:
    payload = {"code": code, "message": message}
    if details:
        payload["details"] = dict(details)
    return payload


def build_envelope(
    *,
    ok: bool,
    request_id: str,
    timestamp_ms: int,
    api_version: str,
    data: Any,
    error: Mapping[str, Any] | None,
) -> dict[str, Any]:
    return {
        "ok": bool(ok),
        "api_version": api_version,
        "request_id": request_id,
        "timestamp_ms": int(timestamp_ms),
        "data": data,
        "error": dict(error) if error is not None else None,
    }

