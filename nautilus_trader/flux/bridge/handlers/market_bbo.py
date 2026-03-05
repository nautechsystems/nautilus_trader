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
from nautilus_trader.flux.bridge.handlers.types import SetJSONOp
from nautilus_trader.flux.bridge.handlers.types import WriteOp
from nautilus_trader.flux.bridge.handlers.utils import as_dict
from nautilus_trader.flux.bridge.handlers.utils import first_text
from nautilus_trader.flux.bridge.handlers.utils import normalize_exchange
from nautilus_trader.flux.bridge.handlers.utils import normalize_symbol_parts
from nautilus_trader.flux.bridge.handlers.utils import normalize_ts_ms
from nautilus_trader.flux.bridge.handlers.utils import with_correlation
from nautilus_trader.flux.common.keys import FluxRedisKeys


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
    out = with_correlation(row, context, ts_ms=ts_ms)
    out["exchange"] = exchange
    out["base"] = base
    out["quote"] = quote
    if bid is not None:
        out["bid"] = bid
    if ask is not None:
        out["ask"] = ask

    keys = FluxRedisKeys(strategy_id=context.strategy_id)
    return [
        SetJSONOp(
            key=keys.market_last(exchange=exchange, base=base, quote=quote),
            value=out,
            ttl_seconds=MARKET_LAST_TTL_SECONDS,
        ),
    ]
