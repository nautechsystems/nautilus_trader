# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import TYPE_CHECKING

from nautilus_trader.adapters.bybit.common.constants import BYBIT_HOUR_INTERVALS
from nautilus_trader.adapters.bybit.common.constants import BYBIT_MINUTE_INTERVALS
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import bar_aggregation_to_str


if TYPE_CHECKING:
    from nautilus_trader.model.identifiers import InstrumentId
    from nautilus_trader.model.objects import Price
    from nautilus_trader.model.objects import Quantity


def parse_aggressor_side(value: str) -> AggressorSide:
    match value:
        case "Buy":
            return AggressorSide.BUYER
        case "Sell":
            return AggressorSide.SELLER
        case _:
            raise ValueError(f"Invalid aggressor side value, was '{value}'")


def parse_bybit_delta(
    instrument_id: InstrumentId,
    values: tuple[Price, Quantity],
    side: OrderSide,
    update_id: int,
    flags: int,
    sequence: int,
    ts_event: int,
    ts_init: int,
    snapshot: bool,
) -> OrderBookDelta:
    price = values[0]
    size = values[1]
    if snapshot:
        action = BookAction.ADD
    else:
        action = BookAction.DELETE if size == 0 else BookAction.UPDATE

    return OrderBookDelta(
        instrument_id=instrument_id,
        action=action,
        order=BookOrder(
            side=side,
            price=price,
            size=size,
            order_id=update_id,
        ),
        flags=flags,
        sequence=sequence,
        ts_event=ts_event,
        ts_init=ts_init,
    )


def get_interval_from_bar_type(bar_type: BarType) -> str:
    aggregation: BarAggregation = bar_type.spec.aggregation
    match aggregation:
        case BarAggregation.MINUTE:
            if bar_type.spec.step not in BYBIT_MINUTE_INTERVALS:
                raise ValueError(
                    f"Bybit only supports the following bar minute intervals: "
                    f"{BYBIT_MINUTE_INTERVALS}",
                )
            return str(bar_type.spec.step)
        case BarAggregation.HOUR:
            if bar_type.spec.step not in BYBIT_HOUR_INTERVALS:
                raise ValueError(
                    f"Bybit only supports the following bar hour intervals: "
                    f"{BYBIT_HOUR_INTERVALS}",
                )
            return str(bar_type.spec.step * 60)
        case BarAggregation.DAY:
            if bar_type.spec.step != 1:
                raise ValueError("Bybit only supports 1 DAY interval bars")
            return "D"
        case BarAggregation.WEEK:
            if bar_type.spec.step == 1:
                return "W"
            if bar_type.spec.step == 4:
                return "M"
            raise ValueError("Bybit only supports 1 WEEK interval bars")
        case _:
            raise ValueError(
                f"Bybit does not support {bar_aggregation_to_str(bar_type.aggregation)} bars",
            )
