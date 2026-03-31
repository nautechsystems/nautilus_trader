from __future__ import annotations

from typing import Any

from flux.bridge.handlers.types import CorrelationContext
from flux.bridge.handlers.types import SetJSONOp
from flux.bridge.handlers.types import WriteOp
from flux.bridge.handlers.utils import as_dict
from flux.bridge.handlers.utils import normalize_ts_ms
from flux.bridge.handlers.utils import strategy_id_for_row
from flux.bridge.handlers.utils import with_correlation
from flux.common.keys import FluxRedisKeys


def transform_state(payload: Any, context: CorrelationContext) -> list[WriteOp]:
    row = as_dict(payload)
    strategy_id = strategy_id_for_row(row, context)
    keys = FluxRedisKeys(strategy_id=strategy_id)
    ts_ms = normalize_ts_ms(row, context.ts_ms)
    return [
        SetJSONOp(
            key=keys.state(),
            value=with_correlation(row, context, ts_ms=ts_ms, strategy_id=strategy_id),
        )
    ]
