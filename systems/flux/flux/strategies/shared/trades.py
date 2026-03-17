from __future__ import annotations

import sys
from contextlib import suppress
from decimal import Decimal
from typing import Any
from typing import Callable
from typing import Mapping

if __name__ == "flux.strategies.shared.trades":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.trades",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.trades":
    sys.modules.setdefault("flux.strategies.shared.trades", sys.modules[__name__])


# Legacy shared contract: the bridge and API already key off this topic.
TOPIC_TRADE = "flux.makerv3.trade"


def _text(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()


def _enum_text(value: Any) -> str:
    name = getattr(value, "name", None)
    if isinstance(name, str) and name.strip():
        return name.strip().upper()
    return _text(value).upper()


def _decimal_or_none(value: Any) -> Decimal | None:
    if value is None:
        return None
    if isinstance(value, Decimal):
        return value
    as_decimal = getattr(value, "as_decimal", None)
    if callable(as_decimal):
        with suppress(Exception):
            out = as_decimal()
            if out is not None:
                return Decimal(str(out))
    as_double = getattr(value, "as_double", None)
    if callable(as_double):
        with suppress(Exception):
            return Decimal(str(as_double()))
    with suppress(Exception):
        return Decimal(str(value))
    return None


def _timestamp_ms(value: Any) -> int:
    with suppress(Exception):
        ts_value = int(value or 0)
        while ts_value > 10_000_000_000_000:
            ts_value //= 1_000
        return max(0, ts_value)
    return 0


def _lookup_instrument(
    instrument_lookup: Callable[[Any], Any] | None,
    instrument_id: Any,
) -> Any | None:
    if not callable(instrument_lookup):
        return None
    with suppress(Exception):
        return instrument_lookup(instrument_id)
    return None


def _currency_code(value: Any) -> str:
    if value is None:
        return ""
    code = getattr(value, "code", None)
    return _text(code or value).upper()


def _symbol_from_instrument(instrument: Any | None, instrument_id_text: str) -> str:
    raw_symbol = _text(getattr(instrument, "raw_symbol", None))
    if raw_symbol:
        return raw_symbol
    if not instrument_id_text:
        return ""
    symbol = instrument_id_text.split(".")[0].strip()
    if ":" in symbol:
        symbol = symbol.split(":", maxsplit=1)[1].strip()
    return symbol


def _coin_from_symbol(symbol: str) -> str:
    normalized = symbol.strip().upper()
    if not normalized:
        return ""
    base = normalized.split("/")[0].strip() or normalized
    base = base.split("-")[0].strip() or base
    return base


def _quote_currency_codes(instrument: Any | None) -> set[str]:
    if instrument is None:
        return set()
    codes: set[str] = set()
    for attr in ("quote_currency", "settlement_currency"):
        code = _currency_code(getattr(instrument, attr, None))
        if code:
            codes.add(code)
    get_cost_currency = getattr(instrument, "get_cost_currency", None)
    if callable(get_cost_currency):
        with suppress(Exception):
            cost_currency = get_cost_currency()
            code = _currency_code(cost_currency)
            if code:
                codes.add(code)
    return codes


def _commission_payload(
    *,
    commission: Any,
    instrument: Any | None,
) -> dict[str, str]:
    amount = _decimal_or_none(commission)
    if amount is None:
        return {}
    amount_text = str(amount)
    currency_code = _currency_code(getattr(commission, "currency", None))
    payload = {
        "fee": amount_text,
        "fee_amount_raw": amount_text,
    }
    if currency_code:
        payload["fee_asset_raw"] = currency_code
        if currency_code in _quote_currency_codes(instrument):
            payload["fee_quote"] = amount_text
    return payload


def build_trade_payload(
    *,
    strategy_id: str,
    event: Any,
    instrument_lookup: Callable[[Any], Any] | None = None,
    trade_role: str | None = None,
    extra_fields: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    instrument_id = getattr(event, "instrument_id", None)
    instrument_id_text = _text(instrument_id)
    instrument = _lookup_instrument(instrument_lookup, instrument_id)
    qty = _decimal_or_none(getattr(event, "last_qty", None))
    price = _decimal_or_none(getattr(event, "last_px", None))
    ts_event = int(getattr(event, "ts_event", 0) or 0)
    trade_id = _text(getattr(event, "trade_id", None))
    client_order_id = _text(getattr(event, "client_order_id", None))
    symbol = _symbol_from_instrument(instrument, instrument_id_text)
    base_asset = _currency_code(getattr(instrument, "base_currency", None))
    coin = base_asset or _coin_from_symbol(symbol)
    quote_asset = _currency_code(getattr(instrument, "quote_currency", None))
    exchange = instrument_id_text.split(".")[-1].strip().lower() if "." in instrument_id_text else ""
    row_key = trade_id or client_order_id or str(_timestamp_ms(ts_event))

    payload: dict[str, Any] = {
        "strategy_id": _text(strategy_id),
        "event": "order_filled",
        "instrument_id": instrument_id_text,
        "client_order_id": client_order_id,
        "trade_id": trade_id,
        "side": _enum_text(getattr(event, "order_side", None) or getattr(event, "side", None)),
        "qty": str(qty) if qty is not None else "",
        "price": str(price) if price is not None else "",
        "ts_event": ts_event,
        "ts_ms": _timestamp_ms(ts_event),
        "row_id": f"{_text(strategy_id)}:{trade_role or 'trade'}:{instrument_id_text}:{row_key}",
    }
    if trade_role:
        payload["trade_role"] = trade_role
    if symbol:
        payload["symbol"] = symbol
    if coin:
        payload["coin"] = coin
        payload["base_asset"] = coin
        payload["inventory_asset"] = coin
    if quote_asset:
        payload["quote_asset"] = quote_asset
    if exchange:
        payload["exchange"] = exchange
        payload["venue"] = exchange

    commission = getattr(event, "commission", None)
    if commission is not None:
        payload.update(_commission_payload(commission=commission, instrument=instrument))

    if extra_fields:
        for key, value in extra_fields.items():
            if value is not None:
                payload[key] = value

    return payload


def publish_trade(
    publish_json: Callable[[str, Mapping[str, Any]], Any],
    *,
    strategy_id: str,
    event: Any,
    instrument_lookup: Callable[[Any], Any] | None = None,
    trade_role: str | None = None,
    extra_fields: Mapping[str, Any] | None = None,
    topic: str = TOPIC_TRADE,
) -> dict[str, Any]:
    payload = build_trade_payload(
        strategy_id=strategy_id,
        event=event,
        instrument_lookup=instrument_lookup,
        trade_role=trade_role,
        extra_fields=extra_fields,
    )
    publish_json(topic, payload)
    return payload


__all__ = [
    "TOPIC_TRADE",
    "build_trade_payload",
    "publish_trade",
]
