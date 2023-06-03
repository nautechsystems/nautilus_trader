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

from typing import Callable

import pandas as pd

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType


map_trigger_method: dict[int, int] = {
    TriggerType.DEFAULT: 0,
    TriggerType.DOUBLE_BID_ASK: 1,
    TriggerType.LAST_TRADE: 2,
    TriggerType.DOUBLE_LAST: 3,
    TriggerType.BID_ASK: 4,
    TriggerType.LAST_OR_BID_ASK: 7,
    TriggerType.MID_POINT: 8,
}

map_time_in_force: dict[int, str] = {
    TimeInForce.DAY: "DAY",
    TimeInForce.GTC: "GTC",
    TimeInForce.IOC: "IOC",
    TimeInForce.GTD: "GTD",
    TimeInForce.AT_THE_OPEN: "OPG",
    TimeInForce.FOK: "FOK",
    # unsupported: 'DTC',
}

map_order_action: dict[int, str] = {
    OrderSide.BUY: "BUY",
    OrderSide.SELL: "SELL",
}

order_side_to_order_action: dict[str, str] = {
    "BOT": "BUY",
    "SLD": "SELL",
}

map_order_type: dict[int, str] = {
    OrderType.LIMIT: "LMT",
    OrderType.LIMIT_IF_TOUCHED: "LIT",
    OrderType.MARKET: "MKT",
    OrderType.MARKET_IF_TOUCHED: "MIT",
    OrderType.MARKET_TO_LIMIT: "MTL",
    OrderType.STOP_LIMIT: "STP LMT",
    OrderType.STOP_MARKET: "STP",
    OrderType.TRAILING_STOP_LIMIT: "TRAIL LIMIT",
    OrderType.TRAILING_STOP_MARKET: "TRAIL",
}


map_order_fields: set[tuple[str, str, Callable]] = {
    # ref: (nautilus_order_field, ib_order_field, value_fn)
    ("client_order_id", "orderRef", lambda x: x.value),
    ("display_qty", "displaySize", lambda x: x.as_double()),
    ("expire_time", "goodTillDate", lambda x: x.strftime("%Y%m%d %H:%M:%S %Z")),
    ("limit_offset", "lmtPriceOffset", lambda x: float(x)),
    ("order_type", "orderType", lambda x: map_order_type[x]),
    ("price", "lmtPrice", lambda x: x.as_double()),
    ("quantity", "totalQuantity", lambda x: x.as_decimal()),
    ("side", "action", lambda x: map_order_action[x]),
    ("time_in_force", "tif", lambda x: map_time_in_force[x]),
    # ("trailing_offset", "trailStopPrice", lambda x: float(x)),
    # ("trigger_price", "auxPrice", lambda x: x.as_double()),
    # ("trigger_type", "triggerMethod", lambda x: map_trigger_method[x]),
    ("parent_order_id", "parentId", lambda x: x.value),
}


map_order_status = {
    "ApiPending": OrderStatus.SUBMITTED,
    "PendingSubmit": OrderStatus.SUBMITTED,
    "PendingCancel": OrderStatus.PENDING_CANCEL,
    "PreSubmitted": OrderStatus.SUBMITTED,
    "Submitted": OrderStatus.ACCEPTED,
    "ApiCancelled": OrderStatus.CANCELED,
    "Cancelled": OrderStatus.CANCELED,
    "Filled": OrderStatus.FILLED,
    "Inactive": OrderStatus.DENIED,
}


def timestring_to_timestamp(timestring: str) -> pd.Timestamp:
    # Support string conversion not supported directly by pd.to_datetime
    # 20230223 00:43:36 America/New_York
    # 20230223 00:43:36 Universal
    dt, tz = timestring.rsplit(" ", 1)
    return pd.Timestamp(dt, tz=tz)
