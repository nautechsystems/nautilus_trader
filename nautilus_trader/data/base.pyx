# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class Data:
    """
    Represents wrapped data which includes data type information.
    """

    def __init__(self, DataType data_type not None, data not None):
        """
        Initialize a new instance of the `Data` class.

        Parameters
        ----------
        data_type : DataType
            The data type.
        data : object
            The data object to wrap.

        """
        self.data_type = data_type
        self.data = data


cdef class DataType:
    """
    Represents a data type including its metadata.
    """

    def __init__(self, type data_type not None, dict metadata=None):
        """
        Initialize a new instance of the `DataType` class.

        Parameters
        ----------
        data_type : type
            The PyObject type of the data.
        metadata : dict
            The data types metadata.

        """
        if metadata is None:
            metadata = {}

        self.type = data_type
        self.metadata = metadata

    def __eq__(self, DataType other) -> bool:
        return self.type == other.type and self.metadata == other.metadata

    def __ne__(self, DataType other) -> bool:
        return self.type != other.type or self.metadata != other.metadata

    def __hash__(self) -> int:
        return hash((self.type, self.metadata))

    def __str__(self) -> str:
        return f"<{self.type.__name__}> {self.metadata}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}(type={self.type.__name__}, metadata={self.metadata})"


cdef class DataCacheFacade:
    """
    Provides a read-only facade for a `DataCache`.
    """

# -- QUERIES ---------------------------------------------------------------------------------------

    cpdef list symbols(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list instruments(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list quote_ticks(self, Symbol symbol):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list trade_ticks(self, Symbol symbol):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list bars(self, BarType bar_type):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Instrument instrument(self, Symbol symbol):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Price price(self, Symbol symbol, PriceType price_type):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef OrderBook order_book(self, Symbol symbol):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef QuoteTick quote_tick(self, Symbol symbol, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef TradeTick trade_tick(self, Symbol symbol, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Bar bar(self, BarType bar_type, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int quote_tick_count(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int trade_tick_count(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int bar_count(self, BarType bar_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint has_order_book(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint has_quote_ticks(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint has_trade_ticks(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint has_bars(self, BarType bar_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef object get_xrate(
        self,
        Venue venue,
        Currency from_currency,
        Currency to_currency,
        PriceType price_type=PriceType.MID,
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")
