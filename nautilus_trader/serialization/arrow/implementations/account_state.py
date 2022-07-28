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
from typing import Dict, List, Optional, Tuple

import msgspec
import pandas as pd

from nautilus_trader.model.currency import Currency
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.serialization.arrow.serializer import register_parquet


def serialize(state: AccountState):
    result: Dict[Tuple[Currency, Optional[InstrumentId]], Dict] = {}

    base = state.to_dict(state)
    del base["balances"]
    del base["margins"]
    base.update(
        {
            "balance_total": None,
            "balance_locked": None,
            "balance_free": None,
            "balance_currency": None,
            "margin_initial": None,
            "margin_maintenance": None,
            "margin_currency": None,
            "margin_instrument_id": None,
        }
    )

    for balance in state.balances:
        key = (balance.currency, None)
        if key not in result:
            result[key] = base.copy()
        result[key].update(
            {
                "balance_total": balance.total.as_double(),
                "balance_locked": balance.locked.as_double(),
                "balance_free": balance.free.as_double(),
                "balance_currency": balance.currency.code,
            }
        )

    for margin in state.margins:
        key = (margin.currency, margin.instrument_id)
        if key not in result:
            result[key] = base.copy()
        result[key].update(
            {
                "margin_initial": margin.initial.as_double(),
                "margin_maintenance": margin.maintenance.as_double(),
                "margin_currency": margin.currency.code,
                "margin_instrument_id": margin.instrument_id.value,
            }
        )

    return list(result.values())


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
        if pd.isnull(initial):
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
    state["balances"] = msgspec.json.encode(balances)
    state["margins"] = msgspec.json.encode(margins)

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
