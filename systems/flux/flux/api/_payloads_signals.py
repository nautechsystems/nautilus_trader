from __future__ import annotations

"""Leg and signal payload assembly helpers."""

from collections.abc import Callable
from collections.abc import Mapping
from collections.abc import Sequence
from typing import Any

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

    global_qty = _first_valid_float(
        skew.get("global_inventory_qty"),
        skew.get("inventory_qty"),
        skew.get("global_qty"),
    )
    if global_qty is not None:
        adjustment["global_qty"] = global_qty
        adjustment["curr_qty"] = global_qty
    local_qty = _first_valid_float(
        skew.get("local_inventory_qty"),
        skew.get("local_qty"),
    )
    if local_qty is not None:
        adjustment["local_qty"] = local_qty

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
            merged.append(item)
            continue
        merged_item = dict(fallback_inventory)
        for key, value in item.items():
            if value is not None:
                merged_item[key] = value
        merged.append(merged_item)
        merged_inventory = True

    if not merged_inventory:
        merged.append(fallback_inventory)
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
        normalized = [dict(item) for item in state_adjustments if isinstance(item, Mapping)]
        fallback = _build_fallback_inventory_skew_adjustments(
            state=state,
            params=params,
            ts_ms=ts_ms,
            risk_delta=risk_delta,
        )
        return _merge_inventory_skew_adjustments(current=normalized, fallback=fallback)

    return _build_fallback_inventory_skew_adjustments(
        state=state,
        params=params,
        ts_ms=ts_ms,
        risk_delta=risk_delta,
    )


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
    """Derive a stable quote snapshot from state payloads and live leg data."""

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


def build_signals_payload_impl(
    *,
    strategy_id: str,
    metadata: StrategyMetadata,
    state: dict[str, Any],
    fv_row: dict[str, Any],
    params: dict[str, Any],
    balances: list[dict[str, Any]],
    legs: dict[str, Any],
    now_ms_fn: Callable[[], int],
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

    decision_edge_bps = _first_valid_float(
        state.get("decision_edge_bps"),
        fv_row.get("decision_edge_bps"),
        spread_net_bps,
    )

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

    edge2_bps = _first_valid_float(state.get("edge2_bps"), fv_row.get("edge2_bps"))
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
    quote_snapshot = _derive_quote_snapshot(
        state=state,
        params=params,
        bot_on=bot_on,
        ts_ms=ts_ms,
        maker_leg=maker_leg,
        ref_leg=ref_leg,
    )

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

    tradeable = bot_on
    near_tradeable = False
    blocked = not bot_on

    md_health: dict[str, Any] = {
        "legs_count": len(legs),
        "stale_legs": sorted(
            contract_id for contract_id, row in legs.items() if safe_int(row.get("age_ms")) is None
        ),
    }
    if ts_ms is not None:
        state_age_ms = max(0, now_ms_fn() - ts_ms)
        md_health["strategy_state_age_ms"] = state_age_ms
        live_legs = any(safe_int(row.get("age_ms")) is not None for row in legs.values())
        state_stale = state_age_ms >= 30_000 and not live_legs
        if state_stale:
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

    return {
        "id": strategy_id,
        "meta": metadata.as_payload(strategy_id=strategy_id),
        "strategy_family": metadata.strategy_family or _derive_strategy_family(metadata.strategy_class),
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
