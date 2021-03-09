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

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.datetime cimport from_unix_time_ms
from nautilus_trader.core.datetime cimport to_unix_time_ms
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport PriceTypeParser
from nautilus_trader.model.identifiers cimport Security
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
        aggregation : BarAggregation (Enum)
            The type of bar aggregation.
        price_type : PriceType (Enum)
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
        return self.step == other.step\
            and self.aggregation == other.aggregation\
            and self.price_type == other.price_type

    def __ne__(self, BarSpecification other) -> bool:
        return not self == other

    def __hash__(self) -> int:
        return hash((self.step, self.aggregation, self.price_type))

    def __str__(self) -> str:
        return f"{self.step}-{BarAggregationParser.to_str(self.aggregation)}-{PriceTypeParser.to_str(self.price_type)}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef BarSpecification from_str_c(str value):
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split('-', maxsplit=2)

        if len(pieces) != 3:
            raise ValueError(f"The BarSpecification string value was malformed, was {value}")

        return BarSpecification(
            int(pieces[0]),
            BarAggregationParser.from_str(pieces[1]),
            PriceTypeParser.from_str(pieces[2]),
        )

    @staticmethod
    def from_str(str value) -> BarSpecification:
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
        return BarSpecification.from_str_c(value)

    cpdef bint is_time_aggregated(self) except *:
        """
        Return a value indicating whether the aggregation method is time-driven.

        - BarAggregation.SECOND
        - BarAggregation.MINUTE
        - BarAggregation.HOUR
        - BarAggregation.DAY

        Returns
        -------
        bool

        """
        return self.aggregation == BarAggregation.SECOND\
            or self.aggregation == BarAggregation.MINUTE\
            or self.aggregation == BarAggregation.HOUR\
            or self.aggregation == BarAggregation.DAY

    cpdef bint is_threshold_aggregated(self) except *:
        """
        Return a value indicating whether the aggregation method is
        threshold-driven.

        - BarAggregation.TICK
        - BarAggregation.TICK_IMBALANCE
        - BarAggregation.VOLUME
        - BarAggregation.VOLUME_IMBALANCE
        - BarAggregation.VALUE
        - BarAggregation.VALUE_IMBALANCE

        Returns
        -------
        bool

        """
        return self.aggregation == BarAggregation.TICK\
            or self.aggregation == BarAggregation.TICK_IMBALANCE\
            or self.aggregation == BarAggregation.VOLUME\
            or self.aggregation == BarAggregation.VOLUME_IMBALANCE\
            or self.aggregation == BarAggregation.VALUE\
            or self.aggregation == BarAggregation.VALUE_IMBALANCE

    cpdef bint is_information_aggregated(self) except *:
        """
        Return a value indicating whether the aggregation method is
        information-driven.

        - BarAggregation.TICK_RUNS
        - BarAggregation.VOLUME_RUNS
        - BarAggregation.VALUE_RUNS

        Returns
        -------
        bool

        """
        return self.aggregation == BarAggregation.TICK_RUNS\
            or self.aggregation == BarAggregation.VOLUME_RUNS\
            or self.aggregation == BarAggregation.VALUE_RUNS


cdef class BarType:
    """
    Represents a bar type being the security and bar specification of bar data.
    """

    def __init__(
        self,
        Security security not None,
        BarSpecification bar_spec not None,
        internal_aggregation=True,
    ):
        """
        Initialize a new instance of the `BarType` class.

        Parameters
        ----------
        security : Security
            The bar types security.
        bar_spec : BarSpecification
            The bar types specification.
        internal_aggregation : bool
            If bars are aggregated internally by the platform. If True the
            `DataEngine` will subscribe to the necessary ticks and aggregate
            bars accordingly. Else if False then bars will be subscribed to
            directly from the exchange/broker.

        Notes
        -----
        It is expected that all bar aggregation methods other than time will be
        internally aggregated.

        """
        self.security = security
        self.spec = bar_spec
        self.is_internal_aggregation = internal_aggregation

    def __eq__(self, BarType other) -> bool:
        return self.security == other.security \
            and self.spec == other.spec \
            and self.is_internal_aggregation == other.is_internal_aggregation

    def __ne__(self, BarType other) -> bool:
        return not self == other

    def __hash__(self) -> int:
        return hash((self.security, self.spec))

    def __str__(self) -> str:
        return f"{self.security}-{self.spec}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self}, internal_aggregation={self.is_internal_aggregation})"

    @staticmethod
    cdef BarType from_serializable_str_c(str value, bint internal_aggregation=True):
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split('-', maxsplit=3)

        if len(pieces) != 4:
            raise ValueError(f"The BarType string value was malformed, was {value}")

        cdef Security security = Security.from_serializable_str_c(pieces[0])
        cdef BarSpecification bar_spec = BarSpecification(
            int(pieces[1]),
            BarAggregationParser.from_str(pieces[2]),
            PriceTypeParser.from_str(pieces[3]),
        )

        return BarType(security, bar_spec, internal_aggregation)

    @staticmethod
    def from_serializable_str(str value, bint internal_aggregation=False) -> BarType:
        """
        Return a bar type parsed from the given string.

        Parameters
        ----------
        value : str
            The bar type string to parse.
        internal_aggregation : bool
            If bars were aggregated internally by the platform.

        Returns
        -------
        BarType

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        return BarType.from_serializable_str_c(value, internal_aggregation)

    cpdef str to_serializable_str(self):
        """
        The serializable string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.security.to_serializable_str()}-{self.spec}"


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
            Condition.true(high_price >= low_price, 'high_price was < low_price')
            Condition.true(high_price >= close_price, 'high_price was < close_price')
            Condition.true(low_price <= close_price, 'low_price was > close_price')

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

    def __hash__(self) -> int:
        return hash((self.open, self.high, self.low, self.close, self.volume, self.timestamp))

    def __str__(self) -> str:
        return f"{self.open},{self.high},{self.low},{self.close},{self.volume},{format_iso8601(self.timestamp)}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef Bar from_serializable_str_c(str value):
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
            from_unix_time_ms(long(pieces[5])),
        )

    @staticmethod
    def from_serializable_str(str value) -> Bar:
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
        return Bar.from_serializable_str_c(value)

    cpdef str to_serializable_str(self):
        """
        The serializable string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.open},{self.high},{self.low},{self.close},{self.volume},{to_unix_time_ms(self.timestamp)}"


cdef class BarData:
    """
    Represents bar data of a `BarType` and `Bar`.
    """

    def __init__(self, BarType bar_type not None, Bar bar not None):
        """
        Initialize a new instance of the `BarData` class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the data.
        bar : Bar
            The bar data.

        """
        self.bar_type = bar_type
        self.bar = bar

    def __repr__(self) -> str:
        return f"{type(self).__name__}(bar_type={self.bar_type}, bar={self.bar})"
