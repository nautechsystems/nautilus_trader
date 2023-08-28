# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Optional

import msgspec
import pandas as pd
import pyarrow as pa
from pyarrow import RecordBatch

from nautilus_trader.model.currency import Currency
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import InstrumentId


def serialize(state: AccountState) -> RecordBatch:
    result: dict[tuple[Currency, Optional[InstrumentId]], dict] = {}

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
        },
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
            },
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
            },
        )

    return pa.RecordBatch.from_pylist(result.values(), schema=SCHEMA)


def _deserialize(values) -> AccountState:
    balances = []
    for v in values:
        total = v.get("balance_total")
        if total is None:
            continue
        balances.append(
            {
                "total": total,
                "locked": v["balance_locked"],
                "free": v["balance_free"],
                "currency": v["balance_currency"],
            },
        )

    margins = []
    for v in values:
        initial = v.get("margin_initial")
        if pd.isna(initial):
            continue
        margins.append(
            {
                "initial": initial,
                "maintenance": v["margin_maintenance"],
                "currency": v["margin_currency"],
                "instrument_id": v["margin_instrument_id"],
            },
        )

    state = {
        k: v
        for k, v in values[0].items()
        if not k.startswith("balance_") and not k.startswith("margin_")
    }
    state["balances"] = msgspec.json.encode(balances)
    state["margins"] = msgspec.json.encode(margins)

    return AccountState.from_dict(state)


def deserialize(data: pa.RecordBatch):
    account_states = []
    for event_id in data.column("event_id").unique().to_pylist():
        event = data.filter(pa.compute.equal(data["event_id"], event_id))
        account = _deserialize(values=event.to_pylist())
        account_states.append(account)
    return account_states


SCHEMA = pa.schema(
    {
        "account_id": pa.dictionary(pa.int16(), pa.string()),
        "account_type": pa.dictionary(pa.int8(), pa.string()),
        "base_currency": pa.dictionary(pa.int16(), pa.string()),
        "balance_total": pa.float64(),
        "balance_locked": pa.float64(),
        "balance_free": pa.float64(),
        "balance_currency": pa.dictionary(pa.int16(), pa.string()),
        "margin_initial": pa.float64(),
        "margin_maintenance": pa.float64(),
        "margin_currency": pa.dictionary(pa.int16(), pa.string()),
        "margin_instrument_id": pa.dictionary(pa.int64(), pa.string()),
        "reported": pa.bool_(),
        "info": pa.binary(),
        "event_id": pa.string(),
        "ts_event": pa.uint64(),
        "ts_init": pa.uint64(),
    },
    metadata={"type": "AccountState"},
)
