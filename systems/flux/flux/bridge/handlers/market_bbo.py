from __future__ import annotations

from typing import Any

from flux.bridge.handlers.types import CorrelationContext
from flux.bridge.handlers.types import SetJSONOp
from flux.bridge.handlers.types import WriteOp
from flux.bridge.handlers.utils import as_dict
from flux.bridge.handlers.utils import first_text
from flux.bridge.handlers.utils import normalize_exchange
from flux.bridge.handlers.utils import normalize_symbol_parts
from flux.bridge.handlers.utils import normalize_ts_ms
from flux.bridge.handlers.utils import strategy_id_for_row
from flux.bridge.handlers.utils import with_correlation
from flux.common.keys import FluxRedisKeys


MARKET_LAST_TTL_SECONDS = 120


def _safe_float(value: Any) -> float | None:
    try:
        if value is None:
            return None
        return float(value)
    except (TypeError, ValueError):
        return None


def transform_market_bbo(payload: Any, context: CorrelationContext) -> list[WriteOp]:
    row = as_dict(payload)
    strategy_id = strategy_id_for_row(row, context)
    instrument_id = first_text(row.get("instrument_id"))
    exchange = normalize_exchange(
        first_text(row.get("exchange"), row.get("venue"), row.get("market_exchange")),
    )
    base, quote = normalize_symbol_parts(
        base=row.get("base"),
        quote=row.get("quote"),
        symbol=first_text(row.get("symbol"), row.get("market_key"), row.get("pair")),
    )

    if not exchange or not base or not quote:
        return []

    bid = _safe_float(row.get("bid") or row.get("best_bid") or row.get("bid_px"))
    ask = _safe_float(row.get("ask") or row.get("best_ask") or row.get("ask_px"))
    if bid is None and ask is None:
        return []

    ts_ms = normalize_ts_ms(row, context.ts_ms)
    out = with_correlation(row, context, ts_ms=ts_ms, strategy_id=strategy_id)
    out["exchange"] = exchange
    out["base"] = base
    out["quote"] = quote
    if bid is not None:
        out["bid"] = bid
    if ask is not None:
        out["ask"] = ask

    keys = FluxRedisKeys(strategy_id=strategy_id)
    ops: list[WriteOp] = []
    if instrument_id:
        ops.append(
            SetJSONOp(
                key=keys.market_last(
                    exchange=exchange,
                    base=base,
                    quote=quote,
                    instrument_id=instrument_id,
                ),
                value=out,
                ttl_seconds=MARKET_LAST_TTL_SECONDS,
            ),
        )
    ops.append(
        SetJSONOp(
            key=keys.market_last(exchange=exchange, base=base, quote=quote),
            value=out,
            ttl_seconds=MARKET_LAST_TTL_SECONDS,
        ),
    )
    return ops
