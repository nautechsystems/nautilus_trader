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
"""
Defines the fundamental data types represented within the trading domain.
"""

from nautilus_trader.core.nautilus_pyo3.model import Bar as RustBar
from nautilus_trader.core.nautilus_pyo3.model import OrderBookDelta as RustOrderBookDelta
from nautilus_trader.core.nautilus_pyo3.model import QuoteTick as RustQuoteTick
from nautilus_trader.core.nautilus_pyo3.model import TradeTick as RustTradeTick
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.bar_aggregation import BarAggregation
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.book import NULL_ORDER
from nautilus_trader.model.data.book import BookOrder
from nautilus_trader.model.data.book import OrderBookDelta
from nautilus_trader.model.data.book import OrderBookDeltas
from nautilus_trader.model.data.status import InstrumentClose
from nautilus_trader.model.data.status import InstrumentStatus
from nautilus_trader.model.data.status import VenueStatus
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker


__all__ = [
    "Bar",
    "BarSpecification",
    "BarType",
    "GenericData",
    "NULL_ORDER",
    "BarAggregation",
    "DataType",
    "BookOrder",
    "OrderBookDelta",
    "OrderBookDeltas",
    "QuoteTick",
    "Ticker",
    "TradeTick",
    "InstrumentClose",
    "InstrumentStatus",
    "VenueStatus",
]


NAUTILUS_PYO3_DATA_TYPES: tuple[type, ...] = (
    RustOrderBookDelta,
    RustQuoteTick,
    RustTradeTick,
    RustBar,
)
