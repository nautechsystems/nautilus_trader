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

    def as_payload(self, *, strategy_id: str) -> dict[str, str]:
        return {
            "strategy_id": strategy_id,
            "class": self.strategy_class,
            "strategy_groups": self.strategy_groups,
            "base_asset": self.base_asset,
            "quote_asset": self.quote_asset,
        }


def contract_id_for_leg(*, exchange: Any, symbol: Any) -> str:
    exchange_text = decode_text(exchange).strip().lower()
    symbol_text = decode_text(symbol).strip().upper()
    return f"{exchange_text}:{symbol_text}"


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
            continue

        flat_row: dict[str, Any] = {}
        for raw_key, raw_value in fields.items():
            key = decode_text(raw_key).strip()
            if not key or key == "payload":
                continue
            parsed_value = load_json(raw_value)
            if parsed_value is None:
                text_value = decode_text(raw_value)
                if text_value == "":
                    continue
                flat_row[key] = text_value
            else:
                flat_row[key] = parsed_value
        if flat_row:
            rows.append(flat_row)
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
        contract_id = contract_id_for_leg(exchange=contract.exchange, symbol=contract.symbol)
        row = market_rows.get(contract_id) or {}
        bid = safe_float(row.get("bid"))
        ask = safe_float(row.get("ask"))
        ts_ms = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
        age_ms = (current_ts_ms - ts_ms) if ts_ms else None
        out[contract_id] = {
            "contract_id": contract_id,
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


def _normalize_best_case(value: Any) -> str | None:
    text = decode_text(value).strip().lower()
    return text if text in {"case1", "case2"} else None


def _first_valid_float(*values: Any) -> float | None:
    for value in values:
        parsed = safe_float(value)
        if parsed is not None:
            return parsed
    return None


def _resolve_role_leg_id(*, role_value: Any, legs_order: list[str], legs: Mapping[str, Any]) -> str | None:
    text = decode_text(role_value).strip()
    if not text:
        return None
    if text in legs:
        return text

    upper = text.upper()
    if upper == "A" and legs_order:
        return legs_order[0]
    if upper == "B" and len(legs_order) > 1:
        return legs_order[1]
    return None


def _role_map_for_signal(
    *,
    state: Mapping[str, Any],
    legs_order: list[str],
    legs: Mapping[str, Any],
) -> dict[str, str]:
    out: dict[str, str] = {}
    raw_role_map = state.get("maker_role_map")
    if isinstance(raw_role_map, Mapping):
        maker_leg = _resolve_role_leg_id(role_value=raw_role_map.get("maker_leg"), legs_order=legs_order, legs=legs)
        ref_leg = _resolve_role_leg_id(role_value=raw_role_map.get("ref_leg"), legs_order=legs_order, legs=legs)
        hedge_leg = _resolve_role_leg_id(role_value=raw_role_map.get("hedge_leg"), legs_order=legs_order, legs=legs)
        if maker_leg:
            out["maker_leg"] = maker_leg
        if ref_leg:
            out["ref_leg"] = ref_leg
        if hedge_leg:
            out["hedge_leg"] = hedge_leg

    if "maker_leg" not in out and legs_order:
        out["maker_leg"] = legs_order[0]
    if "ref_leg" not in out and len(legs_order) > 1:
        out["ref_leg"] = legs_order[1]
    return out


def _spread_bps(*, buy_ask: float | None, sell_bid: float | None) -> float | None:
    if buy_ask is None or sell_bid is None or buy_ask <= 0:
        return None
    return ((sell_bid - buy_ask) / buy_ask) * 10_000.0


def _required_edges_by_case(params: Mapping[str, Any]) -> tuple[float | None, float | None]:
    pool_edge = _first_valid_float(params.get("pool_edge"), params.get("place_edge1"), 0.0)
    bid_edge = _first_valid_float(params.get("cex_bid_edge"), params.get("bid_edge1"), params.get("bid_edge"))
    ask_edge = _first_valid_float(params.get("cex_ask_edge"), params.get("ask_edge1"), params.get("ask_edge"))
    if bid_edge is None and ask_edge is None:
        return None, None

    if bid_edge is None:
        bid_edge = ask_edge
    if ask_edge is None:
        ask_edge = bid_edge
    if bid_edge is None or ask_edge is None:
        return None, None

    pool = pool_edge if pool_edge is not None else 0.0
    return ask_edge + pool, bid_edge + pool


def _risk_delta_from_balances(balances: Sequence[dict[str, Any]]) -> float | None:
    total = Decimal("0")
    found = False
    for row in balances:
        if not isinstance(row, dict):
            continue
        if not _is_position_row(row):
            continue
        qty = _position_signed_qty(row)
        if qty is None:
            continue
        total += qty
        found = True
    return float(total) if found else None


def _state_pricing_debug(state: Mapping[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    pricing_debug = state.get("pricing_debug")
    if not isinstance(pricing_debug, Mapping):
        return {}, {}
    pricing = pricing_debug.get("pricing")
    skew = pricing_debug.get("skew")
    return (
        dict(pricing) if isinstance(pricing, Mapping) else {},
        dict(skew) if isinstance(skew, Mapping) else {},
    )


def _derive_pricing_adjustments(
    *,
    state: Mapping[str, Any],
    params: Mapping[str, Any],
    ts_ms: int | None,
    risk_delta: float | None,
) -> list[dict[str, Any]]:
    state_adjustments = state.get("pricing_adjustments")
    if isinstance(state_adjustments, list):
        return [dict(item) for item in state_adjustments if isinstance(item, Mapping)]

    pricing, skew = _state_pricing_debug(state)
    total_skew_bps = _first_valid_float(skew.get("total_skew_bps"), pricing.get("effective_skew_bps"))
    if total_skew_bps is None and not skew:
        inventory_qty = risk_delta
        if inventory_qty is None:
            return []
        des_qty_global = _first_valid_float(params.get("des_qty_global"), 0.0) or 0.0
        max_qty_global = _first_valid_float(params.get("max_qty_global"), 0.0) or 0.0
        max_skew_global = _first_valid_float(params.get("max_skew_bps_global"), 0.0) or 0.0
        des_qty_local = _first_valid_float(params.get("des_qty_local"), 0.0) or 0.0
        max_qty_local = _first_valid_float(params.get("max_qty_local"), 0.0) or 0.0
        max_skew_local = _first_valid_float(params.get("max_skew_bps_local"), 0.0) or 0.0
        linear_offset = _first_valid_float(params.get("linear_offset_bps"), 0.0) or 0.0

        global_ratio = None
        global_skew = None
        if max_qty_global > 0:
            global_ratio = max(-1.0, min(1.0, (inventory_qty - des_qty_global) / max_qty_global))
            global_skew = global_ratio * max(0.0, max_skew_global)
        local_ratio = None
        local_skew = None
        if max_qty_local > 0:
            local_ratio = max(-1.0, min(1.0, (inventory_qty - des_qty_local) / max_qty_local))
            local_skew = local_ratio * max(0.0, max_skew_local)
        total_skew = linear_offset
        if global_skew is not None:
            total_skew += global_skew
        if local_skew is not None:
            total_skew += local_skew

        skew = {
            "inventory_qty": inventory_qty,
            "des_qty_global": des_qty_global,
            "max_qty_global": max_qty_global,
            "max_skew_bps_global": max_skew_global,
            "des_qty_local": des_qty_local,
            "max_qty_local": max_qty_local,
            "max_skew_bps_local": max_skew_local,
            "global_ratio": global_ratio,
            "global_skew_bps": global_skew,
            "local_ratio": local_ratio,
            "local_skew_bps": local_skew,
            "total_skew_bps": total_skew,
        }
        total_skew_bps = total_skew

    base_bid = _first_valid_float(pricing.get("bid_edge1_cfg_bps"), params.get("bid_edge1"))
    base_ask = _first_valid_float(pricing.get("ask_edge1_cfg_bps"), params.get("ask_edge1"))
    eff_bid = _first_valid_float(pricing.get("bid_edge1_eff_bps"))
    eff_ask = _first_valid_float(pricing.get("ask_edge1_eff_bps"))

    adjustment: dict[str, Any] = {
        "type": "inventory_skew",
    }
    if total_skew_bps is not None:
        adjustment["skew_bps_signed"] = total_skew_bps
        adjustment["inv_skew"] = total_skew_bps

    global_ratio = _first_valid_float(skew.get("global_ratio"))
    local_ratio = _first_valid_float(skew.get("local_ratio"))
    if global_ratio is not None:
        adjustment["inv_ratio_global"] = global_ratio
    if local_ratio is not None:
        adjustment["inv_ratio_local"] = local_ratio
    if global_ratio is not None and local_ratio is not None:
        adjustment["inv_ratio"] = max(-1.0, min(1.0, global_ratio + local_ratio))
    elif global_ratio is not None:
        adjustment["inv_ratio"] = global_ratio
    elif local_ratio is not None:
        adjustment["inv_ratio"] = local_ratio

    global_skew = _first_valid_float(skew.get("global_skew_bps"))
    local_skew = _first_valid_float(skew.get("local_skew_bps"))
    if global_skew is not None:
        adjustment["inv_skew_global"] = global_skew
    if local_skew is not None:
        adjustment["inv_skew_local"] = local_skew

    curr_qty = _first_valid_float(skew.get("inventory_qty"))
    if curr_qty is not None:
        adjustment["curr_qty"] = curr_qty

    des_qty = _first_valid_float(skew.get("des_qty_global"), params.get("des_qty_global"), params.get("des_qty"))
    max_qty = _first_valid_float(skew.get("max_qty_global"), params.get("max_qty_global"), params.get("max_qty"))
    max_skew = _first_valid_float(
        skew.get("max_skew_bps_global"),
        params.get("max_skew_bps_global"),
        params.get("max_skew_bps"),
    )
    if des_qty is not None:
        adjustment["des_qty"] = des_qty
    if max_qty is not None:
        adjustment["max_qty"] = max_qty
    if max_skew is not None:
        adjustment["max_skew_bps"] = max_skew

    if base_bid is not None:
        adjustment["base_bid_edge_bps"] = base_bid
    if base_ask is not None:
        adjustment["base_ask_edge_bps"] = base_ask
    if eff_bid is not None:
        adjustment["eff_bid_edge_bps"] = eff_bid
    if eff_ask is not None:
        adjustment["eff_ask_edge_bps"] = eff_ask
    if base_bid is not None and eff_bid is not None:
        adjustment["delta_bid_edge_bps"] = eff_bid - base_bid
    if base_ask is not None and eff_ask is not None:
        adjustment["delta_ask_edge_bps"] = eff_ask - base_ask

    if ts_ms is not None:
        adjustment["updated_ts_ms"] = ts_ms

    return [adjustment]


def _derive_strategy_family(strategy_class: str) -> str:
    normalized = strategy_class.strip().lower()
    if "maker_v3" in normalized:
        return "maker_v3"
    if "maker_v2" in normalized or normalized.startswith("maker"):
        return "maker_v2"
    return "taker"


def _derive_quote_snapshot(
    *,
    state: Mapping[str, Any],
    params: Mapping[str, Any],
    bot_on: bool,
    ts_ms: int | None,
    maker_leg: Mapping[str, Any] | None,
    ref_leg: Mapping[str, Any] | None,
) -> dict[str, Any]:
    state_maker_v3 = state.get("maker_v3")
    quote_snapshot = {}
    if isinstance(state_maker_v3, Mapping):
        raw_quote_snapshot = state_maker_v3.get("quote_snapshot")
        if isinstance(raw_quote_snapshot, Mapping):
            quote_snapshot = dict(raw_quote_snapshot)

    mode = "ON" if bot_on else "OFF"
    reason = decode_text(state.get("state"))
    quote_snapshot["mode"] = decode_text(quote_snapshot.get("mode")).strip() or mode
    quote_snapshot["reason"] = decode_text(quote_snapshot.get("reason")).strip() or reason
    quote_snapshot["ts_ms"] = coerce_ts_ms(quote_snapshot.get("ts_ms") or ts_ms)

    pricing, _ = _state_pricing_debug(state)
    maker_bid = _first_valid_float(
        pricing.get("maker_top_bid"),
        maker_leg.get("bid") if isinstance(maker_leg, Mapping) else None,
    )
    maker_ask = _first_valid_float(
        pricing.get("maker_top_ask"),
        maker_leg.get("ask") if isinstance(maker_leg, Mapping) else None,
    )
    ref_bid = _first_valid_float(
        pricing.get("ref_bid"),
        ref_leg.get("bid") if isinstance(ref_leg, Mapping) else None,
    )
    ref_ask = _first_valid_float(
        pricing.get("ref_ask"),
        ref_leg.get("ask") if isinstance(ref_leg, Mapping) else None,
    )
    place_bid = _first_valid_float(pricing.get("place_bid"), maker_bid)
    place_ask = _first_valid_float(pricing.get("place_ask"), maker_ask)
    cancel_bid = _first_valid_float(pricing.get("cancel_bid"))
    cancel_ask = _first_valid_float(pricing.get("cancel_ask"))
    eff_bid = _first_valid_float(pricing.get("bid_edge1_eff_bps"))
    eff_ask = _first_valid_float(pricing.get("ask_edge1_eff_bps"))
    place_edge = _first_valid_float(pricing.get("place_edge_bps"), params.get("place_edge1"))

    if isinstance(maker_leg, Mapping):
        quote_snapshot["maker_exchange"] = decode_text(maker_leg.get("exchange")).strip().lower() or None
        quote_snapshot["maker_symbol"] = decode_text(maker_leg.get("symbol")).strip() or None
        quote_snapshot["maker_top_ts_ms"] = coerce_ts_ms(maker_leg.get("ts_ms"))
    if isinstance(ref_leg, Mapping):
        quote_snapshot["ref_exchange"] = decode_text(ref_leg.get("exchange")).strip().lower() or None
        quote_snapshot["ref_symbol"] = decode_text(ref_leg.get("symbol")).strip() or None
        quote_snapshot["ref_ts_ms"] = coerce_ts_ms(ref_leg.get("ts_ms"))
    quote_snapshot["ref_source"] = decode_text(pricing.get("anchor_source")).strip() or "reference_leg"

    if maker_bid is not None:
        quote_snapshot["maker_top_bid"] = maker_bid
    if maker_ask is not None:
        quote_snapshot["maker_top_ask"] = maker_ask
    if ref_bid is not None:
        quote_snapshot["ref_bid"] = ref_bid
    if ref_ask is not None:
        quote_snapshot["ref_ask"] = ref_ask
    if place_bid is not None:
        quote_snapshot["place_bid"] = place_bid
    if place_ask is not None:
        quote_snapshot["place_ask"] = place_ask
    if cancel_bid is not None:
        quote_snapshot["cancel_bid"] = cancel_bid
    if cancel_ask is not None:
        quote_snapshot["cancel_ask"] = cancel_ask
    if eff_bid is not None:
        quote_snapshot["eff_bid_edge_bps"] = eff_bid
    if eff_ask is not None:
        quote_snapshot["eff_ask_edge_bps"] = eff_ask
    if place_edge is not None:
        quote_snapshot["place_edge_bps"] = place_edge

    return quote_snapshot


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
    legs_order = list(legs.keys())
    role_map = _role_map_for_signal(state=state, legs_order=legs_order, legs=legs)

    maker_leg = legs.get(role_map.get("maker_leg") or "") if role_map.get("maker_leg") else None
    ref_leg = legs.get(role_map.get("ref_leg") or "") if role_map.get("ref_leg") else None
    if not isinstance(maker_leg, Mapping):
        maker_leg = None
    if not isinstance(ref_leg, Mapping):
        ref_leg = None

    leg_a = maker_leg if maker_leg is not None else (legs.get(legs_order[0]) if legs_order else None)
    leg_b = ref_leg if ref_leg is not None else (legs.get(legs_order[1]) if len(legs_order) > 1 else None)
    if not isinstance(leg_a, Mapping):
        leg_a = None
    if not isinstance(leg_b, Mapping):
        leg_b = None

    spread_case1_derived = _spread_bps(
        buy_ask=_first_valid_float(leg_a.get("ask") if leg_a is not None else None),
        sell_bid=_first_valid_float(leg_b.get("bid") if leg_b is not None else None),
    )
    spread_case2_derived = _spread_bps(
        buy_ask=_first_valid_float(leg_b.get("ask") if leg_b is not None else None),
        sell_bid=_first_valid_float(leg_a.get("bid") if leg_a is not None else None),
    )

    spread_case1 = _first_valid_float(state.get("spread_net_case1_bps"), fv_row.get("spread_net_case1_bps"), spread_case1_derived)
    spread_case2 = _first_valid_float(state.get("spread_net_case2_bps"), fv_row.get("spread_net_case2_bps"), spread_case2_derived)

    best_case = _normalize_best_case(state.get("spread_net_best_case") or fv_row.get("spread_net_best_case"))
    if best_case is None:
        if spread_case1 is not None and spread_case2 is not None:
            best_case = "case1" if spread_case1 >= spread_case2 else "case2"
        elif spread_case1 is not None:
            best_case = "case1"
        elif spread_case2 is not None:
            best_case = "case2"

    spread_net_bps = _first_valid_float(state.get("spread_net_bps"), fv_row.get("spread_net_bps"))
    if spread_net_bps is None:
        if best_case == "case1":
            spread_net_bps = spread_case1
        elif best_case == "case2":
            spread_net_bps = spread_case2

    decision_edge_bps = _first_valid_float(
        state.get("decision_edge_bps"),
        fv_row.get("decision_edge_bps"),
        spread_net_bps,
    )

    required_case1, required_case2 = _required_edges_by_case(params)
    required_edge_bps = _first_valid_float(state.get("required_edge_bps"), fv_row.get("required_edge_bps"))
    if required_edge_bps is None:
        if best_case == "case1":
            required_edge_bps = required_case1
        elif best_case == "case2":
            required_edge_bps = required_case2

    edge2_bps = _first_valid_float(state.get("edge2_bps"), fv_row.get("edge2_bps"))
    if edge2_bps is None and decision_edge_bps is not None and required_edge_bps is not None:
        edge2_bps = decision_edge_bps - required_edge_bps

    risk_delta = _first_valid_float(state.get("risk_delta"), fv_row.get("risk_delta"))
    if risk_delta is None:
        risk_delta = _risk_delta_from_balances(balances)
    risk_delta_ts_ms = coerce_ts_ms(state.get("risk_delta_ts_ms") or fv_row.get("risk_delta_ts_ms") or ts_ms)

    pricing_adjustments = _derive_pricing_adjustments(
        state=state,
        params=params,
        ts_ms=ts_ms,
        risk_delta=risk_delta,
    )
    quote_snapshot = _derive_quote_snapshot(
        state=state,
        params=params,
        bot_on=bot_on,
        ts_ms=ts_ms,
        maker_leg=maker_leg,
        ref_leg=ref_leg,
    )

    state_quote_status = state.get("maker_quote_status")
    maker_quote_status = dict(state_quote_status) if isinstance(state_quote_status, Mapping) else None
    state_quote_stacks = state.get("quote_stacks")
    quote_stacks = dict(state_quote_stacks) if isinstance(state_quote_stacks, Mapping) else None
    state_balance_readiness = state.get("balance_readiness")
    balance_readiness = dict(state_balance_readiness) if isinstance(state_balance_readiness, Mapping) else None

    tradeable = bot_on
    near_tradeable = False
    blocked = not bot_on

    md_health: dict[str, Any] = {
        "legs_count": len(legs),
        "stale_legs": sorted(
            contract_id
            for contract_id, row in legs.items()
            if safe_int(row.get("age_ms")) is None
        ),
    }
    if ts_ms is not None:
        md_health["strategy_state_age_ms"] = max(0, now_ms() - ts_ms)

    return {
        "id": strategy_id,
        "meta": metadata.as_payload(strategy_id=strategy_id),
        "strategy_family": _derive_strategy_family(metadata.strategy_class),
        "tradeable": tradeable,
        "blocked": blocked,
        "near_tradeable": near_tradeable,
        "managed_orders": managed,
        "params": dict(params),
        "balances_ok": bool(balances),
        "risk_delta": risk_delta,
        "risk_delta_ts_ms": risk_delta_ts_ms,
        "decision_edge_bps": decision_edge_bps,
        "edge2_bps": edge2_bps,
        "required_edge_bps": required_edge_bps,
        "spread_net_bps": spread_net_bps,
        "spread_net_case1_bps": spread_case1,
        "spread_net_case2_bps": spread_case2,
        "spread_net_best_case": best_case,
        "maker_role_map": role_map,
        "maker_quote_status": maker_quote_status,
        "quote_stacks": quote_stacks,
        "pricing_adjustments": pricing_adjustments,
        "balance_readiness": balance_readiness,
        "maker_v3": {
            "quote_snapshot": quote_snapshot,
        },
        "state": state,
        "legs": legs,
        "legs_order": legs_order,
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
    since_seq: int | None = None,
) -> list[dict[str, Any]]:
    filtered: list[dict[str, Any]] = []
    for index, row in enumerate(rows):
        if strategy_id_from_row(row, strategy_id) != strategy_id:
            continue
        out = dict(row)
        seq = safe_int(out.get("seq"))
        if seq is not None:
            out["seq"] = seq
        if since_seq is not None:
            if seq is None or seq <= since_seq:
                continue
        ts_ms = coerce_ts_ms(out.get("ts_ms") or out.get("ts") or out.get("timestamp"))
        if since_ms is not None and ts_ms is not None and ts_ms <= since_ms:
            continue
        out["ts_ms"] = int(ts_ms if ts_ms is not None else 0)

        version = safe_int(out.get("version"))
        out["version"] = int(version if version is not None and version > 0 else 1)

        row_id = decode_text(out.get("row_id")).strip()
        if not row_id:
            entry_id = decode_text(out.get("entry_id")).strip()
            if entry_id:
                row_id = f"{strategy_id}:trade:entry:{entry_id}"
            if seq is not None:
                row_id = f"{strategy_id}:trade:{seq}:{out['ts_ms']}:{out['version']}"
            else:
                row_id = row_id or f"{strategy_id}:trade:{out['ts_ms']}:{index}"
        out["row_id"] = row_id
        filtered.append(out)
    if since_seq is not None:
        # Delta mode must stream oldest unseen seq first to avoid skipping rows.
        filtered.sort(key=lambda item: safe_int(item.get("seq")) or 0)
    else:
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
