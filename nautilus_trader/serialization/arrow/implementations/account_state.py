# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import itertools
from typing import Dict, List

import orjson

from nautilus_trader.model.events.account import AccountState
from nautilus_trader.serialization.arrow.serializer import register_parquet


def serialize(state: AccountState):
    result = []
    base = state.to_dict(state)
    del base["balances"]
    del base["margins"]
    for balance in state.balances:
        balance_data = {
            "balance_total": balance.total.as_double(),
            "balance_locked": balance.locked.as_double(),
            "balance_free": balance.free.as_double(),
            "balance_currency": balance.currency.code,
        }
        result.append({**base, **balance_data})
    for margin in state.margins:
        margin_data = {
            "margin_initial": margin.initial.as_double(),
            "margin_maintenance": margin.maintenance.as_double(),
            "margin_currency": margin.currency.code,
            "margin_instrument_id": margin.instrument_id.value
            if margin.instrument_id is not None
            else None,  # noqa
        }
        result.append({**base, **margin_data})

    return result


def _deserialize(values):
    balances = []
    for v in values:
        total = v.get("balance_total")
        if total is None:
            continue
        balances.append(
            dict(
                total=total,
                locked=v["balance_locked"],
                free=v["balance_free"],
                currency=v["balance_currency"],
            )
        )

    margins = []
    for v in values:
        initial = v.get("margin_initial")
        if initial is None:
            continue
        margins.append(
            dict(
                initial=initial,
                maintenance=v["margin_maintenance"],
                currency=v["margin_currency"],
                instrument_id=v["margin_instrument_id"],
            )
        )

    state = {
        k: v
        for k, v in values[0].items()
        if not k.startswith("balance_") and not k.startswith("margin_")
    }
    state["balances"] = orjson.dumps(balances)
    state["margins"] = orjson.dumps(margins)

    return AccountState.from_dict(state)


def deserialize(data: List[Dict]):
    results = []
    for _, chunk in itertools.groupby(
        sorted(data, key=lambda x: x["event_id"]), key=lambda x: x["event_id"]
    ):
        chunk = list(chunk)  # type: ignore
        results.append(_deserialize(values=chunk))
    return sorted(results, key=lambda x: x.ts_init)


register_parquet(
    AccountState,
    serializer=serialize,
    deserializer=deserialize,
    chunk=True,
)
