# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import pytz

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport bar_aggregation_from_string
from nautilus_trader.model.c_enums.bar_aggregation cimport bar_aggregation_to_string
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport price_type_from_string
from nautilus_trader.model.c_enums.price_type cimport price_type_to_string
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BarSpecification:
    """
    Represents the specification of a financial market trade bar.
    """
    def __init__(
            self,
            int step,
            BarAggregation aggregation not None,
            PriceType price_type not None,
    ):
        """
        Initialize a new instance of the BarSpecification class.

        Parameters
        ----------
        step : int
            The step for binning samples for bar aggregation (> 0).
        aggregation : BarAggregation
            The type of bar aggregation.
        price_type : PriceType
            The price type to use for aggregation.

        Raises
        ------
        ValueError
            If step is not positive (> 0).
        ValueError
            If aggregation is UNDEFINED.
        ValueError
            If price type is UNDEFINED.

        """
        Condition.positive_int(step, 'step')
        Condition.not_equal(aggregation, BarAggregation.UNDEFINED, 'aggregation', 'UNDEFINED')
        Condition.not_equal(price_type, PriceType.UNDEFINED, 'price_type', 'UNDEFINED')

        self.step = step
        self.aggregation = aggregation
        self.price_type = price_type

    @staticmethod
    cdef BarSpecification from_string(str value):
        """
        Return a bar specification parsed from the given string.
        String format example is '200-TICK-MID'.

        Parameters
        ----------
        value : str
            The bar specification string to parse.

        Returns
        -------
        BarSpecification

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        Condition.valid_string(value, 'value')

        cdef list split = value.split('-', maxsplit=3)

        return BarSpecification(
            int(split[0]),
            bar_aggregation_from_string(split[1]),
            price_type_from_string(split[2]),
        )

    @staticmethod
    def py_from_string(str value) -> BarSpecification:
        """
        Return a bar specification parsed from the given string.

        Parameters
        ----------
        value : str
            The bar specification string to parse.

        Examples
        --------
        String format example is '200-TICK-MID'.

        Returns
        -------
        BarSpecification

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        return BarSpecification.from_string(value)

    cdef str aggregation_string(self):
        """
        Return the bar aggregation as a string

        Returns
        -------
        str

        """
        return bar_aggregation_to_string(self.aggregation)

    cdef str price_type_string(self):
        """
        Return the price type as a string.

        Returns
        -------
        str

        """
        return price_type_to_string(self.price_type)

    cpdef bint equals(self, BarSpecification other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.step == other.step \
            and self.aggregation == other.aggregation \
            and self.price_type == other.price_type

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.step}-{bar_aggregation_to_string(self.aggregation)}-{price_type_to_string(self.price_type)}"

    def __eq__(self, BarSpecification other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.equals(other)

    def __ne__(self, BarSpecification other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash((self.step, self.aggregation, self.price_type))

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{self.__class__.__name__}({self.to_string()}) object at {id(self)}>"


cdef list _TIME_BARS = [
    BarAggregation.SECOND,
    BarAggregation.MINUTE,
    BarAggregation.HOUR,
    BarAggregation.DAY,
]


cdef class BarType:
    """
    Represents a financial market symbol and bar specification.
    """

    def __init__(
            self,
            Symbol symbol not None,
            BarSpecification bar_spec not None,
    ):
        """
        Initialize a new instance of the BarType class.

        Parameters
        ----------
        symbol : Symbol
            The bar symbol.
        bar_spec : BarSpecification
            The bar specification.

        """
        self.symbol = symbol
        self.spec = bar_spec

    @staticmethod
    cdef BarType from_string(str value):
        """
        Return a bar type parsed from the given string.

        Parameters
        ----------
        value : str
            The bar type string to parse.

        Returns
        -------
        BarType

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        Condition.valid_string(value, 'value')

        cdef list split = value.split('-', maxsplit=3)
        cdef list symbol_split = split[0].split('.', maxsplit=1)

        cdef Symbol symbol = Symbol(symbol_split[0], Venue(symbol_split[1]))
        cdef BarSpecification bar_spec = BarSpecification(
            int(split[1]),
            bar_aggregation_from_string(split[2]),
            price_type_from_string(split[3]),
        )

        return BarType(symbol, bar_spec)

    @staticmethod
    def py_from_string(str value) -> BarType:
        """
        Return a bar type parsed from the given string.

        Parameters
        ----------
        value : str
            The bar type string to parse.

        Returns
        -------
        BarType

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        return BarType.from_string(value)

    cdef bint is_time_aggregated(self):
        """
        Return a value indicating whether the aggregation is a measure of time.

        Returns
        -------
        bool

        """
        return self.spec.aggregation in _TIME_BARS

    cdef str aggregation_string(self):
        """
        Return the bar aggregation as a string

        :return str.
        """
        return self.spec.aggregation_string()

    cdef str price_type_string(self):
        """
        Return the price type as a string.

        :return str.
        """
        return self.spec.price_type_string()

    cpdef bint equals(self, BarType other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.symbol.equals(other.symbol) and self.spec.equals(other.spec)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.symbol.to_string()}-{self.spec}"

    def __eq__(self, BarType other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.equals(other)

    def __ne__(self, BarType other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash((self.symbol, self.spec))

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{self.__class__.__name__}({self.to_string()}) object at {id(self)}>"


cdef class Bar:
    """
    Represents a financial market trade bar.
    """

    def __init__(
            self,
            Price open_price not None,
            Price high_price not None,
            Price low_price not None,
            Price close_price not None,
            Quantity volume not None,
            datetime timestamp not None,
            bint check=False,
    ):
        """
        Initialize a new instance of the Bar class.

        Parameters
        ----------
        open_price : Price
            The bars open price.
        high_price : Price
            The bars high price.
        low_price : Price
            The bars low price.
        close_price : Price
            The bars close price.
        volume : Quantity
            The bars volume.
        timestamp : datetime
            The bars timestamp (UTC).
        check : bool
            If bar parameters should be checked valid.

        Raises
        ------
        ValueError
            If check True and the high_price is not >= low_price.
        ValueError
            If check True and the high_price is not >= close_price.
        ValueError
            If check True and the low_price is not <= close_price.

        """
        if check:
            Condition.true(high_price.ge(low_price), 'high_price >= low_price')
            Condition.true(high_price.ge(close_price), 'high_price >= close_price')
            Condition.true(low_price.le(close_price), 'low_price <= close_price')

        self.open = open_price
        self.high = high_price
        self.low = low_price
        self.close = close_price
        self.volume = volume
        self.timestamp = timestamp
        self.checked = check

    @staticmethod
    cdef Bar from_serializable_string(str value):
        """
        Return a bar parsed from the given string.

        Parameters
        ----------
        value : str
            The bar string to parse.

        Returns
        -------
        Bar

        """
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split(',', maxsplit=5)

        return Bar(
            Price.from_string(pieces[0]),
            Price.from_string(pieces[1]),
            Price.from_string(pieces[2]),
            Price.from_string(pieces[3]),
            Quantity.from_string(pieces[4]),
            datetime.fromtimestamp(long(pieces[5]) / 1000, pytz.utc),
        )

    @staticmethod
    def py_from_serializable_string(str value) -> Bar:
        """
        Python wrapper for the from_string method.

        Return a bar parsed from the given string.

        Parameters
        ----------
        value : str
            The bar string to parse.

        Returns
        -------
        Bar

        """
        return Bar.from_serializable_string(value)

    cpdef bint equals(self, Bar other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.open.equals(other.open) \
            and self.high.equals(other.high) \
            and self.low.equals(other.low) \
            and self.close.equals(other.close) \
            and self.volume.equals(other.volume) \
            and self.timestamp == other.timestamp

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.open.to_string()},"
                f"{self.high.to_string()},"
                f"{self.low.to_string()},"
                f"{self.close.to_string()},"
                f"{self.volume.to_string()},"
                f"{format_iso8601(self.timestamp)}")

    cpdef str to_serializable_string(self):
        """
        Return the serializable string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self.open.to_string()},"
                f"{self.high.to_string()},"
                f"{self.low.to_string()},"
                f"{self.close.to_string()},"
                f"{self.volume.to_string()},"
                f"{long(self.timestamp.timestamp())}")

    def __eq__(self, Bar other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : Bar
            The other object.

        Returns
        -------
        bool

        """
        return self.equals(other)

    def __ne__(self, Bar other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : Bar
            The other object.

        Returns
        -------
        bool

        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(str(self.timestamp))

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{self.__class__.__name__}({self.to_string()}) object at {id(self)}>"
