from __future__ import annotations

from typing import Any

from flux.bridge.handlers.types import CorrelationContext
from flux.bridge.handlers.types import StreamJSONOp
from flux.bridge.handlers.types import WriteOp
from flux.bridge.handlers.utils import as_dict
from flux.bridge.handlers.utils import as_rows
from flux.bridge.handlers.utils import first_text
from flux.bridge.handlers.utils import normalize_ts_ms
from flux.bridge.handlers.utils import with_correlation
from flux.common.keys import FluxRedisKeys


TRADES_MAXLEN = 20_000


def transform_trade(payload: Any, context: CorrelationContext) -> list[WriteOp]:
    keys = FluxRedisKeys(strategy_id=context.strategy_id)
    rows = as_rows(payload)
    if not rows:
        row = as_dict(payload)
        rows = [row] if row else []

    ops: list[WriteOp] = []
    for index, row in enumerate(rows):
        ts_ms = normalize_ts_ms(row, context.ts_ms)
        out = with_correlation(row, context, ts_ms=ts_ms)
        row_id = first_text(out.get("row_id"), out.get("trade_id"), out.get("exchange_trade_id"))
        out.setdefault("row_id", row_id or f"{context.entry_id}:{index}")
        ops.append(
            StreamJSONOp(
                key=keys.trades_stream(),
                row=out,
                maxlen=TRADES_MAXLEN,
            ),
        )
    return ops
