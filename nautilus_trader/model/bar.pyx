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
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport PriceTypeParser
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef list _TIME_AGGREGATED = [
    BarAggregation.SECOND,
    BarAggregation.MINUTE,
    BarAggregation.HOUR,
    BarAggregation.DAY,
]

cdef class BarSpecification:
    """
    Represents an aggregation specification for generating bars.
    """
    def __init__(
            self,
            int step,
            BarAggregation aggregation,
            PriceType price_type,
    ):
        """
        Initialize a new instance of the `BarSpecification` class.

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

    def __eq__(self, BarSpecification other) -> bool:
        return self.step == other.step \
            and self.aggregation == other.aggregation \
            and self.price_type == other.price_type

    def __ne__(self, BarSpecification other) -> bool:
        return not self == other

    def __hash__(self) -> int:
        return hash((self.step, self.aggregation, self.price_type))

    def __str__(self) -> str:
        return f"{self.step}-{BarAggregationParser.to_string(self.aggregation)}-{PriceTypeParser.to_string(self.price_type)}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef BarSpecification from_string_c(str value):
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split('-', maxsplit=2)

        if len(pieces) != 3:
            raise ValueError(f"The BarSpecification string value was malformed, was {value}")

        return BarSpecification(
            int(pieces[0]),
            BarAggregationParser.from_string(pieces[1]),
            PriceTypeParser.from_string(pieces[2]),
        )

    @staticmethod
    def from_string(str value) -> BarSpecification:
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
        return BarSpecification.from_string_c(value)

    cdef bint is_time_aggregated(self) except *:
        """
        If the aggregation is by certain time interval.

        Returns
        -------
        bool

        """
        return self.aggregation in _TIME_AGGREGATED

    cdef str aggregation_string(self):
        """
        The bar aggregation as a string.

        Returns
        -------
        str

        """
        return self.aggregation_string()

    cdef str price_type_string(self):
        """
        The price type as a string.

        Returns
        -------
        str

        """
        return PriceTypeParser.to_string(self.price_type)


cdef class BarType:
    """
    Represents the symbol and bar specification or a bar or block of bars.
    """

    def __init__(
            self,
            Symbol symbol not None,
            BarSpecification bar_spec not None,
    ):
        """
        Initialize a new instance of the `BarType` class.

        Parameters
        ----------
        symbol : Symbol
            The bar symbol.
        bar_spec : BarSpecification
            The bar specification.

        """
        self.symbol = symbol
        self.spec = bar_spec

    def __eq__(self, BarType other) -> bool:
        return self.symbol == other.symbol and self.spec == other.spec

    def __ne__(self, BarType other) -> bool:
        return not self == other

    def __hash__(self) -> int:
        return hash((self.symbol, self.spec))

    def __str__(self) -> str:
        return f"{self.symbol}-{self.spec}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef BarType from_string_c(str value):
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split('-', maxsplit=3)

        if len(pieces) != 4:
            raise ValueError(f"The BarType string value was malformed, was {value}")

        cdef Symbol symbol = Symbol.from_string_c(pieces[0])
        cdef BarSpecification bar_spec = BarSpecification(
            int(pieces[1]),
            BarAggregationParser.from_string(pieces[2]),
            PriceTypeParser.from_string(pieces[3]),
        )

        return BarType(symbol, bar_spec)

    @staticmethod
    def from_string(str value) -> BarType:
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
        return BarType.from_string_c(value)


cdef class Bar:
    """
    Represents an aggregated bar.
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
        Initialize a new instance of the `Bar` class.

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
            Condition.true(high_price >= low_price, 'high_price >= low_price')
            Condition.true(high_price >= close_price, 'high_price >= close_price')
            Condition.true(low_price <= close_price, 'low_price <= close_price')

        self.open = open_price
        self.high = high_price
        self.low = low_price
        self.close = close_price
        self.volume = volume
        self.timestamp = timestamp
        self.checked = check

    def __eq__(self, Bar other) -> bool:
        return self.open == other.open \
            and self.high == other.high \
            and self.low == other.low \
            and self.close == other.close \
            and self.volume == other.volume \
            and self.timestamp == other.timestamp

    def __ne__(self, Bar other) -> bool:
        return not self == other

    def __str__(self) -> str:
        return f"{self.open},{self.high},{self.low},{self.close},{self.volume},{format_iso8601(self.timestamp)}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    # noinspection: fromtimestamp, long
    # noinspection PyUnresolvedReferences
    @staticmethod
    cdef Bar from_serializable_string_c(str value):
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split(',', maxsplit=5)

        if len(pieces) != 6:
            raise ValueError(f"The Bar string value was malformed, was {value}")

        return Bar(
            Price(pieces[0]),
            Price(pieces[1]),
            Price(pieces[2]),
            Price(pieces[3]),
            Quantity(pieces[4]),
            datetime.fromtimestamp(long(pieces[5]) / 1000, pytz.utc),
        )

    @staticmethod
    def from_serializable_string(str value) -> Bar:
        """
        Parse a bar parsed from the given string.

        Parameters
        ----------
        value : str
            The bar string to parse.

        Returns
        -------
        Bar

        """
        return Bar.from_serializable_string_c(value)

    # noinspection: timestamp()
    # noinspection PyUnresolvedReferences
    cpdef str to_serializable_string(self):
        """
        The serializable string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.open},{self.high},{self.low},{self.close},{self.volume},{long(self.timestamp.timestamp())}"
