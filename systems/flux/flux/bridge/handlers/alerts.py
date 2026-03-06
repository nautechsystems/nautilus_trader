from __future__ import annotations

from typing import Any

from flux.bridge.handlers.types import CorrelationContext
from flux.bridge.handlers.types import StreamJSONOp
from flux.bridge.handlers.types import WriteOp
from flux.bridge.handlers.utils import as_dict
from flux.bridge.handlers.utils import as_rows
from flux.bridge.handlers.utils import normalize_ts_ms
from flux.bridge.handlers.utils import with_correlation
from flux.common.keys import FluxRedisKeys


ALERTS_MAXLEN = 2_000


def transform_alert(payload: Any, context: CorrelationContext) -> list[WriteOp]:
    keys = FluxRedisKeys(strategy_id=context.strategy_id)
    rows = as_rows(payload)
    if not rows:
        row = as_dict(payload)
        rows = [row] if row else []

    ops: list[WriteOp] = []
    for row in rows:
        ts_ms = normalize_ts_ms(row, context.ts_ms)
        ops.append(
            StreamJSONOp(
                key=keys.alerts(),
                row=with_correlation(row, context, ts_ms=ts_ms),
                maxlen=ALERTS_MAXLEN,
            ),
        )
    return ops
