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
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


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

        self._step = step
        self._aggregation = aggregation
        self._price_type = price_type

    def __eq__(self, BarSpecification other) -> bool:
        return self._step == other.step \
            and self._aggregation == other.aggregation \
            and self._price_type == other.price_type

    def __ne__(self, BarSpecification other) -> bool:
        return not self == other

    def __hash__(self) -> int:
        return hash((self._step, self._aggregation, self._price_type))

    def __str__(self) -> str:
        return f"{self._step}-{bar_aggregation_to_string(self._aggregation)}-{price_type_to_string(self._price_type)}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @property
    def step(self):
        """
        The specified step size for bar aggregation.

        Returns
        -------
        int

        """
        return self._step

    @property
    def aggregation(self):
        """
        The specified aggregation method for bars.

        Returns
        -------
        BarAggregation

        """
        return self._aggregation

    @property
    def price_type(self):
        """
        The specified price type for bar aggregation.

        Returns
        -------
        PriceType

        """
        return self._price_type

    @staticmethod
    cdef BarSpecification from_string_c(str value):
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

        cdef list pieces = value.split('-', maxsplit=2)

        if len(pieces) < 3:
            raise ValueError(f"The BarSpecification string value was malformed, was {value}")

        return BarSpecification(
            int(pieces[0]),
            bar_aggregation_from_string(pieces[1]),
            price_type_from_string(pieces[2]),
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

    cdef str aggregation_string(self):
        """
        Return the bar aggregation as a string

        Returns
        -------
        str

        """
        return bar_aggregation_to_string(self._aggregation)

    cdef str price_type_string(self):
        """
        Return the price type as a string.

        Returns
        -------
        str

        """
        return price_type_to_string(self._price_type)


cdef list _TIME_BARS = [
    BarAggregation.SECOND,
    BarAggregation.MINUTE,
    BarAggregation.HOUR,
    BarAggregation.DAY,
]


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
        self._symbol = symbol
        self._spec = bar_spec

    def __eq__(self, BarType other) -> bool:
        return self._symbol == other.symbol and self._spec == other.spec

    def __ne__(self, BarType other) -> bool:
        return not self == other

    def __hash__(self) -> int:
        return hash((self._symbol, self._spec))

    def __str__(self) -> str:
        return f"{self._symbol}-{self._spec}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @property
    def symbol(self):
        """
        Returns
        -------
        Symbol
            The symbol of the bar type.

        """
        return self._symbol

    @property
    def spec(self):
        """
        Returns
        -------
        BarSpecification
            The specification of the bar type.

        """
        return self._spec

    @staticmethod
    cdef BarType from_string_c(str value):
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

        cdef list pieces = value.split('-', maxsplit=3)

        if len(pieces) < 4:
            raise ValueError(f"The BarType string value was malformed, was {value}")

        cdef BarSpecification bar_spec = BarSpecification(
            int(pieces[1]),
            bar_aggregation_from_string(pieces[2]),
            price_type_from_string(pieces[3]),
        )

        return BarType(Symbol.from_string_c(pieces[0]), bar_spec)

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

    cdef bint is_time_aggregated(self) except *:
        """
        Return a value indicating whether the aggregation is a measure of time.

        Returns
        -------
        bool

        """
        return self._spec.aggregation in _TIME_BARS

    cdef str aggregation_string(self):
        """
        Return the bar aggregation as a string

        Returns
        -------
        str

        """
        return self._spec.aggregation_string()

    cdef str price_type_string(self):
        """
        Return the price type as a string.

        Returns
        -------
        str

        """
        return self._spec.price_type_string()


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

        self._open = open_price
        self._high = high_price
        self._low = low_price
        self._close = close_price
        self._volume = volume
        self._timestamp = timestamp
        self._checked = check

    def __eq__(self, Bar other) -> bool:
        return self._open == other.open \
            and self._high == other.high \
            and self._low == other.low \
            and self._close == other.close \
            and self._volume == other.volume \
            and self._timestamp == other.timestamp

    def __ne__(self, Bar other) -> bool:
        return not self == other

    def __hash__(self) -> int:
        return hash(str(self._timestamp))

    def __str__(self) -> str:
        return f"{self._open},{self._high},{self._low},{self._close},{self._volume},{format_iso8601(self._timestamp)}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @property
    def open(self):
        """
        The open price of the bar.

        Returns
        -------
        Price

        """
        return self._open

    @property
    def high(self):
        """
        The high price of the bar.

        Returns
        -------
        Price

        """
        return self._high

    @property
    def low(self):
        """
        The low price of the bar.

        Returns
        -------
        Price

        """
        return self._low

    @property
    def close(self):
        """
        The close price of the bar.

        Returns
        -------
        Price

        """
        return self._close

    @property
    def volume(self):
        """
        The volume of the bar.

        Returns
        -------
        Quantity

        """
        return self._volume

    @property
    def timestamp(self):
        """
        The timestamp the bar closed at.

        Returns
        -------
        datetime

        """
        return self._timestamp

    @property
    def checked(self):
        """
        If the input values were integrity checked.

        Returns
        -------
        bool

        """
        return self._checked

    @staticmethod
    cdef Bar from_serializable_string_c(str value):
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
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split(',', maxsplit=5)

        if len(pieces) < 5:
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

    cpdef str to_serializable_string(self):
        """
        The serializable string representation of this object.

        Returns
        -------
        str

        """
        return f"{self._open},{self._high},{self._low},{self._close},{self._volume},{long(self._timestamp.timestamp())}"
