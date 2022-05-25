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

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.model.c_enums.aggregation_source cimport AggregationSourceParser
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport PriceTypeParser
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BarSpecification:
    """
    Represents a bar aggregation specification including a step, aggregation
    method/rule and price type.

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
        If `step` is not positive (> 0).
    """

    def __init__(
        self,
        int step,
        BarAggregation aggregation,
        PriceType price_type,
    ):
        Condition.positive_int(step, 'step')

        self.step = step
        self.aggregation = aggregation
        self.price_type = price_type

    def __eq__(self, BarSpecification other) -> bool:
        return (
            self.step == other.step
            and self.aggregation == other.aggregation
            and self.price_type == other.price_type
        )

    def __lt__(self, BarSpecification other) -> bool:
        return str(self) < str(other)

    def __le__(self, BarSpecification other) -> bool:
        return str(self) <= str(other)

    def __gt__(self, BarSpecification other) -> bool:
        return str(self) > str(other)

    def __ge__(self, BarSpecification other) -> bool:
        return str(self) >= str(other)

    def __hash__(self) -> int:
        return hash((self.step, self.aggregation, self.price_type))

    def __str__(self) -> str:
        return f"{self.step}-{BarAggregationParser.to_str(self.aggregation)}-{PriceTypeParser.to_str(self.price_type)}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cdef str aggregation_string_c(self):
        return BarAggregationParser.to_str(self.aggregation)

    @staticmethod
    cdef bint check_time_aggregated_c(BarAggregation aggregation):
        if (
            aggregation == BarAggregation.MILLISECOND
            or aggregation == BarAggregation.SECOND
            or aggregation == BarAggregation.MINUTE
            or aggregation == BarAggregation.HOUR
            or aggregation == BarAggregation.DAY
            or aggregation == BarAggregation.WEEK
            or aggregation == BarAggregation.MONTH
        ):
            return True
        else:
            return False

    @staticmethod
    cdef bint check_threshold_aggregated_c(BarAggregation aggregation):
        if (
            aggregation == BarAggregation.TICK
            or aggregation == BarAggregation.TICK_IMBALANCE
            or aggregation == BarAggregation.VOLUME
            or aggregation == BarAggregation.VOLUME_IMBALANCE
            or aggregation == BarAggregation.VALUE
            or aggregation == BarAggregation.VALUE_IMBALANCE
        ):
            return True
        else:
            return False

    @staticmethod
    cdef bint check_information_aggregated_c(BarAggregation aggregation):
        if (
            aggregation == BarAggregation.TICK_RUNS
            or aggregation == BarAggregation.VOLUME_RUNS
            or aggregation == BarAggregation.VALUE_RUNS
        ):
            return True
        else:
            return False

    @staticmethod
    cdef BarSpecification from_str_c(str value):
        Condition.valid_string(value, 'value')

        cdef list pieces = value.rsplit('-', maxsplit=2)

        if len(pieces) != 3:
            raise ValueError(
                f"The BarSpecification string value was malformed, was {value}",
            )

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
            If `value` is not a valid string.

        """
        return BarSpecification.from_str_c(value)

    @staticmethod
    def check_time_aggregated(BarAggregation aggregation):
        """
        Check the given aggregation is a type of time aggregation.

        Parameters
        ----------
        aggregation : BarAggregation
            The aggregation type to check.

        Returns
        -------
        bool
            True if time aggregated, else False.

        """
        return BarSpecification.check_time_aggregated_c(aggregation)

    @staticmethod
    def check_threshold_aggregated(BarAggregation aggregation):
        """
        Check the given aggregation is a type of threshold aggregation.

        Parameters
        ----------
        aggregation : BarAggregation
            The aggregation type to check.

        Returns
        -------
        bool
            True if threshold aggregated, else False.

        """
        return BarSpecification.check_threshold_aggregated_c(aggregation)

    @staticmethod
    def check_information_aggregated(BarAggregation aggregation):
        """
        Check the given aggregation is a type of information aggregation.

        Parameters
        ----------
        aggregation : BarAggregation
            The aggregation type to check.

        Returns
        -------
        bool
            True if information aggregated, else False.

        """
        return BarSpecification.check_information_aggregated_c(aggregation)

    cpdef bint is_time_aggregated(self) except *:
        """
        Return a value indicating whether the aggregation method is time-driven.

        - ``SECOND``
        - ``MINUTE``
        - ``HOUR``
        - ``DAY``
        - ``WEEK``
        - ``MONTH``

        Returns
        -------
        bool

        """
        return BarSpecification.check_time_aggregated_c(self.aggregation)

    cpdef bint is_threshold_aggregated(self) except *:
        """
        Return a value indicating whether the bar aggregation method is
        threshold-driven.

        - ``TICK``
        - ``TICK_IMBALANCE``
        - ``VOLUME``
        - ``VOLUME_IMBALANCE``
        - ``VALUE``
        - ``VALUE_IMBALANCE``

        Returns
        -------
        bool

        """
        return BarSpecification.check_threshold_aggregated_c(self.aggregation)

    cpdef bint is_information_aggregated(self) except *:
        """
        Return a value indicating whether the aggregation method is
        information-driven.

        - ``TICK_RUNS``
        - ``VOLUME_RUNS``
        - ``VALUE_RUNS``

        Returns
        -------
        bool

        """
        return BarSpecification.check_information_aggregated_c(self.aggregation)


cdef class BarType:
    """
    Represents a bar type including the instrument ID, bar specification and
    aggregation source.

    Parameters
    ----------
    instrument_id : InstrumentId
        The bar types instrument ID.
    bar_spec : BarSpecification
        The bar types specification.
    aggregation_source : AggregationSource, default EXTERNAL
        The bar type aggregation source. If ``INTERNAL`` the `DataEngine`
        will subscribe to the necessary ticks and aggregate bars accordingly.
        Else if ``EXTERNAL`` then bars will be subscribed to directly from
        the data publisher.

    Notes
    -----
    It is expected that all bar aggregation methods other than time will be
    internally aggregated.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BarSpecification bar_spec not None,
        AggregationSource aggregation_source=AggregationSource.EXTERNAL,
    ):
        self.instrument_id = instrument_id
        self.spec = bar_spec
        self.aggregation_source = aggregation_source

    def __eq__(self, BarType other) -> bool:
        return (
            self.instrument_id == other.instrument_id
            and self.spec == other.spec
            and self.aggregation_source == other.aggregation_source
        )

    def __lt__(self, BarType other) -> bool:
        return str(self) < str(other)

    def __le__(self, BarType other) -> bool:
        return str(self) <= str(other)

    def __gt__(self, BarType other) -> bool:
        return str(self) > str(other)

    def __ge__(self, BarType other) -> bool:
        return str(self) >= str(other)

    def __hash__(self) -> int:
        return hash((self.instrument_id, self.spec))

    def __str__(self) -> str:
        return f"{self.instrument_id}-{self.spec}-{AggregationSourceParser.to_str(self.aggregation_source)}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef BarType from_str_c(str value):
        Condition.valid_string(value, 'value')

        cdef list pieces = value.rsplit('-', maxsplit=4)

        if len(pieces) != 5:
            raise ValueError(f"The BarType string value was malformed, was {value}")

        cdef InstrumentId instrument_id = InstrumentId.from_str_c(pieces[0])
        cdef BarSpecification bar_spec = BarSpecification(
            int(pieces[1]),
            BarAggregationParser.from_str(pieces[2]),
            PriceTypeParser.from_str(pieces[3]),
        )
        cdef AggregationSource aggregation_source = AggregationSourceParser.from_str(pieces[4])

        return BarType(
            instrument_id=instrument_id,
            bar_spec=bar_spec,
            aggregation_source=aggregation_source,
        )

    @staticmethod
    def from_str(str value) -> BarType:
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
            If `value` is not a valid string.

        """
        return BarType.from_str_c(value)

    cpdef bint is_externally_aggregated(self) except *:
        """
        Return a value indicating whether the bar aggregation source is ``EXTERNAL``.

        Returns
        -------
        bool

        """
        return self.aggregation_source == AggregationSource.EXTERNAL

    cpdef bint is_internally_aggregated(self) except *:
        """
        Return a value indicating whether the bar aggregation source is ``INTERNAL``.

        Returns
        -------
        bool

        """
        return self.aggregation_source == AggregationSource.INTERNAL


cdef class Bar(Data):
    """
    Represents an aggregated bar.

    Parameters
    ----------
    bar_type : BarType
        The bar type for this bar.
    open : Price
        The bars open price.
    high : Price
        The bars high price.
    low : Price
        The bars low price.
    close : Price
        The bars close price.
    volume : Quantity
        The bars volume.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    check : bool
        If bar parameters should be checked valid.

    Raises
    ------
    ValueError
        If `check` True and the `high` is not >= `low`.
    ValueError
        If `check` True and the `high` is not >= `close`.
    ValueError
        If `check` True and the `low` is not <= `close`.
    """

    def __init__(
        self,
        BarType bar_type not None,
        Price open not None,
        Price high not None,
        Price low not None,
        Price close not None,
        Quantity volume not None,
        uint64_t ts_event,
        uint64_t ts_init,
        bint check=False,
    ):
        if check:
            Condition.true(high >= low, 'high was < low')
            Condition.true(high >= close, 'high was < close')
            Condition.true(low <= close, 'low was > close')
        super().__init__(ts_event, ts_init)

        self.type = bar_type
        self.open = open
        self.high = high
        self.low = low
        self.close = close
        self.volume = volume
        self.checked = check

    def __eq__(self, Bar other) -> bool:
        return Bar.to_dict_c(self) == Bar.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(Bar.to_dict_c(self)))

    def __str__(self) -> str:
        return f"{self.type},{self.open},{self.high},{self.low},{self.close},{self.volume},{self.ts_event}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef Bar from_dict_c(dict values):
        Condition.not_none(values, "values")
        return Bar(
            bar_type=BarType.from_str_c(values["bar_type"]),
            open=Price.from_str_c(values["open"]),
            high=Price.from_str_c(values["high"]),
            low=Price.from_str_c(values["low"]),
            close=Price.from_str_c(values["close"]),
            volume=Quantity.from_str_c(values["volume"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(Bar obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "bar_type": str(obj.type),
            "open": str(obj.open),
            "high": str(obj.high),
            "low": str(obj.low),
            "close": str(obj.close),
            "volume": str(obj.volume),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> Bar:
        """
        Return a bar parsed from the given values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        Bar

        """
        return Bar.from_dict_c(values)

    @staticmethod
    def to_dict(Bar obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return Bar.to_dict_c(obj)
