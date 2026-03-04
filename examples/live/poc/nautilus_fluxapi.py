#!/usr/bin/env python3
# -*- coding: utf-8 -*-
from __future__ import annotations

import importlib.util
import json
import os
from dataclasses import dataclass
from decimal import Decimal
from decimal import InvalidOperation
import time
from pathlib import Path
from typing import Any

import redis
from flask import Flask
from flask import jsonify
from flask import Response
from flask import request

from nautilus_trader.flux.params.manager import FluxParamsManager

DEFAULT_STRATEGY_ID = "bybit_binance_plumeusdt_makerv3"
DEFAULT_REDIS_HOST = "127.0.0.1"
DEFAULT_REDIS_PORT = 6380
DEFAULT_REDIS_DB = 0


PARAMS_DEFAULTS: dict[str, Any] = {
    "qty": 1_000.0,
    "des_qty_global": 0.0,
    "max_qty_global": 40_000.0,
    "max_skew_bps_global": 20.0,
    "des_qty_local": 0.0,
    "max_qty_local": 0.0,
    "max_skew_bps_local": 0.0,
    "linear_offset_bps": 0.0,
    "n_orders1": 5,
    "distance1": 2.0,
    "bid_edge1": 10.0,
    "ask_edge1": 10.0,
    "place_edge1": 2.0,
    "n_orders2": 0,
    "distance2": 5.0,
    "bid_edge2": 25.0,
    "ask_edge2": 25.0,
    "place_edge2": 2.0,
    "n_orders3": 0,
    "distance3": 5.0,
    "bid_edge3": 50.0,
    "ask_edge3": 50.0,
    "place_edge3": 2.0,
    "quote_fail_critical_after_count": 3,
    "quote_fail_critical_after_s": 60.0,
    "max_age_ms": 10_000,
    "bot_on": False,
}

PARAMS_SCHEMA: dict[str, dict[str, Any]] = {
    "qty": {"type": "number", "description": "Target base quantity per quote/hedge cycle (mapped from qty)."},
    "des_qty_global": {"type": "number", "description": "Global desired inventory target in base units."},
    "max_qty_global": {"type": "number", "description": "Global hard inventory cap in base units."},
    "max_skew_bps_global": {"type": "number", "description": "Global maker/hedge skew cap in bps."},
    "des_qty_local": {"type": "number", "description": "Local desired inventory target in base units."},
    "max_qty_local": {"type": "number", "description": "Local hard inventory cap in base units."},
    "max_skew_bps_local": {"type": "number", "description": "Local maker skew cap in bps."},
    "linear_offset_bps": {"type": "number", "description": "Linear inventory offset in bps."},
    "n_orders1": {"type": "integer", "description": "Band 1 order depth per side."},
    "distance1": {"type": "number", "description": "Band 1 spacing increment in bps."},
    "bid_edge1": {"type": "number", "description": "Band 1 bid edge in bps."},
    "ask_edge1": {"type": "number", "description": "Band 1 ask edge in bps."},
    "place_edge1": {"type": "number", "description": "Band 1 placement edge in bps."},
    "n_orders2": {"type": "integer", "description": "Band 2 order depth per side."},
    "distance2": {"type": "number", "description": "Band 2 spacing increment in bps."},
    "bid_edge2": {"type": "number", "description": "Band 2 bid edge in bps."},
    "ask_edge2": {"type": "number", "description": "Band 2 ask edge in bps."},
    "place_edge2": {"type": "number", "description": "Band 2 placement edge in bps."},
    "n_orders3": {"type": "integer", "description": "Band 3 order depth per side."},
    "distance3": {"type": "number", "description": "Band 3 spacing increment in bps."},
    "bid_edge3": {"type": "number", "description": "Band 3 bid edge in bps."},
    "ask_edge3": {"type": "number", "description": "Band 3 ask edge in bps."},
    "place_edge3": {"type": "number", "description": "Band 3 placement edge in bps."},
    "quote_fail_critical_after_count": {"type": "integer", "description": "Critical alert escalation after this many failed quotes."},
    "quote_fail_critical_after_s": {"type": "number", "description": "Critical quote-fail escalation time window in seconds."},
    "max_age_ms": {"type": "integer", "description": "Replace managed orders older than this age."},
    "bot_on": {"type": "boolean", "description": "Enable quote publishing and management."},
}

PARAMS_ORDER: tuple[str, ...] = (
    "qty",
    "des_qty_global",
    "max_qty_global",
    "max_skew_bps_global",
    "des_qty_local",
    "max_qty_local",
    "max_skew_bps_local",
    "linear_offset_bps",
    "n_orders1",
    "distance1",
    "bid_edge1",
    "ask_edge1",
    "place_edge1",
    "n_orders2",
    "distance2",
    "bid_edge2",
    "ask_edge2",
    "place_edge2",
    "n_orders3",
    "distance3",
    "bid_edge3",
    "ask_edge3",
    "place_edge3",
    "quote_fail_critical_after_count",
    "quote_fail_critical_after_s",
    "max_age_ms",
    "bot_on",
)


@dataclass(frozen=True)
class ContractMapping:
    exchange: str
    symbol: str


def _decode(value: Any) -> str:
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return "" if value is None else str(value)


def _load_json(value: Any) -> Any:
    if value is None:
        return None
    if isinstance(value, (dict, list, int, float, bool)):
        return value
    text = _decode(value).strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return None


def _coerce_ts_ms(value: Any) -> int | None:
    if value is None:
        return None
    try:
        raw = float(value)
    except (TypeError, ValueError):
        return None

    if raw <= 0:
        return None
    if raw < 1_000_000_000_000:
        return int(raw * 1_000)
    if raw >= 1_000_000_000_000_000_000:
        return int(raw / 1_000_000)
    if raw >= 1_000_000_000_000_000:
        return int(raw / 1000)
    return int(raw)


def _safe_float(value: Any) -> float | None:
    if value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def _safe_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _safe_bool(value: Any) -> bool | None:
    if value is None:
        return None
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        if value in (0, 0.0):
            return False
        if value in (1, 1.0):
            return True
    text = _decode(value).strip().lower()
    if not text:
        return None
    if text in {"1", "true", "t", "yes", "y", "on", "enabled"}:
        return True
    if text in {"0", "false", "f", "no", "n", "off", "disabled"}:
        return False
    return None


def _as_list(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        return list(value)
    if isinstance(value, tuple):
        return list(value)
    return [value]


def _decimal_text(value: Decimal) -> str:
    text = format(value, "f")
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    if text == "-0":
        return "0"
    return text or "0"


def _to_decimal(value: Any) -> Decimal | None:
    if value is None:
        return None
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError, TypeError):
        text = _decode(value).strip()
        if not text:
            return None
        try:
            return Decimal(text)
        except (InvalidOperation, ValueError):
            return None


def _is_position_row(row: dict[str, Any]) -> bool:
    kind = _decode(row.get("kind")).strip().lower()
    if kind == "position":
        return True
    asset = _decode(row.get("asset") or row.get("coin") or row.get("base")).strip().upper()
    instrument_text = _decode(row.get("instrument_id") or row.get("symbol")).strip().upper()
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
        side = _decode(row.get("side") or row.get("position_side")).strip().upper()
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

        sid = _strategy_id_from_row(row, strategy_id)
        exchange = _decode(row.get("exchange") or row.get("venue")).strip().lower()
        instrument = _decode(
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
        row["instrument_id"] = _decode(row.get("instrument_id") or instrument).strip() or instrument
        if not _decode(row.get("asset")).strip():
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


def _unwrap_bus_payload_row(row: Any) -> dict[str, Any]:
    if not isinstance(row, dict):
        return {}

    payload_type = _decode(row.get("type")).strip()
    if "MakerPocBusPayload" not in payload_type and not payload_type.endswith("PocBusPayload"):
        return dict(row)

    inner = _load_json(row.get("payload"))
    if isinstance(inner, list):
        for item in inner:
            if isinstance(item, dict):
                return dict(item)
        return {}
    if isinstance(inner, dict):
        return dict(inner)
    return {}


def _ordered_schema() -> dict[str, dict[str, Any]]:
    schema: dict[str, dict[str, Any]] = {}
    for key in PARAMS_ORDER:
        if key in PARAMS_SCHEMA:
            schema[key] = PARAMS_SCHEMA[key]
    for key, value in PARAMS_SCHEMA.items():
        if key not in schema:
            schema[key] = value
    return schema


def _params_manager(redis_client: redis.Redis, strategy_id: str) -> FluxParamsManager:
    return FluxParamsManager(
        redis_client=redis_client,
        strategy_id=strategy_id,
        schema=_ordered_schema(),
        defaults=PARAMS_DEFAULTS,
    )


def _load_params(redis_client: redis.Redis, strategy_id: str) -> dict[str, Any]:
    return _params_manager(redis_client, strategy_id).load()


def _params_store_error_payload(strategy_id: str, exc: ValueError) -> dict[str, Any]:
    return {
        "ok": False,
        "data": None,
        "error": {
            "code": "params_store_invalid",
            "message": str(exc),
            "strategy_id": strategy_id,
        },
    }


def _read_params_request_payload() -> dict[str, Any]:
    payload = request.get_json(silent=True)
    if not isinstance(payload, dict):
        return {}
    if isinstance(payload.get("params"), dict):
        return payload["params"]
    return {key: value for key, value in payload.items() if key != "source"}


def _build_params_payload(redis_client: redis.Redis, strategy_id: str) -> dict[str, Any]:
    return {
        "strategy_id": strategy_id,
        "params": _load_params(redis_client, strategy_id),
        "schema": _ordered_schema(),
    }


def _apply_params_update(redis_client: redis.Redis, strategy_id: str, updates: dict[str, Any]) -> dict[str, Any]:
    manager = _params_manager(redis_client, strategy_id)
    if not updates:
        return {"updated": [], "params": manager.load()}

    applied_updates = manager.update(updates)
    if applied_updates:
        manager.publish_update(applied_updates, ts_ms=_now_ms())

    return {"updated": sorted(applied_updates), "params": manager.load()}


def _load_contracts_module() -> Any | None:
    path = Path(__file__).resolve().parent / "contracts.py"
    if not path.exists():
        return None
    spec = importlib.util.spec_from_file_location("nautilus_poc_contracts", path)
    if spec is None or spec.loader is None:
        return None
    module = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(module)
        return module
    except Exception:
        return None


def _load_contract_mappings() -> tuple[ContractMapping, ...]:
    contracts_module = _load_contracts_module()
    if contracts_module is not None:
        entries = getattr(contracts_module, "INSTRUMENT_CONTRACTS", None)
        if entries:
            out: list[ContractMapping] = []
            for entry in entries:
                try:
                    out.append(ContractMapping(exchange=str(entry.chainsaw_exchange), symbol=str(entry.chainsaw_symbol)))
                except Exception:
                    continue
            if out:
                return tuple(out)

    return (
        ContractMapping(exchange="bybit_linear", symbol="PLUME/USDT"),
        ContractMapping(exchange="binance_spot", symbol="PLUME/USDT"),
    )


def _split_symbol(symbol: str) -> tuple[str, str]:
    if "/" in symbol:
        base, quote = symbol.split("/", maxsplit=1)
        if base and quote:
            return base.upper(), quote.upper()
    normalized = _decode(symbol).replace("-", "").upper()
    for quote in ("USDT", "USDC", "USD", "BTC", "ETH"):
        if normalized.endswith(quote) and len(normalized) > len(quote):
            return normalized[: -len(quote)], quote
    return "", ""


def _market_key(exchange: str, symbol: str) -> str:
    base, quote = _split_symbol(symbol)
    if not base or not quote:
        return ""
    return f"last:{exchange}:{base}_{quote}"


def _strategy_key(strategy_id: str) -> str:
    return f"maker_arb:{strategy_id}:state"


def _events_key(strategy_id: str) -> str:
    return f"maker_arb:{strategy_id}:events"


def _read_rows_from_list(redis_client: redis.Redis, key: str, limit: int | None = None) -> list[dict[str, Any]]:
    if limit is None or limit <= 0:
        limit = 200
    rows_raw = redis_client.lrange(key, 0, limit - 1)
    out: list[dict[str, Any]] = []
    for item in rows_raw:
        parsed = _load_json(item)
        if isinstance(parsed, dict):
            out.append(dict(parsed))
            continue
        if isinstance(parsed, list):
            for sub in parsed:
                if isinstance(sub, dict):
                    out.append(dict(sub))
            continue
        out.append({"value": parsed})
    return out


def _strategy_id_from_row(row: Any, fallback: str) -> str:
    if isinstance(row, dict):
        value = _decode(row.get("strategy_id")).strip()
        if value:
            return value
    return fallback


def _build_legs(redis_client: redis.Redis) -> dict[str, Any]:
    now_ms = _now_ms()
    out: dict[str, Any] = {}
    for contract in _load_contract_mappings():
        key = _market_key(contract.exchange, contract.symbol)
        row = _load_json(redis_client.get(key)) if key else {}
        if not isinstance(row, dict):
            row = {}
        bid = _safe_float(row.get("bid"))
        ask = _safe_float(row.get("ask"))
        ts_ms = _coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
        age_ms = (now_ms - ts_ms) if ts_ms else None
        out[contract.exchange] = {
            "exchange": contract.exchange,
            "symbol": contract.symbol,
            "bid": bid,
            "ask": ask,
            "mid": (bid + ask) / 2 if bid is not None and ask is not None else None,
            "ts_ms": ts_ms,
            "age_ms": age_ms,
            "state": row.get("state") or "",
        }
    return out


def _safe_bps(numerator: float | None, denominator: float | None) -> float | None:
    if numerator is None or denominator is None:
        return None
    if denominator == 0:
        return None
    return (numerator / denominator) * 10_000.0


def _clamp(value: float, lower: float, upper: float) -> float:
    return max(lower, min(upper, value))


def _build_pricing_skew_debug(
    *,
    legs: dict[str, Any],
    fv_row: dict[str, Any],
    params: dict[str, Any],
    balance_rows: list[dict[str, Any]],
    state: dict[str, Any],
) -> dict[str, Any]:
    now_ms = _now_ms()
    max_age_ms = _safe_int(params.get("max_age_ms")) or 10_000
    bybit_age_ms = _safe_int((legs.get("bybit_linear") or {}).get("age_ms"))
    binance_age_ms = _safe_int((legs.get("binance_spot") or {}).get("age_ms"))
    bybit_fresh_threshold_ms = max(1_000, max_age_ms)
    binance_fresh_threshold_ms = max(30_000, max_age_ms)
    leg_md_health: dict[str, Any] = {
        "bybit_age_ms": bybit_age_ms,
        "binance_age_ms": binance_age_ms,
        "bybit_fresh_threshold_ms": bybit_fresh_threshold_ms,
        "binance_fresh_threshold_ms": binance_fresh_threshold_ms,
        "bybit_fresh": bool(bybit_age_ms is not None and bybit_age_ms < bybit_fresh_threshold_ms),
        "binance_fresh": bool(binance_age_ms is not None and binance_age_ms < binance_fresh_threshold_ms),
        "age_source": "fluxapi_legs_ts_ms",
    }
    strategy_ts_ms = _coerce_ts_ms(state.get("ts_ms") or state.get("ts_event"))
    if strategy_ts_ms is not None:
        leg_md_health["strategy_state_age_ms"] = max(0, now_ms - strategy_ts_ms)

    runtime_debug = state.get("pricing_debug")
    if isinstance(runtime_debug, dict):
        output = dict(runtime_debug)
        current_md = output.get("md_health")
        merged_md = dict(current_md) if isinstance(current_md, dict) else {}
        merged_md.update(leg_md_health)
        output["md_health"] = merged_md
        return output

    bybit = legs.get("bybit_linear") or {}
    binance = legs.get("binance_spot") or {}

    bybit_bid = _safe_float(bybit.get("bid"))
    bybit_ask = _safe_float(bybit.get("ask"))
    bybit_mid = _safe_float(bybit.get("mid"))
    binance_mid = _safe_float(binance.get("mid"))
    fv = _safe_float(fv_row.get("fv"))

    bybit_spread = None
    if bybit_bid is not None and bybit_ask is not None:
        bybit_spread = bybit_ask - bybit_bid
    bybit_spread_bps = _safe_bps(bybit_spread, bybit_mid)
    bybit_bid_edge_bps = _safe_bps((fv - bybit_bid) if (fv is not None and bybit_bid is not None) else None, fv)
    bybit_ask_edge_bps = _safe_bps((bybit_ask - fv) if (fv is not None and bybit_ask is not None) else None, fv)
    basis_mid_bps = _safe_bps(
        (binance_mid - bybit_mid) if (binance_mid is not None and bybit_mid is not None) else None,
        bybit_mid,
    )

    plume_spot_total = None
    plume_perp_qty_sum = Decimal("0")
    plume_perp_qty_found = False
    for row in balance_rows:
        asset = _decode(row.get("asset") or row.get("coin") or row.get("base")).strip().upper()
        exchange = _decode(row.get("exchange") or row.get("venue")).strip().lower()
        instrument_text = _decode(row.get("instrument_id") or row.get("symbol")).strip().upper()
        is_position = (
            _decode(row.get("kind")).strip().lower() == "position"
            or "PERP" in asset
            or "PERP" in instrument_text
            or "LINEAR" in instrument_text
        )
        if exchange.startswith("bybit") and asset == "PLUME":
            plume_spot_total = _safe_float(row.get("total") or row.get("free"))
        if exchange.startswith("bybit") and is_position and (asset.startswith("PLUME") or "PLUME" in instrument_text):
            qty = _to_decimal(
                row.get("signed_qty")
                or row.get("total")
                or row.get("free")
                or row.get("quantity"),
            )
            if qty is not None:
                plume_perp_qty_sum += qty
                plume_perp_qty_found = True

    plume_perp_qty = float(plume_perp_qty_sum) if plume_perp_qty_found else None

    plume_effective_qty = plume_perp_qty if plume_perp_qty is not None else plume_spot_total

    des_qty_global = _safe_float(params.get("des_qty_global")) or 0.0
    max_qty_global = _safe_float(params.get("max_qty_global")) or 0.0
    max_skew_bps_global = _safe_float(params.get("max_skew_bps_global")) or 0.0
    des_qty_local = _safe_float(params.get("des_qty_local")) or 0.0
    max_qty_local = _safe_float(params.get("max_qty_local")) or 0.0
    max_skew_bps_local = _safe_float(params.get("max_skew_bps_local")) or 0.0
    linear_offset_bps = _safe_float(params.get("linear_offset_bps")) or 0.0

    global_ratio = None
    global_skew_bps = None
    if plume_effective_qty is not None and max_qty_global > 0:
        global_ratio = _clamp((plume_effective_qty - des_qty_global) / max_qty_global, -1.0, 1.0)
        global_skew_bps = global_ratio * max_skew_bps_global

    local_ratio = None
    local_skew_bps = None
    if plume_effective_qty is not None and max_qty_local > 0:
        local_ratio = _clamp((plume_effective_qty - des_qty_local) / max_qty_local, -1.0, 1.0)
        local_skew_bps = local_ratio * max_skew_bps_local

    skew_components = [value for value in (global_skew_bps, local_skew_bps) if value is not None]
    total_skew_bps = (sum(skew_components) if skew_components else 0.0) + linear_offset_bps

    return {
        "pricing": {
            "fv": fv,
            "bybit_mid": bybit_mid,
            "binance_mid": binance_mid,
            "basis_mid_bps": basis_mid_bps,
            "bybit_spread_bps": bybit_spread_bps,
            "bybit_bid_edge_bps": bybit_bid_edge_bps,
            "bybit_ask_edge_bps": bybit_ask_edge_bps,
        },
        "skew": {
            "plume_balance_total": plume_effective_qty,
            "plume_perp_position_qty": plume_perp_qty,
            "plume_spot_balance_total": plume_spot_total,
            "des_qty_global": des_qty_global,
            "max_qty_global": max_qty_global,
            "max_skew_bps_global": max_skew_bps_global,
            "des_qty_local": des_qty_local,
            "max_qty_local": max_qty_local,
            "max_skew_bps_local": max_skew_bps_local,
            "linear_offset_bps": linear_offset_bps,
            "global_ratio_estimate": global_ratio,
            "global_skew_bps_estimate": global_skew_bps,
            "local_ratio_estimate": local_ratio,
            "local_skew_bps_estimate": local_skew_bps,
            "effective_skew_bps": total_skew_bps,
        },
        "md_health": {
            **leg_md_health,
        },
    }


def _build_signals_payload(redis_client: redis.Redis, strategy_id: str) -> dict[str, Any]:
    state = _load_json(redis_client.get(_strategy_key(strategy_id)))
    if not isinstance(state, dict):
        state = {}

    fvs = _as_list(_load_json(redis_client.get("fvs.snapshot")))
    fv_row: dict[str, Any] = {}
    for row in fvs:
        if isinstance(row, dict) and _strategy_id_from_row(row, strategy_id) == strategy_id:
            fv_row = row
            break
    if not fv_row and fvs:
        if isinstance(fvs[0], dict):
            fv_row = fvs[0]
    fv_row = _unwrap_bus_payload_row(fv_row)

    bot_on = bool(state.get("bot_on", False))
    managed = _safe_int(state.get("managed_orders")) or 0
    ts_ms = _coerce_ts_ms(state.get("ts_ms") or state.get("ts_event"))
    legs = _build_legs(redis_client)
    params = _load_params(redis_client, strategy_id)
    balances = _build_balances_payload(redis_client, strategy_id)
    debug = _build_pricing_skew_debug(
        legs=legs,
        fv_row=fv_row,
        params=params,
        balance_rows=balances,
        state=state,
    )

    return {
        "id": strategy_id,
        "meta": {
            "class": "maker_v3",
            "strategy_groups": "tokenmm",
            "base_asset": "PLUME",
            "quote_asset": "USDT",
        },
        "tradeable": bot_on,
        "blocked": not bot_on,
        "near_tradeable": False,
        "managed_orders": managed,
        "maker_v3": {
            "quote_snapshot": {
                "mode": "ON" if bot_on else "OFF",
                "reason": str(state.get("state", "")),
                "ts_ms": ts_ms,
            },
        },
        "state": state,
        "legs": legs,
        "fv_row": fv_row,
        "debug": debug,
    }


def _build_balances_payload(redis_client: redis.Redis, strategy_id: str) -> list[dict[str, Any]]:
    raw = _load_json(redis_client.get("balances.snapshot"))
    rows = _as_list(raw)
    out: list[dict[str, Any]] = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        row = dict(row)
        sid = _strategy_id_from_row(row, strategy_id)
        row["strategy_id"] = sid
        accounts = row.get("accounts")
        if isinstance(accounts, list) and accounts:
            for index, account in enumerate(accounts):
                if not isinstance(account, dict):
                    continue
                out.append({**account, "strategy_id": sid, "row_id": f"{strategy_id}:acc:{index}"})
            continue
        events = row.get("events")
        if isinstance(events, list) and events:
            flattened = 0
            for event_index, event in enumerate(events):
                if not isinstance(event, dict):
                    continue
                balances = event.get("balances")
                if not isinstance(balances, list):
                    continue
                account_id = _decode(event.get("account_id")).strip()
                venue = account_id.split("-", maxsplit=1)[0].lower() if account_id else ""
                for balance_index, balance in enumerate(balances):
                    if not isinstance(balance, dict):
                        continue
                    asset = _decode(balance.get("currency")).strip().upper()
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
        out.append(row)

    filtered = [row for row in out if _strategy_id_from_row(row, strategy_id) == strategy_id]
    return _aggregate_position_rows(filtered, strategy_id)


def _build_trades_payload(redis_client: redis.Redis, strategy_id: str, limit: int, since_ms: int | None) -> list[dict[str, Any]]:
    rows = _read_rows_from_list(redis_client, "trades.blotter", limit=limit)
    filtered: list[dict[str, Any]] = []
    for row in rows:
        if _strategy_id_from_row(row, strategy_id) != strategy_id:
            continue
        row_ts = _coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
        if row_ts is not None:
            row["ts_ms"] = row_ts
        if since_ms is not None and row_ts is not None and row_ts <= since_ms:
            continue
        filtered.append(row)

    filtered.sort(key=lambda item: _coerce_ts_ms(item.get("ts_ms")) or 0, reverse=True)
    return filtered


def _build_alerts_payload(redis_client: redis.Redis, strategy_id: str, limit: int) -> list[dict[str, Any]]:
    rows = _read_rows_from_list(redis_client, "alerts.blotter", limit=limit)
    filtered = [row for row in rows if _strategy_id_from_row(row, strategy_id) == strategy_id]
    filtered.sort(key=lambda item: _coerce_ts_ms(item.get("ts_ms") or item.get("ts") or item.get("timestamp")) or 0, reverse=True)
    return filtered


def _now_ms() -> int:
    return int(time.time() * 1000)


def _is_redis_healthy(redis_client: redis.Redis) -> bool:
    try:
        return bool(redis_client.ping())
    except redis.RedisError:
        return False


BASE_HTML_TEMPLATE = """<!doctype html>
<html lang=\"en\">
  <head>
    <meta charset=\"utf-8\" />
    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
    <title>Nautilus TokenMM</title>
    <style>
      :root { color-scheme: dark; }
      body { margin: 0; font-family: Arial, sans-serif; background: #081025; color: #d8e2ff; }
      .wrap { max-width: 1200px; margin: 0 auto; padding: 18px; }
      nav a { display: inline-block; margin-right: 10px; color: #7fd1ff; text-decoration: none; border: 1px solid #233456; border-radius: 8px; padding: 6px 10px; }
      nav a.active { background: #12213c; }
      .card { background: #101c35; border: 1px solid #223357; border-radius: 10px; padding: 12px; margin-top: 12px; }
      table { width: 100%; border-collapse: collapse; font-size: 13px; }
      th, td { border-bottom: 1px solid #223357; text-align: left; padding: 8px 6px; }
      th { color: #93c5fd; }
      .mono { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; }
      .status { margin-top: 8px; font-size: 12px; color: #9caecf; }
      .ok { color: #86efac; }
      .warn { color: #facc15; }
      .err { color: #f87171; }
      .toolbar { margin-top: 8px; }
      .toolbar button { padding: 6px 10px; border: 1px solid #294066; border-radius: 8px; background: #1d2f50; color: #d8e2ff; cursor: pointer; }
      .toolbar button:disabled { opacity: 0.5; cursor: not-allowed; }
      .sub { color: #9aaecf; margin-top: 8px; }
      .section { margin-top: 12px; }
      pre { max-height: 280px; overflow: auto; white-space: pre-wrap; }
      .debug-full { max-height: none; overflow: visible; min-height: 320px; }
    </style>
  </head>
  <body>
    <div class=\"wrap\">
      <h1>Nautilus TokenMM</h1>
      <div id=\"status\" class=\"status mono\">loading</div>
      <div class=\"card\" id=\"content\"></div>
    </div>
  <script>
      const STRATEGY = new URLSearchParams(window.location.search).get('strategy') || '__DEFAULT_STRATEGY__';
      const STRATEGY_QUERY = `strategy=${encodeURIComponent(STRATEGY)}`;
      let paramsEditing = false;
      let paramsDirty = false;

      const fmt = (value) => (value === null || value === undefined ? '-' : String(value));
      const fmtTs = (value) => {
        const v = Number(value);
        if (!Number.isFinite(v) || v <= 0) {
          return '-';
        }
        return new Date(v).toISOString();
      };
      const escapeHtml = (value) => String(value ?? '').replace(/[&<>"']/g, (char) => {
        const map = {
          '&': '&amp;',
          '<': '&lt;',
          '>': '&gt;',
          '\"': '&quot;',
          "'": '&#39;',
        };
        return map[char];
      });
      const apiGet = async (path) => {
        const response = await fetch(`${path}?${STRATEGY_QUERY}`);
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}`);
        }
        return response.json();
      };

      const renderSignalPanel = (strategy, fvRow, legs, debug) => {
        const rows = Object.entries(legs || {}).map(
          ([name, row]) =>
            `<tr><td>${escapeHtml(name)}</td><td>${escapeHtml(row.exchange)}</td><td>${escapeHtml(row.bid)}</td><td>${escapeHtml(row.ask)}</td><td>${escapeHtml(row.mid)}</td><td>${escapeHtml(row.age_ms)} ms</td></tr>`,
        ).join('');
        return `
          <div class=\"section card\">
            <h3>Signal</h3>
            <div class=\"mono\">strategy=${escapeHtml(strategy.id || '-')} blocked=${escapeHtml(strategy.blocked)} managed_orders=${escapeHtml(strategy.managed_orders || 0)} bot_state=${escapeHtml(strategy.tradeable ? 'on' : 'off')}</div>
            <h4>Market BBO</h4>
            <table><thead><tr><th>Leg</th><th>Exchange</th><th>Bid</th><th>Ask</th><th>Mid</th><th>Age</th></tr></thead><tbody>${rows || '<tr><td colspan=\"6\">No data</td></tr>'}</tbody></table>
            <h4>Last FV</h4>
            <pre class=\"mono\">${escapeHtml(JSON.stringify(fvRow || {}, null, 2))}</pre>
            <h4>Pricing/Skew Debug</h4>
            <pre class=\"mono debug-full\">${escapeHtml(JSON.stringify(debug || {}, null, 2))}</pre>
            <h4>State</h4>
            <pre class=\"mono\">${escapeHtml(JSON.stringify(strategy.state || {}, null, 2))}</pre>
          </div>
        `;
      };

      const renderTable = (headerHtml, bodyHtml) => {
        const columns = (headerHtml.match(/<th>/g) || []).length || 6;
        return `<table><thead><tr>${headerHtml}</tr></thead><tbody>${bodyHtml || `<tr><td colspan=\"${columns}\">No rows</td></tr>`}</tbody></table>`;
      };

      const orderedParamNames = (params, schema) => {
        const seen = new Set();
        const out = [];
        for (const name of Object.keys(schema || {})) {
          if (!seen.has(name)) {
            seen.add(name);
            out.push(name);
          }
        }
        for (const name of Object.keys(params || {})) {
          if (!seen.has(name)) {
            seen.add(name);
            out.push(name);
          }
        }
        return out;
      };

      const renderParamsSection = ({params, schema, prefix, title}) => {
        const names = orderedParamNames(params || {}, schema || {});
        const rows = names.map((name) => {
          const meta = schema[name] || {};
          const type = meta.type || 'number';
          const value = Object.prototype.hasOwnProperty.call(params || {}, name) ? params[name] : '';
          if (type === 'boolean') {
            return `<tr><td>${escapeHtml(name)}</td><td><label><input type=\"checkbox\" name=\"${escapeHtml(name)}\" data-param=\"${escapeHtml(name)}\" data-type=\"boolean\" ${value ? 'checked' : ''} /></label></td><td>${escapeHtml(meta.type || '')}</td><td>${escapeHtml(meta.description || '-')}</td></tr>`;
          }
          return `<tr><td>${escapeHtml(name)}</td><td><input type=\"text\" name=\"${escapeHtml(name)}\" data-param=\"${escapeHtml(name)}\" data-type=\"${escapeHtml(type)}\" value=\"${escapeHtml(value)}\" /></td><td>${escapeHtml(meta.type || '')}</td><td>${escapeHtml(meta.description || '-')}</td></tr>`;
        }).join('');
        return `
          <div class=\"section card\">
            <h3>${escapeHtml(title || 'Params')}</h3>
            <form id=\"${prefix}-params-form\">
              <table><thead><tr><th>Param</th><th>Value</th><th>Type</th><th>Description</th></tr></thead><tbody>${rows || '<tr><td colspan=\"4\">No params</td></tr>'}</tbody></table>
              <div class=\"toolbar\"><button type=\"submit\">Save params</button> <button type=\"button\" id=\"${prefix}-params-reset\">Reload values</button></div>
            </form>
            <div id=\"${prefix}-params-status\" class=\"status mono\"></div>
          </div>
        `;
      };

      const postParams = async (event, options) => {
        event.preventDefault();
        const form = event.currentTarget;
        const status = document.getElementById(`${options.prefix}-params-status`);
        const updates = {};
        const fields = form.querySelectorAll('[data-param]');
        for (const field of fields) {
          const name = field.getAttribute('data-param');
          const type = field.getAttribute('data-type') || 'number';
          if (!name) {
            continue;
          }
          if (type === 'boolean') {
            updates[name] = !!field.checked;
            continue;
          }
          updates[name] = `${field.value ?? ''}`;
        }
        const btn = form.querySelector('button[type=\"submit\"]');
        if (btn) {
          btn.disabled = true;
        }
        status.innerHTML = '<span class=\"warn\">saving...</span>';
        try {
          const response = await fetch(`/api/v1/params?${STRATEGY_QUERY}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ params: updates, source: 'ui' }),
          });
          const payload = await response.json();
          if (!response.ok || payload?.ok === false) {
            status.innerHTML = `<span class=\"err\">save failed: ${escapeHtml(payload?.error || `HTTP ${response.status}`)}</span>`;
          } else {
            paramsDirty = false;
            paramsEditing = false;
            status.innerHTML = '<span class=\"ok\">saved</span>';
            if (typeof options.refresh === 'function') {
              await options.refresh();
            }
          }
        } catch (error) {
          status.innerHTML = `<span class=\"err\">save failed: ${escapeHtml(String(error))}</span>`;
        } finally {
          if (btn) {
            btn.disabled = false;
          }
        }
      };

      const renderHome = async () => {
        const [signalData, paramsData, balancesData, tradesData, alertsData] = await Promise.all([
          apiGet('/api/v1/signals'),
          apiGet('/api/v1/params'),
          apiGet('/api/v1/balances'),
          apiGet('/api/v1/trades'),
          apiGet('/api/v1/alerts'),
        ]);

        const strategy = (signalData?.data?.strategies || [])[0] || {};
        const row = (paramsData?.data?.strategies || [])[0] || {};
        const allBalanceRows = balancesData?.data?.rows || [];
        const positions = allBalanceRows.filter(
          (entry) => String(entry.kind || '').toLowerCase() === 'position' || String(entry.asset || '').toUpperCase().includes('PERP'),
        );
        const balances = allBalanceRows.filter(
          (entry) => !positions.includes(entry),
        );
        const trades = tradesData?.data?.rows || [];
        const alerts = alertsData?.data?.rows || [];

        const balanceRows = balances.map((entry) => `<tr><td>${escapeHtml(entry.strategy_id)}</td><td>${escapeHtml(entry.exchange || entry.venue || '')}</td><td>${escapeHtml(entry.base || entry.coin || '')}</td><td>${escapeHtml(entry.free)}</td><td>${escapeHtml(entry.locked)}</td><td>${escapeHtml(entry.total)}</td></tr>`).join('');
        const positionRows = positions.map((entry) => `<tr><td>${escapeHtml(entry.strategy_id)}</td><td>${escapeHtml(entry.exchange || entry.venue || '')}</td><td>${escapeHtml(entry.asset || entry.symbol || entry.instrument_id || '')}</td><td>${escapeHtml(entry.free || entry.signed_qty || entry.quantity || '')}</td><td>${escapeHtml(entry.locked || entry.side || '')}</td><td>${escapeHtml(entry.total || entry.signed_qty || '')}</td></tr>`).join('');
        const tradeRows = trades.map((entry) => `<tr><td>${fmtTs(entry.ts_ms)}</td><td>${escapeHtml(entry.side || entry.order_side)}</td><td>${escapeHtml(entry.qty)}</td><td>${escapeHtml(entry.price)}</td><td>${escapeHtml(entry.symbol || entry.coin)}</td><td>${escapeHtml(entry.venue || entry.exchange)}</td></tr>`).join('');
        const alertRows = alerts.map((entry) => `<tr><td>${fmtTs(entry.ts_ms)}</td><td>${escapeHtml(entry.level)}</td><td>${escapeHtml(entry.message || entry.event || entry.text)}</td><td>${escapeHtml(entry.reason)}</td></tr>`).join('');

        document.getElementById('content').innerHTML = `
          ${renderSignalPanel(strategy, strategy.fv_row || {}, strategy.legs || {}, strategy.debug || {})}
          ${renderParamsSection({
            params: row.params || {},
            schema: row.schema || {},
            prefix: 'home',
            title: 'Params',
          })}
          <div class=\"section card\"><h3>Positions</h3>${renderTable('<th>strategy_id</th><th>exchange</th><th>instrument</th><th>qty</th><th>meta</th><th>value</th>', positionRows)}</div>
          <div class=\"section card\"><h3>Balances</h3>${renderTable('<th>strategy_id</th><th>exchange</th><th>asset</th><th>free</th><th>locked</th><th>total</th>', balanceRows)}</div>
          <div class=\"section card\"><h3>Trades</h3>${renderTable('<th>time</th><th>side</th><th>qty</th><th>price</th><th>symbol</th><th>venue</th>', tradeRows)}</div>
          <div class=\"section card\"><h3>Alerts</h3>${renderTable('<th>time</th><th>level</th><th>message</th><th>reason</th>', alertRows)}</div>
        `;
        document
          .getElementById('home-params-form')
          .addEventListener('submit', (event) => {
            postParams(event, {
              prefix: 'home',
              refresh: async () => {
                await renderHome();
              },
            });
          });
        const paramsForm = document.getElementById('home-params-form');
        const paramsReset = document.getElementById('home-params-reset');
        if (paramsForm) {
          paramsForm.addEventListener('focusin', () => {
            paramsEditing = true;
          });
          paramsForm.addEventListener('input', () => {
            paramsEditing = true;
            paramsDirty = true;
          });
          paramsForm.addEventListener('focusout', () => {
            setTimeout(() => {
              if (!paramsForm.contains(document.activeElement) && !paramsDirty) {
                paramsEditing = false;
              }
            }, 0);
          });
        }
        if (paramsReset) {
          paramsReset.addEventListener('click', async () => {
            paramsDirty = false;
            paramsEditing = false;
            await renderHome();
          });
        }
      };

      const render = async () => {
        try {
          const health = await (await fetch('/api/v1/healthz')).json();
          const ok = health?.ok ? 'ok' : 'error';
          document.getElementById('status').innerHTML = `${ok} redis=${health?.data?.redis_available ? 'up' : 'down'} strategy=${escapeHtml(STRATEGY)}`;
          if (paramsEditing || paramsDirty) {
            return;
          }
          await renderHome();
        } catch (err) {
          document.getElementById('status').innerHTML = `<span class=\"err\">dashboard error</span> ${err}`;
        }
      };

      render();
      setInterval(render, 2000);
    </script>
  </body>
</html>
""".replace("__DEFAULT_STRATEGY__", DEFAULT_STRATEGY_ID)


def build_app() -> Flask:
    app = Flask(__name__)
    app.config["JSON_SORT_KEYS"] = False
    app.json.sort_keys = False

    redis_host = os.getenv("FLUX_REDIS_HOST", DEFAULT_REDIS_HOST)
    redis_port = int(os.getenv("FLUX_REDIS_PORT", str(DEFAULT_REDIS_PORT)))
    redis_db = int(os.getenv("FLUX_REDIS_DB", str(DEFAULT_REDIS_DB)))
    redis_username = os.getenv("FLUX_REDIS_USERNAME") or None
    redis_password = os.getenv("FLUX_REDIS_PASSWORD") or None

    redis_client = redis.Redis(
        host=redis_host,
        port=redis_port,
        db=redis_db,
        username=redis_username,
        password=redis_password,
        decode_responses=False,
    )

    @app.get("/")
    @app.get("/tokenmm")
    @app.get("/tokenmm/signal")
    @app.get("/tokenmm/params")
    @app.get("/tokenmm/balances")
    @app.get("/tokenmm/trades")
    @app.get("/tokenmm/alerts")
    def tokenmm() -> Response:
        return Response(
            BASE_HTML_TEMPLATE,
            headers={
                "Cache-Control": "no-store, no-cache, must-revalidate, max-age=0",
                "Pragma": "no-cache",
                "Expires": "0",
            },
            mimetype="text/html",
        )

    @app.get("/api/v1/healthz")
    def health() -> Any:
        redis_ok = _is_redis_healthy(redis_client)
        fvs = _as_list(_load_json(redis_client.get("fvs.snapshot")))
        return jsonify(
            {
                "ok": redis_ok,
                "data": {
                    "redis_available": redis_ok,
                    "has_fvs": bool(fvs),
                    "time_ms": _now_ms(),
                },
                "error": None if redis_ok else "redis_unavailable",
            },
        )

    @app.get("/api/v1/readyz")
    def ready() -> Any:
        redis_ok = _is_redis_healthy(redis_client)
        fvs = _as_list(_load_json(redis_client.get("fvs.snapshot")))
        ready = bool(redis_ok and fvs)
        return jsonify(
            {
                "ok": ready,
                "data": {
                    "redis_available": redis_ok,
                    "has_fvs": bool(fvs),
                },
                "error": None if ready else "service_not_ready",
            },
        ), (200 if ready else 503)

    @app.get("/api/v1/signals")
    def api_signals() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        try:
            strategies = [_build_signals_payload(redis_client, strategy_id=sid)]
        except ValueError as exc:
            return jsonify(_params_store_error_payload(sid, exc)), 500
        data = {
            "server_ts_ms": _now_ms(),
            "strategies": strategies,
        }
        return jsonify({"ok": True, "data": data, "error": None}), 200

    @app.get("/api/v1/param-schema")
    def api_param_schema() -> Any:
        return jsonify({"ok": True, "data": {"schema": _ordered_schema()}, "error": None}), 200

    @app.get("/api/v1/params")
    def api_params() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        try:
            strategies = [_build_params_payload(redis_client, sid)]
        except ValueError as exc:
            return jsonify(_params_store_error_payload(sid, exc)), 500
        payload = {"strategies": strategies}
        return jsonify({"ok": True, "data": payload, "error": None}), 200

    @app.post("/api/v1/params")
    def api_params_update() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        updates = _read_params_request_payload()
        if not updates:
            return jsonify({"ok": False, "data": None, "error": "missing_payload"}), 400
        try:
            result = _apply_params_update(redis_client, sid, updates)
        except ValueError as exc:
            return jsonify({"ok": False, "data": None, "error": str(exc)}), 400
        payload = {
            "strategy_id": sid,
            "updated": result["updated"],
            "params": result["params"],
            "schema": _ordered_schema(),
        }
        return jsonify({"ok": True, "data": payload, "error": None}), 200

    @app.get("/api/v1/strategies")
    def api_strategies() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        try:
            strategies = [_build_signals_payload(redis_client, strategy_id=sid)]
        except ValueError as exc:
            return jsonify(_params_store_error_payload(sid, exc)), 500
        return jsonify(
            {
                "ok": True,
                "data": {
                    "strategies": strategies,
                    "count": 1,
                },
                "error": None,
            },
        ), 200

    @app.get("/api/v1/strategies/<string:strategy_id>/parameters")
    def api_strategy_parameters(strategy_id: str) -> Any:
        sid = strategy_id or DEFAULT_STRATEGY_ID
        try:
            params = _load_params(redis_client, sid)
        except ValueError as exc:
            return jsonify(_params_store_error_payload(sid, exc)), 500
        payload = {"strategy_id": sid, "params": params, "schema": _ordered_schema()}
        return jsonify({"ok": True, "data": payload, "error": None}), 200

    @app.post("/api/v1/strategies/<string:strategy_id>/parameters")
    @app.patch("/api/v1/strategies/<string:strategy_id>/parameters")
    def api_strategy_parameters_update(strategy_id: str) -> Any:
        sid = strategy_id or DEFAULT_STRATEGY_ID
        updates = _read_params_request_payload()
        if not updates:
            return jsonify({"ok": False, "data": None, "error": "missing_payload"}), 400
        try:
            result = _apply_params_update(redis_client, sid, updates)
        except ValueError as exc:
            return jsonify({"ok": False, "data": None, "error": str(exc)}), 400
        return jsonify(
            {
                "ok": True,
                "data": {
                    "strategy_id": sid,
                    "updated": result["updated"],
                    "params": result["params"],
                    "schema": _ordered_schema(),
                },
                "error": None,
            },
        ), 200

    @app.get("/api/v1/balances")
    def api_balances() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        limit = _safe_int(request.args.get("limit")) or 50
        rows = _build_balances_payload(redis_client, sid)
        return jsonify(
            {
                "ok": True,
                "data": {
                    "rows": rows[: max(1, min(200, limit))],
                    "count": len(rows),
                    "server_ts_ms": _now_ms(),
                },
                "error": None,
            },
        ), 200

    @app.get("/api/v1/trades")
    def api_trades() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        limit = _safe_int(request.args.get("limit")) or 50
        rows = _build_trades_payload(redis_client, sid, limit=max(1, min(200, limit)), since_ms=None)
        return jsonify({"ok": True, "data": {"rows": rows, "count": len(rows), "server_ts_ms": _now_ms()}, "error": None}), 200

    @app.get("/api/v1/trades/delta")
    def api_trades_delta() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        limit = _safe_int(request.args.get("limit")) or 50
        since_ms = _coerce_ts_ms(request.args.get("after"))
        rows = _build_trades_payload(redis_client, sid, limit=max(1, min(200, limit)), since_ms=since_ms)
        return jsonify({"ok": True, "data": {"rows": rows, "count": len(rows), "server_ts_ms": _now_ms(), "after": since_ms}, "error": None}), 200

    @app.get("/api/v1/alerts")
    def api_alerts() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        limit = _safe_int(request.args.get("limit")) or 50
        rows = _build_alerts_payload(redis_client, sid, limit=max(1, min(200, limit)))
        return jsonify({"ok": True, "data": {"rows": rows, "count": len(rows), "server_ts_ms": _now_ms()}, "error": None}), 200

    return app


def main() -> None:
    app = build_app()
    host = os.getenv("HOST", "0.0.0.0")
    port = int(os.getenv("PORT", "5022"))
    app.run(host=host, port=port, debug=False, use_reloader=False)


if __name__ == "__main__":
    main()
