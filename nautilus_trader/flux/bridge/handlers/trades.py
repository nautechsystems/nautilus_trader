# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

from typing import Any

from nautilus_trader.flux.bridge.handlers.types import CorrelationContext
from nautilus_trader.flux.bridge.handlers.types import StreamJSONOp
from nautilus_trader.flux.bridge.handlers.types import WriteOp
from nautilus_trader.flux.bridge.handlers.utils import as_dict
from nautilus_trader.flux.bridge.handlers.utils import as_rows
from nautilus_trader.flux.bridge.handlers.utils import first_text
from nautilus_trader.flux.bridge.handlers.utils import normalize_ts_ms
from nautilus_trader.flux.bridge.handlers.utils import with_correlation
from nautilus_trader.flux.common.keys import FluxRedisKeys


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
