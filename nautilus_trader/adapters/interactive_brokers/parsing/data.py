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

import hashlib

import orjson

from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import TradeId


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


def generate_trade_id(symbol: str, ts_event: int, price: str, size: str) -> TradeId:
    hash_values = (symbol, ts_event, price, size)
    h = hashlib.sha256(orjson.dumps(hash_values))
    return TradeId(h.hexdigest())
