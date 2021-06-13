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

from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport PriceTypeParser
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId
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
        Initialize a new instance of the ``BarSpecification`` class.

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

        """
        Condition.positive_int(step, 'step')

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
        return (
            self.aggregation == BarAggregation.SECOND
            or self.aggregation == BarAggregation.MINUTE
            or self.aggregation == BarAggregation.HOUR
            or self.aggregation == BarAggregation.DAY
        )

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
        return (
            self.aggregation == BarAggregation.TICK
            or self.aggregation == BarAggregation.TICK_IMBALANCE
            or self.aggregation == BarAggregation.VOLUME
            or self.aggregation == BarAggregation.VOLUME_IMBALANCE
            or self.aggregation == BarAggregation.VALUE
            or self.aggregation == BarAggregation.VALUE_IMBALANCE
        )

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
        return (
            self.aggregation == BarAggregation.TICK_RUNS
            or self.aggregation == BarAggregation.VOLUME_RUNS
            or self.aggregation == BarAggregation.VALUE_RUNS
        )


cdef class BarType:
    """
    Represents a bar type being the instrument identifier and bar specification of bar data.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BarSpecification bar_spec not None,
        internal_aggregation=True,
    ):
        """
        Initialize a new instance of the ``BarType`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The bar types instrument identifier.
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
        self.instrument_id = instrument_id
        self.spec = bar_spec
        self.is_internal_aggregation = internal_aggregation

    def __eq__(self, BarType other) -> bool:
        return (
            self.instrument_id == other.instrument_id
            and self.spec == other.spec
            and self.is_internal_aggregation == other.is_internal_aggregation
        )

    def __ne__(self, BarType other) -> bool:
        return not self == other

    def __hash__(self) -> int:
        return hash((self.instrument_id, self.spec))

    def __str__(self) -> str:
        return f"{self.instrument_id}-{self.spec}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self}, internal_aggregation={self.is_internal_aggregation})"

    @staticmethod
    cdef BarType from_serializable_str_c(str value, bint internal_aggregation=True):
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split('-', maxsplit=3)

        if len(pieces) != 4:
            raise ValueError(f"The BarType string value was malformed, was {value}")

        cdef InstrumentId instrument_id = InstrumentId.from_str_c(pieces[0])
        cdef BarSpecification bar_spec = BarSpecification(
            int(pieces[1]),
            BarAggregationParser.from_str(pieces[2]),
            PriceTypeParser.from_str(pieces[3]),
        )

        return BarType(instrument_id, bar_spec, internal_aggregation)

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
        return f"{self.instrument_id}-{self.spec}"


cdef class Bar(Data):
    """
    Represents an aggregated bar.
    """

    def __init__(
        self,
        BarType bar_type not None,
        Price open_price not None,
        Price high_price not None,
        Price low_price not None,
        Price close_price not None,
        Quantity volume not None,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
        bint check=False,
    ):
        """
        Initialize a new instance of the ``Bar`` class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for this bar.
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
        ts_event_ns : int64
            The UNIX timestamp (nanos) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanos) when received by the Nautilus system.
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
        super().__init__(ts_event_ns, ts_recv_ns)

        self.type = bar_type
        self.open = open_price
        self.high = high_price
        self.low = low_price
        self.close = close_price
        self.volume = volume
        self.checked = check

    def __eq__(self, Bar other) -> bool:
        return (
            self.type == other.type
            and self.open == other.open
            and self.high == other.high
            and self.low == other.low
            and self.close == other.close
            and self.volume == other.volume
            and self.ts_recv_ns == other.ts_recv_ns
        )

    def __ne__(self, Bar other) -> bool:
        return not self == other

    def __hash__(self) -> int:
        return hash((self.type, self.open, self.high, self.low, self.close, self.volume, self.ts_event_ns))

    def __str__(self) -> str:
        return f"{self.type},{self.open},{self.high},{self.low},{self.close},{self.volume},{self.ts_event_ns}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "bar_type": self.type.to_serializable_str(),
            "open": str(self.open),
            "high": str(self.high),
            "low": str(self.low),
            "close": str(self.close),
            "volume": str(self.volume),
            "ts_event_ns": self.ts_event_ns,
            "ts_recv_ns": self.ts_recv_ns,
        }

    @staticmethod
    cdef Bar from_serializable_str_c(BarType bar_type, str values):
        Condition.valid_string(values, "values")

        cdef list pieces = values.split(',', maxsplit=6)

        if len(pieces) != 7:
            raise ValueError(f"The Bar string value was malformed, was {values}")

        return Bar(
            bar_type=bar_type,
            open_price=Price.from_str(pieces[0]),
            high_price=Price.from_str(pieces[1]),
            low_price=Price.from_str(pieces[2]),
            close_price=Price.from_str(pieces[3]),
            volume=Quantity.from_str(pieces[4]),
            ts_event_ns=int(pieces[5]),
            ts_recv_ns=int(pieces[6]),
        )

    @staticmethod
    def from_serializable_str(BarType bar_type, str values) -> Bar:
        """
        Parse a bar parsed from the given string.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the bar.
        values : str
            The bar values string to parse.

        Returns
        -------
        Bar

        """
        return Bar.from_serializable_str_c(bar_type, values)

    cpdef str to_serializable_str(self):
        """
        The serializable string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.open},{self.high},{self.low},{self.close},{self.volume},{self.ts_event_ns},{self.ts_recv_ns}"
