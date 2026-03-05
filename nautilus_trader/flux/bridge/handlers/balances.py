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
from nautilus_trader.flux.bridge.handlers.types import JSONRow
from nautilus_trader.flux.bridge.handlers.types import JSONValue
from nautilus_trader.flux.bridge.handlers.types import ReplaceHashJSONOp
from nautilus_trader.flux.bridge.handlers.types import SetJSONOp
from nautilus_trader.flux.bridge.handlers.types import WriteOp
from nautilus_trader.flux.bridge.handlers.utils import as_rows
from nautilus_trader.flux.bridge.handlers.utils import first_text
from nautilus_trader.flux.bridge.handlers.utils import normalize_exchange
from nautilus_trader.flux.bridge.handlers.utils import normalize_ts_ms
from nautilus_trader.flux.bridge.handlers.utils import with_correlation
from nautilus_trader.flux.common.keys import FluxRedisKeys


def _rows_from_payload(payload: Any) -> list[dict[str, Any]]:
    rows = as_rows(payload)
    expanded: list[dict[str, Any]] = []
    for row in rows:
        accounts = row.get("accounts")
        positions = row.get("positions")
        if isinstance(accounts, list):
            for account in accounts:
                if isinstance(account, dict):
                    expanded.append(dict(account))
        if isinstance(positions, list):
            for position in positions:
                if isinstance(position, dict):
                    out = dict(position)
                    out.setdefault("kind", "position")
                    expanded.append(out)
        if not isinstance(accounts, list) and not isinstance(positions, list):
            expanded.append(dict(row))
    return expanded


def _exchange_from_row(row: dict[str, Any]) -> str:
    account_id = first_text(row.get("account_id"))
    prefix = ""
    if "-" in account_id:
        prefix = account_id.split("-", maxsplit=1)[0]
    elif ":" in account_id:
        prefix = account_id.split(":", maxsplit=1)[0]
    return normalize_exchange(
        first_text(row.get("exchange"), row.get("venue"), row.get("source"), prefix, "unknown"),
    )


def _asset_from_row(row: dict[str, Any]) -> str:
    return first_text(
        row.get("asset"),
        row.get("coin"),
        row.get("base"),
        row.get("currency"),
        "UNKNOWN",
    ).upper()


def _account_from_row(row: dict[str, Any]) -> str:
    return first_text(
        row.get("account"),
        row.get("account_id"),
        row.get("balance_location"),
        row.get("scope"),
        "default",
    ).lower()


def transform_balances(payload: Any, context: CorrelationContext) -> list[WriteOp]:
    rows = _rows_from_payload(payload)
    if not rows:
        return []

    normalized_rows: list[JSONValue] = []
    mapping: dict[str, JSONRow] = {}

    for row in rows:
        exchange = _exchange_from_row(row)
        asset = _asset_from_row(row)
        account = _account_from_row(row)
        ts_ms = normalize_ts_ms(row, context.ts_ms)
        out = with_correlation(row, context, ts_ms=ts_ms)
        out["exchange"] = exchange
        out["asset"] = asset
        out["account"] = account
        normalized_rows.append(out)
        mapping[f"{exchange}:{asset}:{account}"] = out

    keys = FluxRedisKeys(strategy_id=context.strategy_id)
    return [
        SetJSONOp(key=keys.balances_snapshot(), value=normalized_rows),
        ReplaceHashJSONOp(key=keys.balances_rows(), mapping=mapping),
    ]
