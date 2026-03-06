"""
Publish and serialize MakerV3 strategy observability payloads.
"""

from __future__ import annotations

import json
from contextlib import suppress
from decimal import Decimal
from typing import Any

from nautilus_trader.flux.strategies.makerv3 import inventory as inventory_mod
from nautilus_trader.flux.strategies.makerv3 import pricing as pricing_mod
from nautilus_trader.flux.strategies.makerv3.constants import BLOCKED_STATE_PREFIX
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_ALERT
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_BALANCES
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_EVENT
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_MARKET_BBO
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_STATE
from nautilus_trader.flux.strategies.makerv3.wire import FluxBusPayload


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


def _matching_base_positions(
    strategy: Any,
    positions: list[Any],
    *,
    base_currency: str | None,
) -> list[Any]:
    if not base_currency:
        return positions

    filtered: list[Any] = []
    for position in positions:
        if inventory_mod.position_matches_base_currency(
            position,
            base_currency=base_currency,
            instrument_lookup=lambda instrument_id: _resolve_instrument(strategy, instrument_id),
        ):
            filtered.append(position)
    return filtered


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


def _serialize_position_payload(position: Any) -> dict[str, Any]:
    pos_to_dict = getattr(position, "to_dict", None)
    if callable(pos_to_dict):
        with suppress(Exception):
            candidate = _json_safe_or_none(pos_to_dict())
            if candidate is not None:
                return candidate

        with suppress(Exception):
            candidate = _json_safe_or_none(pos_to_dict(position))
            if candidate is not None:
                return candidate

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
    return payload


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


def publish_state(strategy: Any, state: str, *, managed_orders_count: int | None = None) -> None:
    """
    Publish a state snapshot and emit blocked/unblocked transition events.
    """
    now_ns = int(strategy.clock.timestamp_ns())
    was_blocked = bool(getattr(strategy, "_state_is_blocked", False))
    is_blocked = _is_blocked_state(state)
    previous_state = getattr(strategy, "_last_state_name", None)
    if was_blocked != is_blocked:
        strategy._publish_event(
            "state_transition",
            from_state=previous_state,
            to_state=state,
            from_blocked=was_blocked,
            to_blocked=is_blocked,
        )
        if not is_blocked:
            strategy._last_stale_cancel_ns = 0
    strategy._state_is_blocked = is_blocked
    strategy._last_state_name = state
    strategy._last_state_ns = now_ns
    if managed_orders_count is None:
        managed_orders_count = len(strategy._managed_orders())
    payload: dict[str, Any] = {
        "strategy_id": strategy._external_strategy_id,
        "state": state,
        "bot_on": strategy._effective_bot_on(),
        "managed_orders": max(managed_orders_count, strategy._tracked_managed_order_count()),
        "ts_event": now_ns,
        "ts_ms": now_ns // 1_000_000,
    }
    if strategy._last_pricing_debug:
        payload["pricing_debug"] = strategy._last_pricing_debug
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

    if not accounts and hasattr(strategy, "portfolio"):
        try:
            maker_venue = getattr(strategy.config.maker_instrument_id, "venue", None)
            account = (
                strategy.portfolio.account(venue=maker_venue) if maker_venue is not None else None
            )
        except Exception:
            account = None
        if account is not None:
            accounts.append(account)

    for account in accounts:
        payload["accounts"].append(_serialize_account_payload(account))

    positions: list[Any] = []
    positions_open = getattr(cache, "positions_open", None)
    if callable(positions_open):
        with suppress(Exception):
            positions.extend(
                positions_open(
                    instrument_id=strategy.config.maker_instrument_id,
                ),
            )
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
            positions.extend(
                _matching_base_positions(
                    strategy,
                    list(positions_open()),
                    base_currency=maker_base_currency,
                ),
            )

    for position in positions:
        payload["positions"].append(_serialize_position_payload(position))

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
