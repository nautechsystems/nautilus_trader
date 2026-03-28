from __future__ import annotations

"""Leg and signal payload assembly helpers."""

from collections.abc import Callable
from collections.abc import Mapping
from collections.abc import Sequence
from typing import Any
from typing import cast

from nautilus_trader.flux.strategies.shared.quote_health import evaluate_quote_health

from ._payloads_common import ContractCatalogEntry
from ._payloads_common import StrategyMetadata
from ._payloads_common import _is_position_row
from ._payloads_common import _position_signed_qty
from ._payloads_common import canonical_naming_fields
from ._payloads_common import coerce_ts_ms
from ._payloads_common import contract_id_for_leg
from ._payloads_common import decode_text
from ._payloads_common import safe_bool
from ._payloads_common import safe_float
from ._payloads_common import safe_int


def build_legs_payload_impl(
    *,
    contracts: Sequence[ContractCatalogEntry],
    market_rows: Mapping[str, dict[str, Any]],
    current_ts_ms: int,
) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for contract in contracts:
        contract_id = contract_id_for_leg(
            exchange=contract.exchange,
            symbol=contract.symbol,
            instrument_id=contract.instrument_id,
        )
        row = market_rows.get(contract_id) or {}
        bid = safe_float(row.get("bid"))
        ask = safe_float(row.get("ask"))
        ts_ms = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
        age_ms = (current_ts_ms - ts_ms) if ts_ms else None
        leg = {
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
        leg.update(
            canonical_naming_fields(
                instrument_id=row.get("instrument_id") or contract.instrument_id,
                exchange=contract.exchange,
                symbol=contract.symbol,
                asset=None,
                inventory_asset=None,
                is_position=False,
            ),
        )
        leg.setdefault("coin", leg.get("base_asset") or leg.get("inventory_asset") or "")
        out[contract_id] = leg
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


def _first_valid_bool(*values: Any) -> bool | None:
    for value in values:
        parsed = safe_bool(value)
        if parsed is not None:
            return parsed
    return None


def _first_valid_int(*values: Any) -> int | None:
    for value in values:
        parsed = safe_int(value)
        if parsed is not None:
            return parsed
    return None


def _first_valid_text(*values: Any) -> str | None:
    for value in values:
        parsed = decode_text(value).strip()
        if parsed:
            return parsed
    return None


def _first_valid_upper_text(*values: Any) -> str | None:
    parsed = _first_valid_text(*values)
    return parsed.upper() if parsed is not None else None


def _resolve_role_leg_id(
    *,
    role_value: Any,
    legs_order: list[str],
    legs: Mapping[str, Any],
) -> str | None:
    text = decode_text(role_value).strip()
    if not text:
        return None
    if text in legs:
        return text

    raw_instrument_id = text.split(":", maxsplit=1)[1] if ":" in text else text
    raw_instrument_id = raw_instrument_id.strip().upper()
    if raw_instrument_id:
        for leg_id, leg in legs.items():
            if not isinstance(leg, Mapping):
                continue
            leg_instrument_id = decode_text(leg.get("instrument_id")).strip().upper()
            if leg_instrument_id and leg_instrument_id == raw_instrument_id:
                return leg_id

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
        maker_leg = _resolve_role_leg_id(
            role_value=raw_role_map.get("maker_leg"),
            legs_order=legs_order,
            legs=legs,
        )
        ref_leg = _resolve_role_leg_id(
            role_value=raw_role_map.get("ref_leg"),
            legs_order=legs_order,
            legs=legs,
        )
        hedge_leg = _resolve_role_leg_id(
            role_value=raw_role_map.get("hedge_leg"),
            legs_order=legs_order,
            legs=legs,
        )
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
    bid_edge = _first_valid_float(
        params.get("cex_bid_edge"),
        params.get("bid_edge1"),
        params.get("bid_edge"),
    )
    ask_edge = _first_valid_float(
        params.get("cex_ask_edge"),
        params.get("ask_edge1"),
        params.get("ask_edge"),
    )
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
    total = 0.0
    found = False
    for row in balances:
        if not isinstance(row, dict):
            continue
        if not _is_position_row(row):
            continue
        qty = _position_signed_qty(row)
        if qty is None:
            continue
        total += float(qty)
        found = True
    return total if found else None


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


def _first_inventory_skew_adjustment(
    pricing_adjustments: Sequence[Mapping[str, Any]] | Sequence[dict[str, Any]],
) -> dict[str, Any]:
    for adjustment in pricing_adjustments:
        if not isinstance(adjustment, Mapping):
            continue
        if decode_text(adjustment.get("type")).strip().lower() != "inventory_skew":
            continue
        return dict(adjustment)
    return {}


def _project_signal_inventory_fields(
    *,
    state: Mapping[str, Any],
    pricing_adjustments: Sequence[dict[str, Any]],
) -> dict[str, Any]:
    _pricing, skew = _state_pricing_debug(state)
    inventory_adjustment = _first_inventory_skew_adjustment(pricing_adjustments)

    projected: dict[str, Any] = {}

    position_qty_venue = _first_valid_float(
        state.get("position_qty_venue"),
        state.get("local_position_qty_venue"),
        skew.get("local_position_qty_venue"),
        skew.get("position_qty_venue"),
    )
    if position_qty_venue is not None:
        projected["position_qty_venue"] = position_qty_venue

    position_qty_base = _first_valid_float(
        state.get("position_qty_base"),
        state.get("local_position_qty_base"),
        skew.get("local_position_qty_base"),
        skew.get("position_qty_base"),
    )
    if position_qty_base is not None:
        projected["position_qty_base"] = position_qty_base

    local_qty_base = _first_valid_float(
        state.get("local_qty_base"),
        state.get("local_qty"),
        skew.get("local_inventory_qty_base"),
        skew.get("local_inventory_qty"),
        inventory_adjustment.get("local_qty_base"),
        inventory_adjustment.get("local_qty"),
    )
    if local_qty_base is not None:
        projected["local_qty_base"] = local_qty_base
        projected["local_qty"] = local_qty_base

    local_qty_venue = _first_valid_float(
        state.get("local_qty_venue"),
        inventory_adjustment.get("local_qty_venue"),
    )
    if local_qty_venue is not None:
        projected["local_qty_venue"] = local_qty_venue

    global_qty_base = _first_valid_float(
        state.get("global_qty_base"),
        state.get("global_qty"),
        skew.get("global_inventory_qty_base"),
        skew.get("global_inventory_qty"),
        skew.get("inventory_qty_base"),
        skew.get("inventory_qty"),
        inventory_adjustment.get("global_qty_base"),
        inventory_adjustment.get("global_qty"),
        inventory_adjustment.get("curr_qty"),
    )
    if global_qty_base is not None:
        projected["global_qty_base"] = global_qty_base
        projected["global_qty"] = global_qty_base

    global_qty_complete = _first_valid_bool(
        state.get("global_qty_base_complete"),
        state.get("global_qty_complete"),
        skew.get("global_inventory_qty_base_complete"),
        skew.get("global_inventory_qty_complete"),
        inventory_adjustment.get("global_qty_base_complete"),
        inventory_adjustment.get("global_qty_complete"),
    )
    if global_qty_complete is not None:
        projected["global_qty_base_complete"] = global_qty_complete
        projected["global_qty_complete"] = global_qty_complete

    aggregation_mode = _first_valid_text(
        state.get("aggregation_mode"),
        skew.get("global_inventory_aggregation_mode"),
        inventory_adjustment.get("aggregation_mode"),
    )
    if aggregation_mode is not None:
        projected["aggregation_mode"] = aggregation_mode

    qty_conversion_status = _first_valid_text(
        state.get("qty_conversion_status"),
        state.get("local_qty_conversion_status"),
        skew.get("local_position_qty_conversion_status"),
        skew.get("qty_conversion_status"),
        inventory_adjustment.get("qty_conversion_status"),
    )
    if qty_conversion_status is not None:
        projected["qty_conversion_status"] = qty_conversion_status

    qty_conversion_source = _first_valid_text(
        state.get("qty_conversion_source"),
        state.get("local_qty_conversion_source"),
        skew.get("local_position_qty_conversion_source"),
        skew.get("qty_conversion_source"),
        inventory_adjustment.get("qty_conversion_source"),
    )
    if qty_conversion_source is not None:
        projected["qty_conversion_source"] = qty_conversion_source

    return projected


def _build_fallback_inventory_skew_adjustments(
    *,
    state: Mapping[str, Any],
    params: Mapping[str, Any],
    ts_ms: int | None,
    risk_delta: float | None,
) -> list[dict[str, Any]]:
    """Backfill inventory skew details when state payloads omit them or send sparse adjustments."""

    pricing, skew = _state_pricing_debug(state)
    total_skew_bps = _first_valid_float(
        skew.get("total_skew_bps"),
        pricing.get("effective_skew_bps"),
    )
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
            global_ratio = max(-1.0, min(1.0, (des_qty_global - inventory_qty) / max_qty_global))
            global_skew = global_ratio * max(0.0, max_skew_global)
        local_ratio = None
        local_skew = None
        if max_qty_local > 0:
            local_ratio = max(-1.0, min(1.0, (des_qty_local - inventory_qty) / max_qty_local))
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
    # `skew_bps_signed` is the canonical signed quote translation exported to
    # operators and Fluxboard. Positive means our market moved up; negative
    # means it moved down. UI should prefer this instead of re-deriving sign.
    if total_skew_bps is not None:
        adjustment["skew_bps_signed"] = total_skew_bps
        adjustment["inv_skew"] = total_skew_bps

    linear_offset_bps = _first_valid_float(skew.get("linear_offset_bps"), params.get("linear_offset_bps"))
    if linear_offset_bps is not None:
        adjustment["linear_offset_bps"] = linear_offset_bps

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

    global_qty = _first_valid_float(
        state.get("global_qty_base"),
        skew.get("global_inventory_qty_base"),
        skew.get("inventory_qty_base"),
        skew.get("global_inventory_qty"),
        skew.get("inventory_qty"),
        skew.get("global_qty"),
    )
    if global_qty is not None:
        adjustment["global_qty_base"] = global_qty
        adjustment["global_qty"] = global_qty
        adjustment["curr_qty"] = global_qty
    local_qty = _first_valid_float(
        state.get("local_qty_base"),
        skew.get("local_inventory_qty_base"),
        skew.get("local_inventory_qty"),
        skew.get("local_qty"),
    )
    if local_qty is not None:
        adjustment["local_qty_base"] = local_qty
        adjustment["local_qty"] = local_qty

    global_qty_complete = safe_bool(state.get("global_qty_base_complete"))
    if global_qty_complete is None:
        global_qty_complete = safe_bool(skew.get("global_inventory_qty_base_complete"))
    if global_qty_complete is None:
        global_qty_complete = safe_bool(skew.get("global_inventory_qty_complete"))
    if global_qty_complete is not None:
        adjustment["global_qty_base_complete"] = global_qty_complete
        adjustment["global_qty_complete"] = global_qty_complete

    aggregation_mode = decode_text(
        state.get("aggregation_mode") or skew.get("global_inventory_aggregation_mode"),
    ).strip()
    if aggregation_mode:
        adjustment["aggregation_mode"] = aggregation_mode

    des_qty = _first_valid_float(
        skew.get("des_qty_global"),
        params.get("des_qty_global"),
        params.get("des_qty"),
    )
    max_qty = _first_valid_float(
        skew.get("max_qty_global"),
        params.get("max_qty_global"),
        params.get("max_qty"),
    )
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


def _normalize_inventory_skew_adjustment(adjustment: Mapping[str, Any] | dict[str, Any]) -> dict[str, Any]:
    normalized = dict(adjustment)
    if decode_text(normalized.get("type")).strip().lower() != "inventory_skew":
        return normalized

    signed_skew = _first_valid_float(normalized.get("skew_bps_signed"))
    legacy_skew = _first_valid_float(normalized.get("inv_skew"))
    if signed_skew is not None:
        normalized["skew_bps_signed"] = signed_skew
        normalized["inv_skew"] = signed_skew
    elif legacy_skew is not None:
        normalized["skew_bps_signed"] = legacy_skew
        normalized["inv_skew"] = legacy_skew

    for key in ("linear_offset_bps", "inv_skew_global", "inv_skew_local"):
        value = _first_valid_float(normalized.get(key))
        if value is not None:
            normalized[key] = value

    return normalized


def _merge_inventory_skew_adjustments(
    *,
    current: list[dict[str, Any]],
    fallback: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Merge sparse inventory-skew adjustments onto a fully derived fallback adjustment."""

    if not fallback:
        return current
    if not current:
        return fallback

    fallback_inventory = next(
        (dict(item) for item in fallback if item.get("type") == "inventory_skew"),
        None,
    )
    if fallback_inventory is None:
        return current

    merged: list[dict[str, Any]] = []
    merged_inventory = False
    for item in current:
        if item.get("type") != "inventory_skew" or merged_inventory:
            merged.append(_normalize_inventory_skew_adjustment(item))
            continue
        merged_item = dict(fallback_inventory)
        for key, value in item.items():
            if value is not None:
                merged_item[key] = value
        merged.append(_normalize_inventory_skew_adjustment(merged_item))
        merged_inventory = True

    if not merged_inventory:
        merged.append(_normalize_inventory_skew_adjustment(fallback_inventory))
    return merged


def _derive_pricing_adjustments(
    *,
    state: Mapping[str, Any],
    params: Mapping[str, Any],
    ts_ms: int | None,
    risk_delta: float | None,
) -> list[dict[str, Any]]:
    state_adjustments = state.get("pricing_adjustments")
    if isinstance(state_adjustments, list):
        normalized = [
            _normalize_inventory_skew_adjustment(item)
            for item in state_adjustments
            if isinstance(item, Mapping)
        ]
        fallback = _build_fallback_inventory_skew_adjustments(
            state=state,
            params=params,
            ts_ms=ts_ms,
            risk_delta=risk_delta,
        )
        return _merge_inventory_skew_adjustments(current=normalized, fallback=fallback)

    return [
        _normalize_inventory_skew_adjustment(item)
        for item in _build_fallback_inventory_skew_adjustments(
        state=state,
        params=params,
        ts_ms=ts_ms,
        risk_delta=risk_delta,
        )
    ]


def _derive_strategy_family(strategy_class: str) -> str:
    normalized = strategy_class.strip().lower()
    if "equities_maker" in normalized:
        return "equities_maker"
    if "equities_taker" in normalized:
        return "equities_taker"
    if "maker_v4" in normalized:
        return "maker_v4"
    if "maker_v3" in normalized:
        return "maker_v3"
    if "maker_v2" in normalized or normalized.startswith("maker"):
        return "maker_v2"
    return "taker"


def _uses_equities_arb_contract(strategy_family: str) -> bool:
    return strategy_family in {"maker_v4", "equities_maker", "equities_taker"}


def _normalize_v4_leg_snapshot(
    existing: Mapping[str, Any] | None,
    leg: Mapping[str, Any] | None,
) -> dict[str, Any]:
    payload = dict(existing) if isinstance(existing, Mapping) else {}
    if isinstance(leg, Mapping):
        venue = decode_text(leg.get("exchange") or leg.get("venue")).strip().upper()
        route = decode_text(leg.get("route")).strip().upper()
        symbol = decode_text(leg.get("symbol")).strip()
        instrument_id = decode_text(leg.get("instrument_id")).strip()
        bid = _first_valid_float(leg.get("bid"))
        ask = _first_valid_float(leg.get("ask"))
        mid = _first_valid_float(leg.get("mid"))
        ts_ms = coerce_ts_ms(leg.get("ts_ms"))
        age_ms = safe_int(leg.get("age_ms"))
        existing_ts_ms = coerce_ts_ms(payload.get("ts_ms"))
        prefer_live_market = ts_ms is not None and (existing_ts_ms is None or ts_ms >= existing_ts_ms)
        if venue and (prefer_live_market or not decode_text(payload.get("venue")).strip()):
            payload["venue"] = venue
        if route and (prefer_live_market or not decode_text(payload.get("route")).strip()):
            payload["route"] = route
        if symbol and (prefer_live_market or not decode_text(payload.get("symbol")).strip()):
            payload["symbol"] = symbol
        if instrument_id and (prefer_live_market or not decode_text(payload.get("instrument_id")).strip()):
            payload["instrument_id"] = instrument_id
        if bid is not None and (prefer_live_market or _first_valid_float(payload.get("bid")) is None):
            payload["bid"] = bid
        if ask is not None and (prefer_live_market or _first_valid_float(payload.get("ask")) is None):
            payload["ask"] = ask
        if mid is not None and (prefer_live_market or _first_valid_float(payload.get("mid")) is None):
            payload["mid"] = mid
        if ts_ms is not None and (prefer_live_market or coerce_ts_ms(payload.get("ts_ms")) is None):
            payload["ts_ms"] = ts_ms
        if age_ms is not None and (prefer_live_market or safe_int(payload.get("age_ms")) is None):
            payload["age_ms"] = age_ms
    return payload


def _apply_quote_health_to_v4_leg(
    payload: Mapping[str, Any] | None,
    *,
    leg_role: str,
    max_quote_age_ms: int,
) -> dict[str, Any]:
    normalized = dict(payload) if isinstance(payload, Mapping) else {}
    transport_connected = (
        True
        if normalized.get("ts_ms") is not None
        or normalized.get("bid") is not None
        or normalized.get("ask") is not None
        else None
    )
    quote_health = evaluate_quote_health(
        leg_role=leg_role,
        bid=_first_valid_float(normalized.get("bid")),
        ask=_first_valid_float(normalized.get("ask")),
        quote_age_ms=safe_int(normalized.get("age_ms")),
        max_quote_age_ms=max_quote_age_ms,
        transport_connected=transport_connected,
        subscription_healthy=transport_connected,
    )
    explicit_feed_state = decode_text(normalized.get("feed_state")).strip().lower()
    if explicit_feed_state not in {"ok", "degraded", "down", "unknown"}:
        explicit_feed_state = ""
    explicit_quote_state = decode_text(normalized.get("quote_state")).strip().lower()
    if explicit_quote_state not in {"fresh", "old", "missing"}:
        explicit_quote_state = ""
    if explicit_feed_state or explicit_quote_state:
        optimistic_explicit = explicit_feed_state == "ok" and explicit_quote_state == "fresh"
        conservative_explicit = (
            explicit_feed_state in {"degraded", "down", "unknown"}
            or explicit_quote_state in {"old", "missing"}
        )
        live_age_ms = safe_int(normalized.get("age_ms"))
        explicit_stale_override = live_age_ms is not None and live_age_ms > max_quote_age_ms
        if not conservative_explicit and (not optimistic_explicit or explicit_stale_override):
            normalized["feed_state"] = quote_health.feed_state
            normalized["quote_state"] = quote_health.quote_state
            normalized["pricing_usable"] = quote_health.usable_for_pricing
            normalized["hedge_usable"] = quote_health.usable_for_hedging
            if quote_health.reason_code is not None:
                normalized["reason_code"] = quote_health.reason_code
            else:
                normalized.pop("reason_code", None)
            return normalized

        if explicit_feed_state:
            normalized["feed_state"] = explicit_feed_state
        else:
            normalized.pop("feed_state", None)
        if explicit_quote_state:
            normalized["quote_state"] = explicit_quote_state
        else:
            normalized.pop("quote_state", None)

        derived_usable: bool | None = None
        if explicit_feed_state == "ok" and explicit_quote_state == "fresh":
            derived_usable = True
        elif explicit_feed_state in {"degraded", "down", "unknown"} or explicit_quote_state in {"old", "missing"}:
            derived_usable = False

        if normalized.get("pricing_usable") is None and derived_usable is not None:
            normalized["pricing_usable"] = derived_usable
        if normalized.get("hedge_usable") is None and derived_usable is not None:
            normalized["hedge_usable"] = derived_usable
        return normalized
    normalized["feed_state"] = quote_health.feed_state
    normalized["quote_state"] = quote_health.quote_state
    normalized["pricing_usable"] = quote_health.usable_for_pricing
    normalized["hedge_usable"] = quote_health.usable_for_hedging
    if quote_health.reason_code is not None:
        normalized["reason_code"] = quote_health.reason_code
    else:
        normalized.pop("reason_code", None)
    return normalized


def _ibkr_route_from_instrument_id_text(instrument_id: str | None) -> str | None:
    text = decode_text(instrument_id).strip().upper()
    if "." not in text:
        return None
    route = text.rsplit(".", maxsplit=1)[-1].strip()
    if route in {"SMART", "BLUEOCEAN"}:
        return route
    return None


def _derive_quote_snapshot_v4(
    *,
    state: Mapping[str, Any],
    params: Mapping[str, Any],
    ts_ms: int | None,
    maker_leg: Mapping[str, Any] | None,
    hedge_leg: Mapping[str, Any] | None,
    ref_leg: Mapping[str, Any] | None,
    hedge_leg_id: str | None = None,
) -> dict[str, Any]:
    state_maker_v4 = state.get("maker_v4")
    quote_snapshot: dict[str, Any] = {}
    if isinstance(state_maker_v4, Mapping):
        raw_quote_snapshot = state_maker_v4.get("quote_snapshot")
        if isinstance(raw_quote_snapshot, Mapping):
            quote_snapshot = dict(raw_quote_snapshot)

    ibkr_max_quote_age_ms = (
        safe_int(quote_snapshot.get("max_ibkr_quote_age_ms"))
        or safe_int(params.get("max_ibkr_quote_age_ms"))
        or 1_000
    )
    quote_snapshot["max_ibkr_quote_age_ms"] = ibkr_max_quote_age_ms
    quote_snapshot["maker_leg"] = _apply_quote_health_to_v4_leg(
        _normalize_v4_leg_snapshot(
            quote_snapshot.get("maker_leg") if isinstance(quote_snapshot.get("maker_leg"), Mapping) else None,
            maker_leg,
        ),
        leg_role="maker",
        max_quote_age_ms=safe_int(params.get("max_age_ms")) or 10_000,
    )
    quote_snapshot["hedge_leg"] = _apply_quote_health_to_v4_leg(
        _normalize_v4_leg_snapshot(
            quote_snapshot.get("hedge_leg") if isinstance(quote_snapshot.get("hedge_leg"), Mapping) else None,
            hedge_leg,
        ),
        leg_role="hedge",
        max_quote_age_ms=ibkr_max_quote_age_ms,
    )
    quote_snapshot["ref_leg"] = _apply_quote_health_to_v4_leg(
        _normalize_v4_leg_snapshot(
            quote_snapshot.get("ref_leg") if isinstance(quote_snapshot.get("ref_leg"), Mapping) else None,
            ref_leg,
        ),
        leg_role="reference",
        max_quote_age_ms=ibkr_max_quote_age_ms,
    )
    hedge_leg_id_text = decode_text(hedge_leg_id).strip()
    if hedge_leg_id_text:
        hedge_route = (
            decode_text(quote_snapshot.get("hedge_route")).strip().upper()
            or _ibkr_route_from_instrument_id_text(hedge_leg_id_text)
        )
        hedge_snapshot = quote_snapshot["hedge_leg"]
        ref_snapshot = quote_snapshot["ref_leg"]
        if not hedge_snapshot:
            synthesized: dict[str, Any] = {
                "instrument_id": hedge_leg_id_text,
            }
            ref_venue = (
                decode_text(ref_snapshot.get("venue")).strip().upper()
                if isinstance(ref_snapshot, Mapping)
                else ""
            )
            ref_symbol = (
                decode_text(ref_snapshot.get("symbol")).strip()
                if isinstance(ref_snapshot, Mapping)
                else ""
            )
            if ref_venue:
                synthesized["venue"] = ref_venue
            elif hedge_route:
                synthesized["venue"] = "IBKR"
            if ref_symbol:
                synthesized["symbol"] = ref_symbol
            if hedge_route:
                synthesized["route"] = hedge_route
            quote_snapshot["hedge_leg"] = synthesized
        else:
            if "instrument_id" not in hedge_snapshot:
                hedge_snapshot["instrument_id"] = hedge_leg_id_text
            if hedge_route and "route" not in hedge_snapshot:
                hedge_snapshot["route"] = hedge_route
            if isinstance(ref_snapshot, Mapping):
                ref_venue = decode_text(ref_snapshot.get("venue")).strip().upper()
                ref_symbol = decode_text(ref_snapshot.get("symbol")).strip()
                if ref_venue and "venue" not in hedge_snapshot:
                    hedge_snapshot["venue"] = ref_venue
            if ref_symbol and "symbol" not in hedge_snapshot:
                hedge_snapshot["symbol"] = ref_symbol
    live_quote_ts_ms = max(
        (
            candidate
            for candidate in (
                coerce_ts_ms(quote_snapshot["maker_leg"].get("ts_ms"))
                if isinstance(quote_snapshot.get("maker_leg"), Mapping)
                else None,
                coerce_ts_ms(quote_snapshot["hedge_leg"].get("ts_ms"))
                if isinstance(quote_snapshot.get("hedge_leg"), Mapping)
                else None,
                coerce_ts_ms(quote_snapshot["ref_leg"].get("ts_ms"))
                if isinstance(quote_snapshot.get("ref_leg"), Mapping)
                else None,
            )
            if candidate is not None
        ),
        default=None,
    )
    quote_snapshot["ts_ms"] = coerce_ts_ms(live_quote_ts_ms or quote_snapshot.get("ts_ms") or ts_ms)
    return quote_snapshot


def _derive_makerv4_operator_payload(
    *,
    strategy_family: str,
    state: Mapping[str, Any],
    params: Mapping[str, Any],
    quote_snapshot: Mapping[str, Any],
) -> dict[str, Any]:
    state_maker_v4 = state.get("maker_v4")
    state_v4 = state_maker_v4 if isinstance(state_maker_v4, Mapping) else {}
    hedge_policy = state_v4.get("hedge_policy")
    hedge_policy_map = hedge_policy if isinstance(hedge_policy, Mapping) else {}
    pending_hedge = state_v4.get("pending_hedge")
    pending_hedge_map = pending_hedge if isinstance(pending_hedge, Mapping) else {}
    hedge_backlog = state_v4.get("hedge_backlog")
    hedge_backlog_map = hedge_backlog if isinstance(hedge_backlog, Mapping) else {}
    hedge_leg = quote_snapshot.get("hedge_leg")
    hedge_leg_map = hedge_leg if isinstance(hedge_leg, Mapping) else {}

    execution_mode = (
        _first_valid_text(
            params.get("execution_mode"),
            state_v4.get("execution_mode"),
            quote_snapshot.get("execution_mode"),
        )
        or ("take_take" if strategy_family == "equities_taker" else "maker_hedge")
    ).lower()
    if execution_mode not in {"maker_hedge", "take_take"}:
        execution_mode = "take_take" if strategy_family == "equities_taker" else "maker_hedge"

    fee_assumptions = state_v4.get("fee_assumptions")
    fee_assumptions_map = fee_assumptions if isinstance(fee_assumptions, Mapping) else {}
    quote_fee_assumptions = quote_snapshot.get("fee_assumptions")
    quote_fee_assumptions_map = (
        quote_fee_assumptions if isinstance(quote_fee_assumptions, Mapping) else {}
    )
    maker_taker_fee_bps = _first_valid_float(
        fee_assumptions_map.get("maker_taker_fee_bps"),
        quote_fee_assumptions_map.get("maker_taker_fee_bps"),
        fee_assumptions_map.get("hl_taker_fee_bps"),
        quote_fee_assumptions_map.get("hl_taker_fee_bps"),
    )
    maker_maker_fee_bps = _first_valid_float(
        fee_assumptions_map.get("maker_maker_fee_bps"),
        quote_fee_assumptions_map.get("maker_maker_fee_bps"),
        fee_assumptions_map.get("hl_maker_fee_bps"),
        quote_fee_assumptions_map.get("hl_maker_fee_bps"),
    )
    if "cancel_after_ms" in pending_hedge_map:
        cancel_after_ms = safe_int(pending_hedge_map.get("cancel_after_ms"))
    elif "cancel_after_ms" in hedge_policy_map:
        cancel_after_ms = safe_int(hedge_policy_map.get("cancel_after_ms"))
    else:
        cancel_after_ms = _first_valid_int(quote_snapshot.get("cancel_after_ms"))

    fee_assumptions_payload = {
        "ibkr_fee_plan": _first_valid_text(
            fee_assumptions_map.get("ibkr_fee_plan"),
            quote_fee_assumptions_map.get("ibkr_fee_plan"),
        ),
        "ibkr_fee_min_usd": _first_valid_float(
            fee_assumptions_map.get("ibkr_fee_min_usd"),
            quote_fee_assumptions_map.get("ibkr_fee_min_usd"),
        ),
        "assumed_hedge_fee_bps": _first_valid_float(
            fee_assumptions_map.get("assumed_hedge_fee_bps"),
            quote_fee_assumptions_map.get("assumed_hedge_fee_bps"),
            quote_snapshot.get("assumed_hedge_fee_bps"),
        ),
    }
    if strategy_family in {"equities_maker", "equities_taker"}:
        fee_assumptions_payload["hl_taker_fee_bps"] = maker_taker_fee_bps
        fee_assumptions_payload["hl_maker_fee_bps"] = maker_maker_fee_bps
    else:
        fee_assumptions_payload["maker_taker_fee_bps"] = maker_taker_fee_bps
        fee_assumptions_payload["maker_maker_fee_bps"] = maker_maker_fee_bps

    payload = {
        "execution_mode": execution_mode,
        "behavior": "take_take" if execution_mode == "take_take" else "maker",
        "hedge_policy": {
            "route": _first_valid_upper_text(
                pending_hedge_map.get("route"),
                hedge_policy_map.get("route"),
                quote_snapshot.get("hedge_route"),
                hedge_leg_map.get("route"),
            ),
            "time_in_force": _first_valid_upper_text(
                pending_hedge_map.get("time_in_force"),
                hedge_policy_map.get("time_in_force"),
                quote_snapshot.get("time_in_force"),
            ),
            "outside_rth": _first_valid_bool(
                pending_hedge_map.get("outside_rth"),
                hedge_policy_map.get("outside_rth"),
                quote_snapshot.get("outside_rth"),
            ),
            "include_overnight": _first_valid_bool(
                pending_hedge_map.get("include_overnight"),
                hedge_policy_map.get("include_overnight"),
                quote_snapshot.get("include_overnight"),
            ),
            "cancel_after_ms": cancel_after_ms,
        },
        "fee_assumptions": fee_assumptions_payload,
        "hedge_backlog": None,
    }
    if hedge_backlog_map:
        payload["hedge_backlog"] = {
            "fill_id": _first_valid_text(hedge_backlog_map.get("fill_id")),
            "side": _first_valid_upper_text(hedge_backlog_map.get("side")),
            "requested_qty": _first_valid_text(hedge_backlog_map.get("requested_qty")),
            "blocked_reason": _first_valid_text(hedge_backlog_map.get("blocked_reason")),
            "fill_ts_ms": _first_valid_int(hedge_backlog_map.get("fill_ts_ms")),
            "maker_fee_bps": _first_valid_float(hedge_backlog_map.get("maker_fee_bps")),
        }
    return payload


def _maker_v4_quote_health_blocks_new_risk(
    *,
    state: Mapping[str, Any],
    quote_snapshot: Mapping[str, Any],
) -> bool:
    state_maker_v4 = state.get("maker_v4")
    state_v4 = state_maker_v4 if isinstance(state_maker_v4, Mapping) else {}
    hedge_backlog = state_v4.get("hedge_backlog")
    if isinstance(hedge_backlog, Mapping) and hedge_backlog:
        return True

    maker_leg = quote_snapshot.get("maker_leg")
    ref_leg = quote_snapshot.get("ref_leg")
    hedge_leg = quote_snapshot.get("hedge_leg")
    maker_leg_map = maker_leg if isinstance(maker_leg, Mapping) else {}
    ref_leg_map = ref_leg if isinstance(ref_leg, Mapping) else {}
    hedge_leg_map = hedge_leg if isinstance(hedge_leg, Mapping) else {}

    if _first_valid_bool(maker_leg_map.get("pricing_usable")) is False:
        return True
    if _first_valid_bool(ref_leg_map.get("pricing_usable")) is False:
        return True
    if _first_valid_bool(ref_leg_map.get("hedge_usable")) is False:
        return True
    if _first_valid_bool(hedge_leg_map.get("hedge_usable")) is False:
        return True
    return False


def _maker_v4_can_render_from_quote_snapshot_while_paused(
    *,
    state: Mapping[str, Any],
    quote_snapshot: Mapping[str, Any] | None,
    bot_on: bool,
    maker_leg: Mapping[str, Any] | None,
    ref_leg: Mapping[str, Any] | None,
) -> bool:
    if bot_on:
        return False
    if not isinstance(quote_snapshot, Mapping):
        return False
    live_maker_ts_ms = coerce_ts_ms(maker_leg.get("ts_ms")) if isinstance(maker_leg, Mapping) else None
    live_ref_ts_ms = coerce_ts_ms(ref_leg.get("ts_ms")) if isinstance(ref_leg, Mapping) else None
    if live_maker_ts_ms is None:
        return False
    if live_ref_ts_ms is None:
        return False
    return not _maker_v4_quote_health_blocks_new_risk(
        state=state,
        quote_snapshot=quote_snapshot,
    )


def _derive_quote_snapshot(
    *,
    state: Mapping[str, Any],
    params: Mapping[str, Any],
    bot_on: bool,
    ts_ms: int | None,
    maker_leg: Mapping[str, Any] | None,
    ref_leg: Mapping[str, Any] | None,
) -> dict[str, Any]:
    """Derive a stable quote snapshot from state payloads and live leg data.

    Operator-facing maker quote rows, spread, and effective edges are intended
    to come from one quote-snapshot epoch. Downstream renderers should treat
    this object as the pricing source of truth rather than mixing in other
    live-leg values opportunistically.
    """

    state_maker_v3 = state.get("maker_v3")
    quote_snapshot = {}
    if isinstance(state_maker_v3, Mapping):
        raw_quote_snapshot = state_maker_v3.get("quote_snapshot")
        if isinstance(raw_quote_snapshot, Mapping):
            quote_snapshot = dict(raw_quote_snapshot)

    pricing, _ = _state_pricing_debug(state)
    pricing_ts_ms = coerce_ts_ms(pricing.get("ts_ms"))
    mode = "ON" if bot_on else "OFF"
    reason = decode_text(state.get("state"))
    quote_snapshot["mode"] = decode_text(quote_snapshot.get("mode")).strip() or mode
    quote_snapshot["reason"] = decode_text(quote_snapshot.get("reason")).strip() or reason
    quote_snapshot["ts_ms"] = coerce_ts_ms(pricing_ts_ms or quote_snapshot.get("ts_ms") or ts_ms)

    raw_maker_bid = _first_valid_float(quote_snapshot.get("maker_top_bid"), quote_snapshot.get("bid"))
    raw_maker_ask = _first_valid_float(quote_snapshot.get("maker_top_ask"), quote_snapshot.get("ask"))
    raw_ref_bid = _first_valid_float(quote_snapshot.get("ref_bid"))
    raw_ref_ask = _first_valid_float(quote_snapshot.get("ref_ask"))
    raw_place_bid = _first_valid_float(quote_snapshot.get("place_bid"))
    raw_place_ask = _first_valid_float(quote_snapshot.get("place_ask"))
    raw_cancel_bid = _first_valid_float(quote_snapshot.get("cancel_bid"))
    raw_cancel_ask = _first_valid_float(quote_snapshot.get("cancel_ask"))

    has_snapshot_pricing = any(
        _first_valid_float(
            pricing.get(key),
            quote_snapshot.get(key),
        )
        is not None
        for key in (
            "maker_top_bid",
            "maker_top_ask",
            "ref_bid",
            "ref_ask",
            "place_bid",
            "place_ask",
            "cancel_bid",
            "cancel_ask",
            "bid_edge1_eff_bps",
            "ask_edge1_eff_bps",
        )
    )

    maker_bid = _first_valid_float(pricing.get("maker_top_bid"), raw_maker_bid)
    maker_ask = _first_valid_float(pricing.get("maker_top_ask"), raw_maker_ask)
    ref_bid = _first_valid_float(pricing.get("ref_bid"), raw_ref_bid)
    ref_ask = _first_valid_float(pricing.get("ref_ask"), raw_ref_ask)
    place_bid = _first_valid_float(pricing.get("place_bid"), raw_place_bid)
    place_ask = _first_valid_float(pricing.get("place_ask"), raw_place_ask)
    cancel_bid = _first_valid_float(pricing.get("cancel_bid"), raw_cancel_bid)
    cancel_ask = _first_valid_float(pricing.get("cancel_ask"), raw_cancel_ask)

    if not has_snapshot_pricing:
        maker_bid = _first_valid_float(
            maker_bid,
            maker_leg.get("bid") if isinstance(maker_leg, Mapping) else None,
        )
        maker_ask = _first_valid_float(
            maker_ask,
            maker_leg.get("ask") if isinstance(maker_leg, Mapping) else None,
        )
        ref_bid = _first_valid_float(
            ref_bid,
            ref_leg.get("bid") if isinstance(ref_leg, Mapping) else None,
        )
        ref_ask = _first_valid_float(
            ref_ask,
            ref_leg.get("ask") if isinstance(ref_leg, Mapping) else None,
        )

    eff_bid = _first_valid_float(pricing.get("bid_edge1_eff_bps"))
    eff_ask = _first_valid_float(pricing.get("ask_edge1_eff_bps"))
    place_edge = _first_valid_float(
        pricing.get("place_edge_bps"),
        quote_snapshot.get("place_edge_bps"),
        params.get("place_edge1"),
    )

    if isinstance(maker_leg, Mapping):
        quote_snapshot["maker_exchange"] = (
            decode_text(maker_leg.get("exchange")).strip().lower() or None
        )
        quote_snapshot["maker_symbol"] = decode_text(maker_leg.get("symbol")).strip() or None
        quote_snapshot["maker_top_ts_ms"] = coerce_ts_ms(maker_leg.get("ts_ms"))
    if isinstance(ref_leg, Mapping):
        quote_snapshot["ref_exchange"] = (
            decode_text(ref_leg.get("exchange")).strip().lower() or None
        )
        quote_snapshot["ref_symbol"] = decode_text(ref_leg.get("symbol")).strip() or None
        quote_snapshot["ref_ts_ms"] = coerce_ts_ms(ref_leg.get("ts_ms"))
    quote_snapshot["ref_source"] = (
        decode_text(pricing.get("anchor_source")).strip() or "reference_leg"
    )

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


def _quote_snapshot_raw_market_vs_ref_mid_bps(quote_snapshot: Mapping[str, Any] | None) -> float | None:
    if not isinstance(quote_snapshot, Mapping):
        return None
    maker_bid = _first_valid_float(
        quote_snapshot.get("maker_top_bid"),
        quote_snapshot.get("bid"),
    )
    maker_ask = _first_valid_float(
        quote_snapshot.get("maker_top_ask"),
        quote_snapshot.get("ask"),
    )
    ref_bid = _first_valid_float(quote_snapshot.get("ref_bid"))
    ref_ask = _first_valid_float(quote_snapshot.get("ref_ask"))
    if (
        maker_bid is None
        or maker_ask is None
        or ref_bid is None
        or ref_ask is None
    ):
        return None
    ref_mid = (ref_bid + ref_ask) / 2.0
    if not ref_mid:
        return None
    market_mid = (maker_bid + maker_ask) / 2.0
    return ((market_mid - ref_mid) / ref_mid) * 10_000.0


def _quote_snapshot_quoted_mid_vs_ref_mid_bps(quote_snapshot: Mapping[str, Any] | None) -> float | None:
    if not isinstance(quote_snapshot, Mapping):
        return None
    place_bid = _first_valid_float(quote_snapshot.get("place_bid"))
    place_ask = _first_valid_float(quote_snapshot.get("place_ask"))
    ref_bid = _first_valid_float(quote_snapshot.get("ref_bid"))
    ref_ask = _first_valid_float(quote_snapshot.get("ref_ask"))
    if (
        place_bid is None
        or place_ask is None
        or ref_bid is None
        or ref_ask is None
    ):
        return None
    ref_mid = (ref_bid + ref_ask) / 2.0
    if not ref_mid:
        return None
    market_mid = (place_bid + place_ask) / 2.0
    return ((market_mid - ref_mid) / ref_mid) * 10_000.0


def _top_level_signal_ts_ms(
    *,
    state: Mapping[str, Any],
    quote_snapshot: Mapping[str, Any] | None,
    fallback_ts_ms: int | None,
) -> int | None:
    if isinstance(quote_snapshot, Mapping):
        quoted_ts_ms = coerce_ts_ms(quote_snapshot.get("ts_ms"))
        if quoted_ts_ms is not None:
            return quoted_ts_ms
    return coerce_ts_ms(state.get("ts_ms") or state.get("ts_event") or fallback_ts_ms)


def _top_level_signal_mode(
    *,
    quote_snapshot: Mapping[str, Any] | None,
    bot_on: bool,
) -> str:
    if isinstance(quote_snapshot, Mapping):
        mode = decode_text(quote_snapshot.get("mode")).strip().upper()
        if mode:
            return mode
    return "ON" if bot_on else "OFF"


def _top_level_signal_reason(
    *,
    state: Mapping[str, Any],
    quote_snapshot: Mapping[str, Any] | None,
) -> str | None:
    if isinstance(quote_snapshot, Mapping):
        reason = decode_text(quote_snapshot.get("reason")).strip()
        if reason:
            return reason
    reason = decode_text(state.get("state")).strip()
    return reason or None


def _top_level_signal_signed_skew_bps(
    *,
    pricing_adjustments: Sequence[dict[str, Any]],
    quote_snapshot: Mapping[str, Any] | None,
) -> float | None:
    inventory_adjustment = _first_inventory_skew_adjustment(pricing_adjustments)
    return _first_valid_float(
        inventory_adjustment.get("skew_bps_signed"),
        quote_snapshot.get("skew_bps_signed") if isinstance(quote_snapshot, Mapping) else None,
    )


def build_signals_payload_impl(
    *,
    strategy_id: str,
    metadata: StrategyMetadata,
    state: dict[str, Any],
    fv_row: dict[str, Any],
    params: dict[str, Any],
    balances: list[dict[str, Any]],
    legs: dict[str, Any],
    running: bool | None,
    now_ms_fn: Callable[[], int],
) -> dict[str, Any]:
    parsed_bot_on = safe_bool(state.get("bot_on"))
    bot_on = bool(parsed_bot_on if parsed_bot_on is not None else state.get("bot_on", False))
    managed = safe_int(state.get("managed_orders")) or 0
    ts_ms = coerce_ts_ms(state.get("ts_ms") or state.get("ts_event"))
    legs_order = list(legs.keys())
    role_map = _role_map_for_signal(state=state, legs_order=legs_order, legs=legs)
    raw_role_map = state.get("maker_role_map")
    raw_hedge_leg_id = (
        decode_text(raw_role_map.get("hedge_leg")).strip()
        if isinstance(raw_role_map, Mapping)
        else ""
    )

    maker_leg = legs.get(role_map.get("maker_leg") or "") if role_map.get("maker_leg") else None
    ref_leg = legs.get(role_map.get("ref_leg") or "") if role_map.get("ref_leg") else None
    hedge_leg = legs.get(role_map.get("hedge_leg") or "") if role_map.get("hedge_leg") else None
    if not isinstance(maker_leg, Mapping):
        maker_leg = None
    if not isinstance(ref_leg, Mapping):
        ref_leg = None
    if not isinstance(hedge_leg, Mapping):
        hedge_leg = None
    hedge_leg_id = decode_text(role_map.get("hedge_leg")).strip()
    raw_role_map = state.get("maker_role_map")
    if not hedge_leg_id and isinstance(raw_role_map, Mapping):
        hedge_leg_id = decode_text(raw_role_map.get("hedge_leg")).strip()
    if hedge_leg is None and hedge_leg_id and isinstance(ref_leg, Mapping):
        hedge_leg = dict(ref_leg)
        hedge_leg["instrument_id"] = hedge_leg_id

    leg_a = (
        maker_leg if maker_leg is not None else (legs.get(legs_order[0]) if legs_order else None)
    )
    leg_b = (
        ref_leg
        if ref_leg is not None
        else (legs.get(legs_order[1]) if len(legs_order) > 1 else None)
    )
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

    spread_case1 = _first_valid_float(
        state.get("spread_net_case1_bps"),
        fv_row.get("spread_net_case1_bps"),
        spread_case1_derived,
    )
    spread_case2 = _first_valid_float(
        state.get("spread_net_case2_bps"),
        fv_row.get("spread_net_case2_bps"),
        spread_case2_derived,
    )

    best_case = _normalize_best_case(
        state.get("spread_net_best_case") or fv_row.get("spread_net_best_case"),
    )
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

    explicit_decision_edge_bps = _first_valid_float(
        state.get("decision_edge_bps"),
        fv_row.get("decision_edge_bps"),
    )
    decision_edge_bps = _first_valid_float(explicit_decision_edge_bps, spread_net_bps)

    required_case1, required_case2 = _required_edges_by_case(params)
    required_edge_bps = _first_valid_float(
        state.get("required_edge_bps"),
        fv_row.get("required_edge_bps"),
    )
    if required_edge_bps is None:
        if best_case == "case1":
            required_edge_bps = required_case1
        elif best_case == "case2":
            required_edge_bps = required_case2

    explicit_edge2_bps = _first_valid_float(state.get("edge2_bps"), fv_row.get("edge2_bps"))
    edge2_bps = explicit_edge2_bps
    if edge2_bps is None and decision_edge_bps is not None and required_edge_bps is not None:
        edge2_bps = decision_edge_bps - required_edge_bps

    risk_delta = _first_valid_float(state.get("risk_delta"), fv_row.get("risk_delta"))
    if risk_delta is None:
        risk_delta = _risk_delta_from_balances(balances)
    risk_delta_ts_ms = coerce_ts_ms(
        state.get("risk_delta_ts_ms") or fv_row.get("risk_delta_ts_ms") or ts_ms,
    )

    pricing_adjustments = _derive_pricing_adjustments(
        state=state,
        params=params,
        ts_ms=ts_ms,
        risk_delta=risk_delta,
    )
    inventory_fields = _project_signal_inventory_fields(
        state=state,
        pricing_adjustments=pricing_adjustments,
    )
    strategy_family = metadata.strategy_family or _derive_strategy_family(metadata.strategy_class)
    uses_equities_arb_contract = _uses_equities_arb_contract(strategy_family)
    quote_snapshot = (
        _derive_quote_snapshot_v4(
            state=state,
            params=params,
            ts_ms=ts_ms,
            maker_leg=maker_leg,
            hedge_leg=hedge_leg,
            ref_leg=ref_leg,
            hedge_leg_id=raw_hedge_leg_id or role_map.get("hedge_leg"),
        )
        if uses_equities_arb_contract
        else _derive_quote_snapshot(
            state=state,
            params=params,
            bot_on=bot_on,
            ts_ms=ts_ms,
            maker_leg=maker_leg,
            ref_leg=ref_leg,
        )
    )

    if strategy_family == "maker_v3":
        raw_spread_bps = _quote_snapshot_raw_market_vs_ref_mid_bps(quote_snapshot)
        if raw_spread_bps is not None:
            spread_net_bps = raw_spread_bps
        quote_spread_bps = _quote_snapshot_quoted_mid_vs_ref_mid_bps(quote_snapshot)
        if quote_spread_bps is not None and explicit_decision_edge_bps is None:
            decision_edge_bps = quote_spread_bps
            if explicit_edge2_bps is None and required_edge_bps is not None:
                edge2_bps = decision_edge_bps - required_edge_bps

    state_quote_status = state.get("maker_quote_status")
    maker_quote_status = (
        dict(state_quote_status) if isinstance(state_quote_status, Mapping) else None
    )
    state_quote_stacks = state.get("quote_stacks")
    quote_stacks = dict(state_quote_stacks) if isinstance(state_quote_stacks, Mapping) else None
    state_balance_readiness = state.get("balance_readiness")
    balance_readiness = (
        dict(state_balance_readiness) if isinstance(state_balance_readiness, Mapping) else None
    )
    state_quote_blockers = state.get("quote_blockers")
    quote_blockers = (
        [dict(row) for row in state_quote_blockers if isinstance(row, Mapping)]
        if isinstance(state_quote_blockers, list)
        else []
    )

    state_name = decode_text(state.get("state")).strip().lower()
    state_blocked = state_name.startswith("blocked_")
    tradeable = bot_on and not state_blocked
    near_tradeable = False
    blocked = (not bot_on) or state_blocked
    def _is_side_local_quote_blocker(blocker: Mapping[str, Any]) -> bool:
        blocked_side = decode_text(blocker.get("blocked_side")).strip().upper()
        if blocked_side not in {"BUY", "SELL"}:
            return False
        if maker_quote_status is None:
            return False
        opposite_prefix = "ask" if blocked_side == "BUY" else "bid"
        opposite_open = safe_int(maker_quote_status.get(f"{opposite_prefix}_open")) or 0
        opposite_depth = safe_int(maker_quote_status.get(f"{opposite_prefix}_depth")) or 0
        opposite_blocked = safe_int(maker_quote_status.get(f"{opposite_prefix}_blocked")) or 0
        return max(opposite_open, opposite_depth - opposite_blocked) > 0

    blocking_quote_blockers = [
        row
        for row in quote_blockers
        if decode_text(row.get("reason_code")).strip().lower() not in {"", "pending_cancel_in_flight"}
        and not _is_side_local_quote_blocker(row)
    ]
    if blocking_quote_blockers:
        tradeable = False
        blocked = True

    top_level_ts_ms = _top_level_signal_ts_ms(
        state=state,
        quote_snapshot=quote_snapshot,
        fallback_ts_ms=ts_ms,
    )
    md_health: dict[str, Any] = {
        "legs_count": len(legs),
        "stale_legs": sorted(
            contract_id for contract_id, row in legs.items() if safe_int(row.get("age_ms")) is None
        ),
    }
    if ts_ms is not None:
        md_health["strategy_state_age_ms"] = max(0, now_ms_fn() - ts_ms)

    explicit_quote_snapshot = isinstance(state.get("maker_v3"), Mapping) and isinstance(
        cast(Mapping[str, Any], state.get("maker_v3")).get("quote_snapshot"),
        Mapping,
    )
    explicit_quote_snapshot = explicit_quote_snapshot or (
        isinstance(state.get("maker_v4"), Mapping)
        and isinstance(cast(Mapping[str, Any], state.get("maker_v4")).get("quote_snapshot"), Mapping)
    )
    explicit_quote_snapshot = explicit_quote_snapshot or (
        isinstance(state.get("equities_arb"), Mapping)
        and isinstance(
            cast(Mapping[str, Any], state.get("equities_arb")).get("quote_snapshot"),
            Mapping,
        )
    )
    has_partial_strategy_state = bool(
        pricing_adjustments
        or explicit_quote_snapshot
        or state_name
        or managed
        or parsed_bot_on is not None
    )

    state_stale = False
    leg_clock_ts_ms = max(
        (
            leg_ts_ms
            for row in legs.values()
            if (leg_ts_ms := coerce_ts_ms(row.get("ts_ms"))) is not None
        ),
        default=None,
    )
    if top_level_ts_ms is not None and leg_clock_ts_ms is not None:
        top_level_age_ms = max(0, leg_clock_ts_ms - top_level_ts_ms)
        md_health["signal_state_age_ms"] = top_level_age_ms
        state_stale = top_level_age_ms >= 30_000
    elif top_level_ts_ms is not None:
        top_level_age_ms = max(0, now_ms_fn() - top_level_ts_ms)
        md_health["signal_state_age_ms"] = top_level_age_ms
        state_stale = top_level_age_ms >= 30_000
    elif top_level_ts_ms is None and has_partial_strategy_state:
        state_stale = True

    suppress_state_stale_block = (
        strategy_family == "maker_v4"
        and _maker_v4_can_render_from_quote_snapshot_while_paused(
            state=state,
            quote_snapshot=quote_snapshot,
            bot_on=bot_on,
            maker_leg=maker_leg,
            ref_leg=ref_leg,
        )
    )

    if state_stale and not suppress_state_stale_block:
        managed = 0
        tradeable = False
        blocked = True
        maker_quote_status = {
            "bid_open": 0,
            "ask_open": 0,
            "bid_depth": 0,
            "ask_depth": 0,
            "bid_blocked": 0,
            "ask_blocked": 0,
        }
    md_health["state_stale"] = state_stale

    if state_stale and not suppress_state_stale_block:
        top_level_mode = "STALE"
        top_level_reason = "stale_state"
        top_level_skew_bps = None
    else:
        top_level_mode = _top_level_signal_mode(
            quote_snapshot=quote_snapshot,
            bot_on=bot_on,
        )
        if suppress_state_stale_block and not bot_on:
            top_level_reason = "bot_off"
        else:
            top_level_reason = _top_level_signal_reason(
                state=state,
                quote_snapshot=quote_snapshot,
            )
        top_level_skew_bps = _top_level_signal_signed_skew_bps(
            pricing_adjustments=pricing_adjustments,
            quote_snapshot=quote_snapshot,
        )

    if uses_equities_arb_contract and _maker_v4_quote_health_blocks_new_risk(
        state=state,
        quote_snapshot=quote_snapshot,
    ):
        tradeable = False
        blocked = True

    equities_arb_payload = (
        {
            "quote_snapshot": quote_snapshot,
            "operator": _derive_makerv4_operator_payload(
                strategy_family=strategy_family,
                state=state,
                params=params,
                quote_snapshot=quote_snapshot,
            ),
        }
        if uses_equities_arb_contract
        else None
    )

    return {
        "id": strategy_id,
        "meta": metadata.as_payload(strategy_id=strategy_id),
        "running": running,
        "strategy_family": strategy_family,
        "ts_ms": top_level_ts_ms,
        "mode": top_level_mode,
        "reason": top_level_reason,
        "skew_bps_signed": top_level_skew_bps,
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
        **inventory_fields,
        **({"equities_arb": equities_arb_payload} if uses_equities_arb_contract else {}),
        **({"maker_v4": equities_arb_payload} if strategy_family == "maker_v4" else {}),
        **({"maker_v3": {"quote_snapshot": quote_snapshot}} if not uses_equities_arb_contract else {}),
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
