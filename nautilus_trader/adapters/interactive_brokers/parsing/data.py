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

import datetime
from decimal import Decimal

# fmt: off
from nautilus_trader.core.datetime import nanos_to_secs
from nautilus_trader.model.data import BarAggregation
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import TradeId


# fmt: on

MKT_DEPTH_OPERATIONS = {
    0: BookAction.ADD,
    1: BookAction.UPDATE,
    2: BookAction.DELETE,
}

IB_SIDE = {1: OrderSide.BUY, 0: OrderSide.SELL}

# TODO
IB_TICK_TYPE = {
    1: "Last",
    2: "AllLast",
    3: "BidAsk",
    4: "MidPoint",
}


def what_to_show(bar_type: BarType) -> str:
    mapping = {
        PriceType.ASK: "ASK",
        PriceType.BID: "BID",
        PriceType.LAST: "TRADES",
        PriceType.MID: "MIDPOINT",
    }
    if str(bar_type.instrument_id.venue) == "PAXOS" and bar_type.spec.price_type == PriceType.LAST:
        return "AGGTRADES"
    else:
        return mapping[bar_type.spec.price_type]


def generate_trade_id(ts_event: int, price: float, size: Decimal) -> TradeId:
    trade_id = TradeId(f"{int(nanos_to_secs(ts_event))}-{price}-{size}")
    assert len(trade_id.value) < 36, f"TradeId too long, was {len(id.value)}"  # type: ignore
    return trade_id


def bar_spec_to_bar_size(bar_spec: BarSpecification) -> str:
    aggregation = bar_spec.aggregation
    step = bar_spec.step
    if (aggregation == BarAggregation.SECOND and step == 5) or (
        aggregation == BarAggregation.SECOND and step in [10, 15, 30]
    ):
        return f"{step} secs"
    elif aggregation == BarAggregation.MINUTE and step in [1, 2, 3, 5, 10, 15, 20, 30]:
        return f"{step} min{'' if step == 1 else 's'}"
    elif aggregation == BarAggregation.HOUR and step in [1, 2, 3, 4, 8]:
        return f"{step} hour{'' if step == 1 else 's'}"
    elif aggregation == BarAggregation.DAY and step == 1:
        return f"{step} day"
    elif aggregation == BarAggregation.WEEK and step == 1:
        return f"{step} week"
    else:
        raise ValueError(
            f"InteractiveBrokers doesn't support subscription for {bar_spec!r}",
        )


def timedelta_to_duration_str(duration: datetime.timedelta) -> str:
    if duration.days >= 365:
        return f"{duration.days / 365:.0f} Y"
    elif duration.days >= 30:
        return f"{duration.days / 30:.0f} M"
    elif duration.days >= 7:
        return f"{duration.days / 7:.0f} W"
    elif duration.days >= 1:
        return f"{duration.days:.0f} D"
    else:
        return f"{max(30, duration.total_seconds()):.0f} S"
