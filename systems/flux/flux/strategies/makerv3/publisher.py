"""
Publish and serialize MakerV3 strategy observability payloads.
"""

from __future__ import annotations

import json
from collections.abc import Sequence
from collections.abc import Mapping
from contextlib import suppress
from decimal import Decimal
from typing import Any

from flux.api.payloads import contract_id_for_leg
from flux.api.payloads import decode_text
from flux.common.quantity_units import exposure_from_venue_qty
from flux.strategies.shared.publisher_common import build_role_map_payload
from flux.strategies.makerv3 import inventory as inventory_mod
from flux.strategies.makerv3 import pricing as pricing_mod
from flux.strategies.makerv3 import runtime_params as runtime_params_mod
from flux.strategies.makerv3.constants import BLOCKED_STATE_PREFIX
from flux.strategies.makerv3.constants import TOPIC_ALERT
from flux.strategies.makerv3.constants import TOPIC_BALANCES
from flux.strategies.makerv3.constants import TOPIC_EVENT
from flux.strategies.makerv3.constants import TOPIC_MARKET_BBO
from flux.strategies.makerv3.constants import TOPIC_STATE
from flux.events import FluxBusPayload
from nautilus_trader.model.enums import OrderSide


_to_decimal_or_none = pricing_mod.to_decimal_or_none


def to_json_safe(payload: Any) -> str:
    """
    Return deterministic compact JSON for message bus publishing.
    """
    return json.dumps(payload, sort_keys=True, separators=(",", ":"))


def _stringify_identifier(value: Any) -> str:
    if value is None:
        return ""
    to_str = getattr(value, "to_str", None)
    if callable(to_str):
        with suppress(Exception):
            return str(to_str())
    code = getattr(value, "code", None)
    if code is not None:
        return str(code)
    return str(value)


def _money_to_text(value: Any) -> str:
    if value is None:
        return "0"
    as_decimal = getattr(value, "as_decimal", None)
    if callable(as_decimal):
        with suppress(Exception):
            return str(as_decimal())
    return str(value)


def _json_safe_or_none(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, dict):
        return None
    try:
        return json.loads(to_json_safe(value))
    except Exception:
        return None
    try:
        return json.loads(to_json_safe(value))
    except Exception:
        return None


def _strategy_cache(strategy: Any) -> Any | None:
    cache = getattr(strategy, "_cache", None)
    if cache is None:
        cache = getattr(strategy, "cache", None)
    return cache


def _resolve_instrument(strategy: Any, instrument_id: Any) -> Any | None:
    instruments = getattr(strategy, "_instruments", {})
    instrument = instruments.get(instrument_id) if isinstance(instruments, dict) else None
    if instrument is not None:
        return instrument

    cache = _strategy_cache(strategy)
    instrument_lookup = getattr(cache, "instrument", None)
    if callable(instrument_lookup):
        with suppress(Exception):
            return instrument_lookup(instrument_id)
    return None


def _contract_role_id(strategy: Any, instrument_id: Any) -> str:
    instrument = _resolve_instrument(strategy, instrument_id)
    instrument_text = _stringify_identifier(instrument_id).strip().upper()
    exchange = _stringify_identifier(getattr(instrument_id, "venue", None)).strip().lower()
    symbol = _stringify_identifier(getattr(instrument, "raw_symbol", ""))
    base = _stringify_identifier(getattr(instrument, "base_currency", None)).upper()
    quote = _stringify_identifier(getattr(instrument, "quote_currency", None)).upper()
    if (not base or not quote) and symbol:
        parsed_base, parsed_quote = inventory_mod.normalize_contract_symbol(symbol)
        base = base or parsed_base
        quote = quote or parsed_quote
    if not exchange:
        return ""
    if not symbol:
        symbol = instrument_text.split(".", maxsplit=1)[0]
    pair = f"{base}/{quote}" if base and quote else symbol
    return contract_id_for_leg(exchange=exchange, symbol=pair, instrument_id=instrument_text)


def _maker_role_map_payload(strategy: Any) -> dict[str, str]:
    maker_leg = _contract_role_id(strategy, strategy.config.maker_instrument_id)
    ref_leg = _contract_role_id(strategy, strategy.config.reference_instrument_id)
    return build_role_map_payload(maker_leg=maker_leg, ref_leg=ref_leg)


def _matching_base_positions(
    strategy: Any,
    positions: list[Any],
    *,
    base_currency: str | None,
) -> list[Any]:
    maker_instrument_id = getattr(getattr(strategy, "config", None), "maker_instrument_id", None)
    if maker_instrument_id is not None:
        exact_matches = [
            position
            for position in positions
            if getattr(position, "instrument_id", None) == maker_instrument_id
        ]
        return exact_matches

    if not base_currency:
        return []

    filtered: list[Any] = []
    for position in positions:
        if inventory_mod.position_matches_base_currency(
            position,
            base_currency=base_currency,
            instrument_lookup=lambda instrument_id: _resolve_instrument(strategy, instrument_id),
        ):
            filtered.append(position)
    return filtered


def _effective_inventory_positions(strategy: Any, positions: Sequence[Any]) -> list[Any]:
    cache = _strategy_cache(strategy)
    orders_for_position = getattr(cache, "orders_for_position", None)
    return inventory_mod.effective_inventory_positions(
        positions,
        order_lookup=orders_for_position if callable(orders_for_position) else None,
    )


def _account_balances_rows(account: Any) -> list[dict[str, str]]:
    balances_total_fn = getattr(account, "balances_total", None)
    balances_free_fn = getattr(account, "balances_free", None)
    balances_locked_fn = getattr(account, "balances_locked", None)

    if not callable(balances_total_fn):
        return []

    try:
        balances_total = balances_total_fn()
    except Exception:
        return []

    if not isinstance(balances_total, dict):
        return []

    try:
        balances_free = balances_free_fn() if callable(balances_free_fn) else {}
    except Exception:
        balances_free = {}

    try:
        balances_locked = balances_locked_fn() if callable(balances_locked_fn) else {}
    except Exception:
        balances_locked = {}

    rows: list[dict[str, str]] = []
    for currency in sorted(balances_total.keys(), key=_stringify_identifier):
        code = _stringify_identifier(currency).upper()
        rows.append(
            {
                "currency": code,
                "free": _money_to_text(balances_free.get(currency)),
                "locked": _money_to_text(balances_locked.get(currency)),
                "total": _money_to_text(balances_total.get(currency)),
            },
        )

    return rows


def _serialize_account_payload(account: Any) -> dict[str, Any]:
    acct_to_dict = getattr(account, "to_dict", None)
    if callable(acct_to_dict):
        with suppress(Exception):
            candidate = _json_safe_or_none(acct_to_dict())
            if candidate is not None:
                return candidate

        with suppress(Exception):
            candidate = _json_safe_or_none(acct_to_dict(account))
            if candidate is not None:
                return candidate

    account_id = _stringify_identifier(getattr(account, "id", None))
    if not account_id:
        account_id = _stringify_identifier(
            getattr(getattr(account, "last_event", None), "account_id", None),
        )

    balances = _account_balances_rows(account)
    payload: dict[str, Any] = {"type": type(account).__name__}
    if account_id:
        payload["account_id"] = account_id
    if balances:
        payload["events"] = [{"account_id": account_id, "balances": balances}]
    if len(payload) == 1:
        payload["repr"] = repr(account)

    return payload


def _position_qty_payload(
    strategy: Any,
    *,
    instrument_id: Any,
    signed_qty: Any,
    avg_px_open: Any = None,
) -> dict[str, Any]:
    signed_qty_dec = _to_decimal_or_none(signed_qty)
    if signed_qty_dec is None:
        return {}

    payload = {
        "signed_qty_venue": decimal_to_json_str(signed_qty_dec),
        "quantity_venue": decimal_to_json_str(abs(signed_qty_dec)),
    }
    instrument = _resolve_instrument(strategy, instrument_id)
    if instrument is None:
        return payload

    shared_last_px = avg_px_open
    shared_last_px_lookup = getattr(strategy, "_inventory_base_exposure_last_px", None)
    if callable(shared_last_px_lookup):
        with suppress(Exception):
            latest_last_px = shared_last_px_lookup()
            if latest_last_px is not None:
                shared_last_px = latest_last_px

    with suppress(Exception):
        exposure = exposure_from_venue_qty(instrument, signed_qty_dec, last_px=shared_last_px)
        payload["qty_conversion_status"] = exposure.qty_conversion_status
        payload["qty_conversion_source"] = exposure.qty_conversion_source
        if exposure.base_qty is not None:
            payload["signed_qty_base"] = decimal_to_json_str(exposure.base_qty)
            payload["quantity_base"] = decimal_to_json_str(abs(exposure.base_qty))
    return payload


def _augment_position_payload(
    strategy: Any,
    payload: dict[str, Any],
    *,
    instrument_id: Any = None,
    signed_qty: Any = None,
    avg_px_open: Any = None,
) -> dict[str, Any]:
    instrument_id = instrument_id if instrument_id is not None else payload.get("instrument_id")
    if not instrument_id:
        return payload
    payload.update(
        _position_qty_payload(
            strategy,
            instrument_id=instrument_id,
            signed_qty=payload.get("signed_qty") if signed_qty is None else signed_qty,
            avg_px_open=payload.get("avg_px_open") if avg_px_open is None else avg_px_open,
        ),
    )
    return payload


def _serialize_position_payload(strategy: Any, position: Any) -> dict[str, Any]:
    if isinstance(position, Mapping):
        candidate = _json_safe_or_none(dict(position))
        if candidate is not None:
            return _augment_position_payload(
                strategy,
                candidate,
                instrument_id=position.get("instrument_id"),
                signed_qty=position.get("signed_qty"),
                avg_px_open=position.get("avg_px_open"),
            )

    pos_to_dict = getattr(position, "to_dict", None)
    if callable(pos_to_dict):
        with suppress(Exception):
            candidate = _json_safe_or_none(pos_to_dict())
            if candidate is not None:
                return _augment_position_payload(
                    strategy,
                    candidate,
                    instrument_id=getattr(position, "instrument_id", None),
                    signed_qty=getattr(position, "signed_qty", None),
                    avg_px_open=getattr(position, "avg_px_open", None),
                )

        with suppress(Exception):
            candidate = _json_safe_or_none(pos_to_dict(position))
            if candidate is not None:
                return _augment_position_payload(
                    strategy,
                    candidate,
                    instrument_id=getattr(position, "instrument_id", None),
                    signed_qty=getattr(position, "signed_qty", None),
                    avg_px_open=getattr(position, "avg_px_open", None),
                )

    payload: dict[str, Any] = {"type": type(position).__name__}
    for field_name in (
        "position_id",
        "instrument_id",
        "side",
        "signed_qty",
        "quantity",
        "avg_px_open",
        "avg_px_close",
        "realized_pnl",
    ):
        value = getattr(position, field_name, None)
        if value is not None:
            payload[field_name] = _stringify_identifier(value)
    if len(payload) == 1:
        payload["repr"] = repr(position)
    return _augment_position_payload(
        strategy,
        payload,
        instrument_id=getattr(position, "instrument_id", None),
        signed_qty=getattr(position, "signed_qty", None),
        avg_px_open=getattr(position, "avg_px_open", None),
    )


def _serialize_position_report_snapshot(
    strategy: Any,
    snapshot: Mapping[str, Any],
) -> dict[str, Any] | None:
    signed_qty = _to_decimal_or_none(snapshot.get("signed_qty"))
    if signed_qty is None:
        return None
    if signed_qty == 0:
        return None

    payload: dict[str, Any] = {
        "kind": "position",
        "instrument_id": _stringify_identifier(snapshot.get("instrument_id")),
        "signed_qty": decimal_to_json_str(signed_qty),
        "quantity": decimal_to_json_str(abs(signed_qty)),
        "side": "LONG" if signed_qty > 0 else "SHORT",
    }
    signed_qty_venue = _to_decimal_or_none(snapshot.get("signed_qty_venue"))
    if signed_qty_venue is None:
        signed_qty_venue = signed_qty
    quantity_venue = _to_decimal_or_none(snapshot.get("quantity_venue"))
    if quantity_venue is None and signed_qty_venue is not None:
        quantity_venue = abs(signed_qty_venue)
    signed_qty_base = _to_decimal_or_none(snapshot.get("signed_qty_base"))
    quantity_base = _to_decimal_or_none(snapshot.get("quantity_base"))
    if quantity_base is None and signed_qty_base is not None:
        quantity_base = abs(signed_qty_base)
    if signed_qty_venue is not None:
        payload["signed_qty_venue"] = decimal_to_json_str(signed_qty_venue)
    if quantity_venue is not None:
        payload["quantity_venue"] = decimal_to_json_str(quantity_venue)
    if signed_qty_base is not None:
        payload["signed_qty_base"] = decimal_to_json_str(signed_qty_base)
    if quantity_base is not None:
        payload["quantity_base"] = decimal_to_json_str(quantity_base)
    qty_conversion_status = _stringify_identifier(snapshot.get("qty_conversion_status")).strip()
    if qty_conversion_status:
        payload["qty_conversion_status"] = qty_conversion_status
    qty_conversion_source = _stringify_identifier(snapshot.get("qty_conversion_source")).strip()
    if qty_conversion_source:
        payload["qty_conversion_source"] = qty_conversion_source
    avg_px_open = decimal_to_json_str(snapshot.get("avg_px_open"))
    if avg_px_open is not None:
        payload["avg_px_open"] = avg_px_open
    position_id = _stringify_identifier(snapshot.get("position_id"))
    if position_id:
        payload["position_id"] = position_id
    return _augment_position_payload(
        strategy,
        payload,
        instrument_id=snapshot.get("instrument_id"),
        signed_qty=snapshot.get("signed_qty"),
        avg_px_open=snapshot.get("avg_px_open"),
    )


def decimal_to_json_str(value: Any) -> str | None:
    """
    Return a JSON-safe decimal string without scientific notation.
    """
    if value is None:
        return None

    as_decimal = _to_decimal_or_none(value)
    if as_decimal is None:
        return str(value)

    text = format(as_decimal, "f")
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    if text == "-0":
        return "0"
    return text or "0"


def decimal_to_wire_price(value: Any, *, precision: int | None) -> str:
    """
    Return a decimal string formatted for external consumers.
    """
    as_decimal = _to_decimal_or_none(value)
    if as_decimal is None:
        return str(value)
    if precision is None or precision < 0:
        return str(as_decimal)
    return f"{as_decimal:.{precision}f}"


def _is_blocked_state(state: str) -> bool:
    return str(state).startswith(BLOCKED_STATE_PREFIX)


def publish_market_bbo(
    strategy: Any,
    *,
    instrument_id: Any,
    bid: Decimal,
    ask: Decimal,
    ts_ns: int,
) -> None:
    """
    Publish a top-of-book snapshot for `instrument_id`.
    """
    instrument_text = str(instrument_id)
    instrument = strategy._instruments.get(instrument_id)
    if instrument is None:
        instrument = strategy.cache.instrument(instrument_id)
    price_precision_raw = getattr(instrument, "price_precision", None)
    try:
        price_precision = int(price_precision_raw) if price_precision_raw is not None else None
    except Exception:
        price_precision = None

    exchange = _stringify_identifier(getattr(instrument_id, "venue", None)).lower()
    symbol = _stringify_identifier(getattr(instrument, "raw_symbol", ""))
    if not symbol:
        symbol = instrument_text.split(".", maxsplit=1)[0]

    base = _stringify_identifier(getattr(instrument, "base_currency", None)).upper()
    quote = _stringify_identifier(getattr(instrument, "quote_currency", None)).upper()
    if not base or not quote:
        parsed_base, parsed_quote = inventory_mod.normalize_contract_symbol(symbol)
        base = base or parsed_base
        quote = quote or parsed_quote

    symbol_pair = f"{base}/{quote}" if base and quote else symbol
    fv_coin = (
        f"{base.lower()}/{quote.lower()}"
        if base and quote
        else symbol.replace("-", "/").replace("_", "/").lower()
    )

    payload = {
        "strategy_id": strategy._external_strategy_id,
        "instrument_id": instrument_text,
        "exchange": exchange,
        "base": base,
        "quote": quote,
        "symbol": symbol_pair,
        "fv_coin": fv_coin,
        "bid": decimal_to_wire_price(bid, precision=price_precision),
        "ask": decimal_to_wire_price(ask, precision=price_precision),
        "ts_event": ts_ns,
        "ts_ms": ts_ns // 1_000_000,
    }
    strategy._publish_json(TOPIC_MARKET_BBO, payload)


def publish_state_if_due(strategy: Any) -> None:
    """
    Publish a periodic running state update when not blocked.
    """
    if bool(getattr(strategy, "_state_is_blocked", False)):
        return
    now_ns = int(strategy.clock.timestamp_ns())
    if now_ns - strategy._last_state_ns < 250_000_000:
        return
    strategy._publish_state("running")


def publish_balances_if_due(strategy: Any) -> None:
    """
    Publish balances snapshot when the configured interval has elapsed.
    """
    now_ns = int(strategy.clock.timestamp_ns())
    if now_ns - strategy._last_balances_ns < strategy.BALANCES_PUBLISH_INTERVAL_MS * 1_000_000:
        return
    strategy._publish_balances()


def _inventory_skew_debug_payload(strategy: Any) -> dict[str, Any] | None:
    runtime_params_snapshot = getattr(strategy, "_quote_runtime_params_snapshot", None)
    compute_inventory_skew = getattr(strategy, "_compute_inventory_skew", None)
    if not callable(runtime_params_snapshot) or not callable(compute_inventory_skew):
        return None

    with suppress(Exception):
        runtime_params = runtime_params_snapshot()
        skew_ctx = compute_inventory_skew(runtime_params=runtime_params)
        if not isinstance(skew_ctx, Mapping):
            return None
        return {
            "inventory_qty_base": decimal_to_json_str(skew_ctx.get("inventory_qty_base")),
            "inventory_qty": decimal_to_json_str(skew_ctx.get("inventory_qty")),
            "inventory_source": skew_ctx.get("inventory_source"),
            "position_qty_base": decimal_to_json_str(skew_ctx.get("position_qty_base")),
            "position_qty_venue": decimal_to_json_str(skew_ctx.get("position_qty_venue")),
            "position_qty_complete": skew_ctx.get("position_qty_complete"),
            "position_qty": decimal_to_json_str(skew_ctx.get("position_qty")),
            "spot_base_total": decimal_to_json_str(skew_ctx.get("spot_qty")),
            "global_position_qty_base": decimal_to_json_str(skew_ctx.get("global_position_qty_base")),
            "global_position_qty_venue": decimal_to_json_str(skew_ctx.get("global_position_qty_venue")),
            "global_position_qty_complete": skew_ctx.get("global_position_qty_complete"),
            "global_position_qty": decimal_to_json_str(skew_ctx.get("global_position_qty")),
            "global_spot_qty": decimal_to_json_str(skew_ctx.get("global_spot_qty")),
            "global_inventory_qty_base": decimal_to_json_str(skew_ctx.get("global_inventory_qty_base")),
            "global_inventory_qty_complete": skew_ctx.get("global_inventory_qty_complete"),
            "global_inventory_qty": decimal_to_json_str(skew_ctx.get("global_inventory_qty")),
            "global_inventory_source": skew_ctx.get("global_inventory_source"),
            "local_position_qty_base": decimal_to_json_str(skew_ctx.get("local_position_qty_base")),
            "local_position_qty_venue": decimal_to_json_str(skew_ctx.get("local_position_qty_venue")),
            "local_position_qty_complete": skew_ctx.get("local_position_qty_complete"),
            "local_position_qty": decimal_to_json_str(skew_ctx.get("local_position_qty")),
            "local_spot_qty": decimal_to_json_str(skew_ctx.get("local_spot_qty")),
            "local_inventory_qty_base": decimal_to_json_str(skew_ctx.get("local_inventory_qty_base")),
            "local_inventory_qty_complete": skew_ctx.get("local_inventory_qty_complete"),
            "local_inventory_qty": decimal_to_json_str(skew_ctx.get("local_inventory_qty")),
            "local_inventory_source": skew_ctx.get("local_inventory_source"),
            "global_position_qty_conversion_status": skew_ctx.get("global_position_qty_conversion_status"),
            "global_position_qty_conversion_source": skew_ctx.get("global_position_qty_conversion_source"),
            "local_position_qty_conversion_status": skew_ctx.get("local_position_qty_conversion_status"),
            "local_position_qty_conversion_source": skew_ctx.get("local_position_qty_conversion_source"),
            "base_currency": skew_ctx.get("base_currency"),
            "des_qty_global": decimal_to_json_str(skew_ctx.get("des_qty_global")),
            "max_qty_global": decimal_to_json_str(skew_ctx.get("max_qty_global")),
            "max_skew_bps_global": decimal_to_json_str(skew_ctx.get("max_skew_bps_global")),
            "des_qty_local": decimal_to_json_str(skew_ctx.get("des_qty_local")),
            "max_qty_local": decimal_to_json_str(skew_ctx.get("max_qty_local")),
            "max_skew_bps_local": decimal_to_json_str(skew_ctx.get("max_skew_bps_local")),
            "linear_offset_bps": decimal_to_json_str(skew_ctx.get("linear_offset_bps")),
            "global_ratio": decimal_to_json_str(skew_ctx.get("global_ratio")),
            "global_skew_bps": decimal_to_json_str(skew_ctx.get("global_skew_bps")),
            "local_ratio": decimal_to_json_str(skew_ctx.get("local_ratio")),
            "local_skew_bps": decimal_to_json_str(skew_ctx.get("local_skew_bps")),
            "total_skew_bps": decimal_to_json_str(skew_ctx.get("total_skew_bps")),
        }
    return None


def _pricing_debug_payload(strategy: Any) -> dict[str, Any] | None:
    existing = getattr(strategy, "_last_pricing_debug", None)
    payload: dict[str, Any] = {}
    if isinstance(existing, Mapping):
        for key, value in existing.items():
            payload[key] = dict(value) if isinstance(value, Mapping) else value

    # Inventory skew reflects shared portfolio state and must stay live even when
    # quote-cycle pricing details are being reused from cache.
    skew_payload = _inventory_skew_debug_payload(strategy)
    if skew_payload is not None:
        payload["skew"] = skew_payload

    return payload or None


def _quote_target_depth(strategy: Any) -> int:
    runtime_params_snapshot = getattr(strategy, "_quote_runtime_params_snapshot", None)
    if not callable(runtime_params_snapshot):
        return 0

    with suppress(Exception):
        runtime_params = runtime_params_snapshot()
        total = Decimal(0)
        for key in ("n_orders1", "n_orders2", "n_orders3"):
            value = _to_decimal_or_none(runtime_params.get(key))
            if value is None:
                continue
            total += max(Decimal(0), value)
        return max(0, int(total))
    return 0


def _maker_quote_status_payload(
    strategy: Any,
    *,
    managed_orders: Sequence[Any],
    state: str | None = None,
) -> dict[str, int] | None:
    bid_open = 0
    ask_open = 0
    for order in managed_orders:
        side = getattr(order, "side", None)
        if side == OrderSide.BUY or _stringify_identifier(side).strip().upper() == "BUY":
            bid_open += 1
        elif side == OrderSide.SELL or _stringify_identifier(side).strip().upper() == "SELL":
            ask_open += 1

    bid_depth = _quote_target_depth(strategy)
    ask_depth = bid_depth
    if bid_depth <= 0 and ask_depth <= 0 and bid_open <= 0 and ask_open <= 0:
        return None

    return {
        "bid_open": int(max(0, bid_open)),
        "ask_open": int(max(0, ask_open)),
        "bid_depth": int(max(0, bid_depth)),
        "ask_depth": int(max(0, ask_depth)),
        "bid_blocked": int(max(0, bid_depth - bid_open)),
        "ask_blocked": int(max(0, ask_depth - ask_open)),
    }


def publish_state(
    strategy: Any,
    state: str,
    *,
    managed_orders_count: int | None = None,
    managed_orders: Sequence[Any] | None = None,
    refresh_pricing_debug: bool = True,
) -> None:
    """
    Publish a state snapshot and emit blocked/unblocked transition events.
    """
    now_ns = int(strategy.clock.timestamp_ns())
    was_blocked = bool(getattr(strategy, "_state_is_blocked", False))
    is_blocked = _is_blocked_state(state)
    effective_state = runtime_params_mod.effective_state_name(strategy, state)
    previous_state = getattr(strategy, "_last_state_name", None)
    if was_blocked != is_blocked:
        strategy._publish_event(
            "state_transition",
            from_state=previous_state,
            to_state=effective_state,
            from_blocked=was_blocked,
            to_blocked=is_blocked,
        )
        if not is_blocked:
            strategy._last_stale_cancel_ns = 0
    strategy._state_is_blocked = is_blocked
    strategy._last_state_name = effective_state
    strategy._last_state_ns = now_ns
    managed_orders_list = list(managed_orders) if managed_orders is not None else None
    if managed_orders_list is None:
        managed_orders_list = list(strategy._managed_orders())
    if managed_orders_count is None:
        managed_orders_count = len(managed_orders_list)
    tracked_managed_orders = strategy._tracked_managed_order_count()
    effective_bot_on = strategy._effective_bot_on()
    persisted_bot_on = runtime_params_mod.persisted_bot_on(strategy)
    config_bot_on = runtime_params_mod.config_bot_on(strategy)
    bot_on_reason = runtime_params_mod.bot_on_reason(strategy)
    payload: dict[str, Any] = {
        "strategy_id": strategy._external_strategy_id,
        "state": effective_state,
        "bot_on": effective_bot_on,
        "effective_bot_on": effective_bot_on,
        "persisted_bot_on": persisted_bot_on,
        "config_bot_on": config_bot_on,
        "startup_bot_off_active": bool(getattr(strategy, "_startup_bot_off_active", False)),
        "terminal_order_denial_active": bool(
            getattr(strategy, "_terminal_order_denial_circuit_open", False),
        ),
        "bot_on_reason": bot_on_reason,
        "managed_orders": max(0, int(managed_orders_count)),
        "tracked_managed_orders": tracked_managed_orders,
        "ts_event": now_ns,
        "ts_ms": now_ns // 1_000_000,
    }
    maker_quote_status = _maker_quote_status_payload(
        strategy,
        managed_orders=managed_orders_list,
        state=effective_state,
    )
    if maker_quote_status is not None:
        payload["maker_quote_status"] = maker_quote_status
    quote_progress_fn = getattr(strategy, "_quote_progress_payload", None)
    if callable(quote_progress_fn):
        quote_progress = quote_progress_fn()
        if quote_progress:
            payload["quote_progress"] = quote_progress
    quote_blockers_fn = getattr(strategy, "_quote_blockers_payload", None)
    if callable(quote_blockers_fn):
        quote_blockers = quote_blockers_fn(state=effective_state)
        if quote_blockers:
            payload["quote_blockers"] = quote_blockers
    maker_role_map = _maker_role_map_payload(strategy)
    if maker_role_map:
        payload["maker_role_map"] = maker_role_map
    pricing_debug = (
        _pricing_debug_payload(strategy)
        if refresh_pricing_debug
        else getattr(strategy, "_last_pricing_debug", None)
    )
    if pricing_debug:
        strategy._last_pricing_debug = pricing_debug
        payload["pricing_debug"] = pricing_debug
    last_quote_snapshot = getattr(strategy, "_last_quote_snapshot", None)
    if isinstance(last_quote_snapshot, Mapping):
        quote_snapshot = dict(last_quote_snapshot)
        quote_snapshot["mode"] = decode_text(quote_snapshot.get("mode")).strip() or (
            "ON" if effective_bot_on else "OFF"
        )
        quote_snapshot["reason"] = decode_text(quote_snapshot.get("reason")).strip() or effective_state
        payload["maker_v3"] = {"quote_snapshot": quote_snapshot}
    strategy._publish_json(
        TOPIC_STATE,
        payload,
    )


def publish_event(strategy: Any, name: str, *, ts_ns: int | None = None, **payload: Any) -> None:
    """
    Publish a structured strategy event to the event topic.
    """
    now_ns = int(strategy.clock.timestamp_ns()) if ts_ns is None else int(ts_ns)
    data: dict[str, Any] = {
        "strategy_id": strategy._external_strategy_id,
        "event": name,
        "ts_event": now_ns,
        "ts_ms": now_ns // 1_000_000,
    }
    data.update(payload)
    strategy._publish_json(TOPIC_EVENT, data)


def publish_actionable_alert(
    strategy: Any,
    *,
    alert_key: str,
    message: str,
    level: str = "warning",
    reason_code: str | None = None,
    cooldown_ms: int = 0,
    transition: str | None = None,
    now_ns: int | None = None,
    **extra_fields: Any,
) -> bool:
    """
    Publish a cooldown/transition-gated alert and return True when emitted.
    """
    publish_ns = int(strategy.clock.timestamp_ns()) if now_ns is None else int(now_ns)
    if not message:
        return False
    last_sent_ns = int(strategy._last_actionable_alert_ns.get(alert_key, 0))
    last_transition = strategy._last_actionable_alert_transition.get(alert_key)
    transition_changed = transition is not None and transition != last_transition
    cooldown_ns = max(0, int(cooldown_ms)) * 1_000_000
    if (
        cooldown_ns > 0
        and last_sent_ns > 0
        and publish_ns - last_sent_ns < cooldown_ns
        and not transition_changed
    ):
        return False
    if transition is not None:
        strategy._last_actionable_alert_transition[alert_key] = transition
    strategy._last_actionable_alert_ns[alert_key] = publish_ns
    strategy._publish_alert(
        message=message,
        level=level,
        ts_ns=publish_ns,
        alert_key=alert_key,
        reason_code=reason_code,
        actionable=True,
        **extra_fields,
    )
    return True


def publish_alert(
    strategy: Any,
    message: str,
    level: str = "warning",
    *,
    ts_ns: int | None = None,
    alert_key: str | None = None,
    reason_code: str | None = None,
    actionable: bool | None = None,
    **extra_fields: Any,
) -> None:
    """
    Publish an alert payload to the alert topic.
    """
    now_ns = int(strategy.clock.timestamp_ns()) if ts_ns is None else int(ts_ns)
    payload: dict[str, Any] = {
        "strategy_id": strategy._external_strategy_id,
        "level": level,
        "message": message,
        "ts_event": now_ns,
        "ts_ms": now_ns // 1_000_000,
    }
    if alert_key is not None:
        payload["alert_key"] = alert_key
    if reason_code is not None:
        payload["reason_code"] = reason_code
    if actionable is not None:
        payload["actionable"] = actionable
    payload.update(extra_fields)
    strategy._publish_json(
        TOPIC_ALERT,
        payload,
    )


def publish_balances(strategy: Any) -> None:  # noqa: C901
    """
    Publish account and position balances snapshot.
    """
    now_ns = int(strategy.clock.timestamp_ns())
    strategy._last_balances_ns = now_ns
    payload: dict[str, Any] = {
        "strategy_id": strategy._external_strategy_id,
        "accounts": [],
        "positions": [],
    }
    cache = _strategy_cache(strategy)
    try:
        accounts_lookup = getattr(cache, "accounts", None)
        accounts = list(accounts_lookup()) if callable(accounts_lookup) else []
    except Exception:
        accounts = []

    for account in accounts:
        payload["accounts"].append(_serialize_account_payload(account))

    supplemental_balance_snapshot = None
    supplemental_balance_snapshot_lookup = getattr(strategy, "_supplemental_balance_snapshot", None)
    if callable(supplemental_balance_snapshot_lookup):
        with suppress(Exception):
            supplemental_balance_snapshot = supplemental_balance_snapshot_lookup()
    if isinstance(supplemental_balance_snapshot, Mapping):
        supplemental_accounts = supplemental_balance_snapshot.get("accounts")
        if isinstance(supplemental_accounts, Sequence):
            for account in supplemental_accounts:
                if isinstance(account, dict):
                    payload["accounts"].append(dict(account))

    fresh_maker_position_snapshot = None
    fresh_snapshot_lookup = getattr(strategy, "_fresh_maker_position_report_snapshot", None)
    if callable(fresh_snapshot_lookup):
        with suppress(Exception):
            fresh_maker_position_snapshot = fresh_snapshot_lookup()
    has_fresh_maker_position_snapshot = isinstance(fresh_maker_position_snapshot, Mapping)
    serialized_fresh_position = (
        _serialize_position_report_snapshot(strategy, fresh_maker_position_snapshot)
        if has_fresh_maker_position_snapshot
        else None
    )
    if serialized_fresh_position is not None:
        payload["positions"].append(serialized_fresh_position)

    positions: list[Any] = []
    if not has_fresh_maker_position_snapshot:
        positions_open = getattr(cache, "positions_open", None)
        if callable(positions_open):
            with suppress(Exception):
                positions.extend(
                    positions_open(
                        instrument_id=strategy.config.maker_instrument_id,
                    ),
                )
        positions = _effective_inventory_positions(strategy, positions)
        if not positions and callable(positions_open):
            maker_instrument = getattr(strategy, "_maker_instrument", None)
            instruments = getattr(strategy, "_instruments", {})
            if maker_instrument is None and isinstance(instruments, dict):
                maker_instrument = instruments.get(strategy.config.maker_instrument_id)
            maker_base_currency = inventory_mod.maker_base_currency_code(
                instrument=maker_instrument,
                instrument_id=strategy.config.maker_instrument_id,
            )
            with suppress(Exception):
                all_positions = _effective_inventory_positions(strategy, list(positions_open()))
                positions.extend(
                    _matching_base_positions(
                        strategy,
                        all_positions,
                        base_currency=maker_base_currency,
                    ),
                )

    for position in positions:
        payload["positions"].append(_serialize_position_payload(strategy, position))

    if isinstance(supplemental_balance_snapshot, Mapping):
        supplemental_positions = supplemental_balance_snapshot.get("positions")
        if isinstance(supplemental_positions, Sequence):
            for position in supplemental_positions:
                if isinstance(position, dict):
                    payload["positions"].append(dict(position))

    payload["ts_event"] = now_ns
    payload["ts_ms"] = now_ns // 1_000_000
    strategy._publish_json(TOPIC_BALANCES, payload)


def publish_json(strategy: Any, topic: str, payload: dict[str, Any] | list[Any]) -> None:
    """
    Serialize payload and publish it on `topic` via the message bus.
    """
    payload_json = to_json_safe(payload)
    if FluxBusPayload is None:
        strategy.msgbus.publish(topic=topic, msg=payload_json)
        return
    strategy.msgbus.publish(
        topic=topic,
        msg=FluxBusPayload(topic=topic, payload=payload_json),
    )
