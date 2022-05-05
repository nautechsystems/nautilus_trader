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

from typing import Dict, Union

from nautilus_trader.model.events.position import PositionChanged
from nautilus_trader.model.events.position import PositionClosed
from nautilus_trader.model.events.position import PositionEvent
from nautilus_trader.model.events.position import PositionOpened
from nautilus_trader.model.objects import Money
from nautilus_trader.serialization.arrow.serializer import register_parquet


def try_float(x):
    if x == "None" or x is None:
        return
    return float(x)


def serialize(event: PositionEvent):
    data = {k: v for k, v in event.to_dict(event).items() if k not in ("order_fill",)}
    caster = {
        "net_qty": float,
        "quantity": float,
        "peak_qty": float,
        "avg_px_open": float,
        "last_qty": float,
        "last_px": float,
        "avg_px_close": try_float,
        "realized_return": try_float,
    }
    values = {k: caster[k](v) if k in caster else v for k, v in data.items()}  # type: ignore
    if "realized_pnl" in values:
        realized = Money.from_str(values["realized_pnl"])
        values["realized_pnl"] = realized.as_double()
    if "unrealized_pnl" in values:
        unrealized = Money.from_str(values["unrealized_pnl"])
        values["unrealized_pnl"] = unrealized.as_double()
    return values


def deserialize(cls):
    def inner(data: Dict) -> Union[PositionOpened, PositionChanged, PositionClosed]:
        for k in ("quantity", "last_qty", "peak_qty", "last_px"):
            if k in data:
                data[k] = str(data[k])
        if "realized_pnl" in data:
            data["realized_pnl"] = f"{data['realized_pnl']} {data['currency']}"
        if "unrealized_pnl" in data:
            data["unrealized_pnl"] = f"{data['unrealized_pnl']} {data['currency']}"
        return cls.from_dict(data)

    return inner


for cls in (PositionOpened, PositionChanged, PositionClosed):
    register_parquet(
        cls,
        serializer=serialize,
        deserializer=deserialize(cls=cls),
    )
