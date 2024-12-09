# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
"""
The `model` subpackage defines a rich trading domain model.

The domain model is agnostic of any system design, seeking to represent the logic and
state transitions of trading in a generic way. Many system implementations could be
built around this domain model.

"""

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.book import Level
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.greeks import GreeksData
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position


# Defines all order book data types (capable of updating an L2_MBP and L3_MBO book)
BOOK_DATA_TYPES: set[type] = {
    OrderBookDelta,
    OrderBookDeltas,
    OrderBookDepth10,
}

NAUTILUS_PYO3_DATA_TYPES: tuple[type, ...] = (
    nautilus_pyo3.OrderBookDelta,
    nautilus_pyo3.OrderBookDepth10,
    nautilus_pyo3.QuoteTick,
    nautilus_pyo3.TradeTick,
    nautilus_pyo3.Bar,
)


__all__ = [
    "AccountBalance",
    "AccountId",
    "Bar",
    "BarSpecification",
    "BarType",
    "BookOrder",
    "ClientId",
    "ClientOrderId",
    "ComponentId",
    "Currency",
    "CustomData",
    "DataType",
    "ExecAlgorithmId",
    "GreeksData",
    "InstrumentClose",
    "InstrumentId",
    "InstrumentStatus",
    "Level",
    "MarginBalance",
    "Money",
    "OrderBook",
    "OrderBookDelta",
    "OrderBookDeltas",
    "OrderBookDepth10",
    "OrderListId",
    "Position",
    "PositionId",
    "Price",
    "Quantity",
    "QuoteTick",
    "StrategyId",
    "Symbol",
    "TradeId",
    "TradeTick",
    "TraderId",
    "Venue",
    "VenueOrderId",
]
