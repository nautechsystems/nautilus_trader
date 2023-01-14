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

from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.model cimport BarSpecification_t
from nautilus_trader.core.rust.model cimport BarType_t
from nautilus_trader.core.rust.model cimport bar_eq
from nautilus_trader.core.rust.model cimport bar_free
from nautilus_trader.core.rust.model cimport bar_hash
from nautilus_trader.core.rust.model cimport bar_new
from nautilus_trader.core.rust.model cimport bar_new_from_raw
from nautilus_trader.core.rust.model cimport bar_specification_eq
from nautilus_trader.core.rust.model cimport bar_specification_free
from nautilus_trader.core.rust.model cimport bar_specification_ge
from nautilus_trader.core.rust.model cimport bar_specification_gt
from nautilus_trader.core.rust.model cimport bar_specification_hash
from nautilus_trader.core.rust.model cimport bar_specification_le
from nautilus_trader.core.rust.model cimport bar_specification_lt
from nautilus_trader.core.rust.model cimport bar_specification_new
from nautilus_trader.core.rust.model cimport bar_specification_to_cstr
from nautilus_trader.core.rust.model cimport bar_to_cstr
from nautilus_trader.core.rust.model cimport bar_type_copy
from nautilus_trader.core.rust.model cimport bar_type_eq
from nautilus_trader.core.rust.model cimport bar_type_free
from nautilus_trader.core.rust.model cimport bar_type_ge
from nautilus_trader.core.rust.model cimport bar_type_gt
from nautilus_trader.core.rust.model cimport bar_type_hash
from nautilus_trader.core.rust.model cimport bar_type_le
from nautilus_trader.core.rust.model cimport bar_type_lt
from nautilus_trader.core.rust.model cimport bar_type_new
from nautilus_trader.core.rust.model cimport bar_type_to_cstr
from nautilus_trader.core.rust.model cimport instrument_id_clone
from nautilus_trader.core.rust.model cimport instrument_id_new_from_cstr
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.model.data.bar_aggregation cimport BarAggregation
from nautilus_trader.model.enums_c cimport AggregationSource
from nautilus_trader.model.enums_c cimport PriceType
from nautilus_trader.model.enums_c cimport aggregation_source_from_str
from nautilus_trader.model.enums_c cimport bar_aggregation_from_str
from nautilus_trader.model.enums_c cimport bar_aggregation_to_str
from nautilus_trader.model.enums_c cimport price_type_from_str
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

        self._mem = bar_specification_new(
            step,
            aggregation,
            price_type
        )

    def __getstate__(self):
        return (
            self._mem.step,
            self._mem.aggregation,
            self._mem.price_type,
        )

    def __setstate__(self, state):
        self._mem = bar_specification_new(
            state[0],
            state[1],
            state[2]
        )

    def __del__(self) -> None:
        # Never allocation heap memory
        bar_specification_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    cdef str to_str(self):
        return cstr_to_pystr(bar_specification_to_cstr(&self._mem))

    def __eq__(self, BarSpecification other) -> bool:
        return bar_specification_eq(&self._mem, &other._mem)

    def __lt__(self, BarSpecification other) -> bool:
        return bar_specification_lt(&self._mem, &other._mem)

    def __le__(self, BarSpecification other) -> bool:
        return bar_specification_le(&self._mem, &other._mem)

    def __gt__(self, BarSpecification other) -> bool:
        return bar_specification_gt(&self._mem, &other._mem)

    def __ge__(self, BarSpecification other) -> bool:
        return bar_specification_ge(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return bar_specification_hash(&self._mem)

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cdef str aggregation_string_c(self):
        return bar_aggregation_to_str(self.aggregation)

    @staticmethod
    cdef BarSpecification from_mem_c(BarSpecification_t mem):
        cdef BarSpecification spec = BarSpecification.__new__(BarSpecification)
        spec._mem = mem
        return spec

    @staticmethod
    cdef BarSpecification from_str_c(str value):
        Condition.valid_string(value, 'value')

        cdef list pieces = value.rsplit('-', maxsplit=2)

        if len(pieces) != 3:
            raise ValueError(
                f"The `BarSpecification` string value was malformed, was {value}",
            )

        return BarSpecification(
            int(pieces[0]),
            bar_aggregation_from_str(pieces[1]),
            price_type_from_str(pieces[2]),
        )

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

    @property
    def step(self) -> int:
        """
        Return the step size for the specification.

        Returns
        -------
        int

        """
        return self._mem.step

    @property
    def aggregation(self) -> BarAggregation:
        """
        Return the aggregation for the specification.

        Returns
        -------
        BarAggregation

        """
        return self._mem.aggregation

    @property
    def price_type(self) -> PriceType:
        """
        Return the price type for the specification.

        Returns
        -------
        PriceType

        """
        return self._mem.price_type

    @property
    def timedelta(self) -> timedelta:
        """
        Return the timedelta for the specification.

        Returns
        -------
        timedelta

        Raises
        ------
        ValueError
            If `aggregation` is not a time aggregation, or is``MONTH`` (which is ambiguous).

        """
        if self.aggregation == BarAggregation.MILLISECOND:
            return timedelta(milliseconds=self.step)
        elif self.aggregation == BarAggregation.SECOND:
            return timedelta(seconds=self.step)
        elif self.aggregation == BarAggregation.MINUTE:
            return timedelta(minutes=self.step)
        elif self.aggregation == BarAggregation.HOUR:
            return timedelta(hours=self.step)
        elif self.aggregation == BarAggregation.DAY:
            return timedelta(days=self.step)
        elif self.aggregation == BarAggregation.WEEK:
            return timedelta(days=self.step * 7)
        else:
            raise ValueError(
                f"timedelta not supported for aggregation "
                f"{bar_aggregation_to_str(self.aggregation)}",
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
    def from_timedelta(timedelta duration, PriceType price_type) -> BarSpecification:
        """
        Return a bar specification parsed from the given timedelta and price_type.

        Parameters
        ----------
        duration : timedelta
            The bar specification timedelta to parse.
        price_type : PriceType
            The bar specification price_type.

        Examples
        --------
        BarSpecification.from_timedelta(datetime.timedelta(minutes=5), PriceType.LAST).

        Returns
        -------
        BarSpecification

        Raises
        ------
        ValueError
            If `duration` is not rounded step of aggregation.

        """
        if duration.days >= 7:
            bar_spec = BarSpecification(duration.days / 7, BarAggregation.WEEK, price_type)
        elif duration.days >= 1:
            bar_spec = BarSpecification(duration.days, BarAggregation.DAY, price_type)
        elif duration.total_seconds() >= 3600:
            bar_spec = BarSpecification(duration.total_seconds() / 3600, BarAggregation.HOUR, price_type)
        elif duration.total_seconds() >= 60:
            bar_spec = BarSpecification(duration.total_seconds() / 60, BarAggregation.MINUTE, price_type)
        elif duration.total_seconds() >= 1:
            bar_spec = BarSpecification(duration.total_seconds(), BarAggregation.SECOND, price_type)
        else:
            bar_spec = BarSpecification(duration.total_seconds() * 1000, BarAggregation.MILLISECOND, price_type)

        if bar_spec.timedelta.total_seconds() == duration.total_seconds():
            return bar_spec
        else:
            raise ValueError(
                f"Duration {repr(duration)} is ambiguous.",
            )

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
        self._mem = bar_type_new(
            instrument_id_clone(&instrument_id._mem),
            bar_spec._mem,
            aggregation_source
        )

    def __getstate__(self):
        return (
            self.instrument_id.value,
            self._mem.spec.step,
            self._mem.spec.aggregation,
            self._mem.spec.price_type,
            self._mem.aggregation_source
        )

    def __setstate__(self, state):
        self._mem = bar_type_new(
            instrument_id_new_from_cstr(
                pystr_to_cstr(state[0]),
            ),
            bar_specification_new(
                state[1],
                state[2],
                state[3]
            ),
            state[4],
        )

    def __del__(self) -> None:
        if self._mem.instrument_id.symbol.value != NULL:
            bar_type_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    cdef str to_str(self):
        return cstr_to_pystr(bar_type_to_cstr(&self._mem))

    def __eq__(self, BarType other) -> bool:
        return bar_type_eq(&self._mem, &other._mem)

    def __lt__(self, BarType other) -> bool:
        return bar_type_lt(&self._mem, &other._mem)

    def __le__(self, BarType other) -> bool:
        return bar_type_le(&self._mem, &other._mem)

    def __gt__(self, BarType other) -> bool:
        return bar_type_gt(&self._mem, &other._mem)

    def __ge__(self, BarType other) -> bool:
        return bar_type_ge(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return bar_type_hash(&self._mem)

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef BarType from_mem_c(BarType_t mem):
        cdef BarType bar_type = BarType.__new__(BarType)
        bar_type._mem = bar_type_copy(&mem)
        return bar_type

    @staticmethod
    cdef BarType from_str_c(str value):
        Condition.valid_string(value, 'value')

        cdef list pieces = value.rsplit('-', maxsplit=4)

        if len(pieces) != 5:
            raise ValueError(f"The `BarType` string value was malformed, was {value}")

        cdef InstrumentId instrument_id = InstrumentId.from_str_c(pieces[0])
        cdef BarSpecification bar_spec = BarSpecification(
            int(pieces[1]),
            bar_aggregation_from_str(pieces[2]),
            price_type_from_str(pieces[3]),
        )
        cdef AggregationSource aggregation_source = aggregation_source_from_str(pieces[4])

        return BarType(
            instrument_id=instrument_id,
            bar_spec=bar_spec,
            aggregation_source=aggregation_source,
        )

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the instrument ID for the bar type.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(self._mem.instrument_id)

    @property
    def spec(self) -> BarSpecification:
        """
        Return the specification for the bar type.

        Returns
        -------
        BarSpecification

        """
        return BarSpecification.from_mem_c(self._mem.spec)

    @property
    def aggregation_source(self) -> AggregationSource:
        """
        Return the aggregation source for the bar type.

        Returns
        -------
        AggregationSource

        """
        return self._mem.aggregation_source

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
    is_revision : bool, default False
        If this bar is a revision of a previous bar with the same `ts_event`.

    Raises
    ------
    ValueError
        If `high` is not >= `low`.
    ValueError
        If `high` is not >= `close`.
    ValueError
        If `low` is not <= `close`.
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
        bint is_revision = False,
    ):
        Condition.true(high._mem.raw >= open._mem.raw, "high was < open")
        Condition.true(high._mem.raw >= low._mem.raw, "high was < low")
        Condition.true(high._mem.raw >= close._mem.raw, "high was < close")
        Condition.true(low._mem.raw <= close._mem.raw, "low was > close")
        Condition.true(low._mem.raw <= open._mem.raw, "low was > open")
        super().__init__(ts_event, ts_init)

        self._mem = bar_new(
            bar_type_copy(&bar_type._mem),
            open._mem,
            high._mem,
            low._mem,
            close._mem,
            volume._mem,
            ts_event,
            ts_init,
        )
        self.is_revision = is_revision

    def __getstate__(self):
        return (
            self.bar_type.instrument_id.value,
            self._mem.bar_type.spec.step,
            self._mem.bar_type.spec.aggregation,
            self._mem.bar_type.spec.price_type,
            self._mem.bar_type.aggregation_source,
            self._mem.open.raw,
            self._mem.high.raw,
            self._mem.low.raw,
            self._mem.close.raw,
            self._mem.close.precision,
            self._mem.volume.raw,
            self._mem.volume.precision,
            self.ts_event,
            self.ts_init,
        )

    def __setstate__(self, state):
        self._mem = bar_new_from_raw(
            bar_type_new(
                instrument_id_new_from_cstr(
                    pystr_to_cstr(state[0]),
                ),
                bar_specification_new(
                    state[1],
                    state[2],
                    state[3],
                ),
                state[4],
            ),
            state[5],
            state[6],
            state[7],
            state[8],
            state[9],
            state[10],
            state[11],
            state[12],
            state[13],
        )
        self.ts_event = state[12]
        self.ts_init = state[13]

    def __del__(self) -> None:
        if self._mem.bar_type.instrument_id.symbol.value != NULL:
            bar_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __eq__(self, Bar other) -> bool:
        return bar_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return bar_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(bar_to_cstr(&self._mem))

    def __str__(self) -> str:
        return self.to_str()

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
            "bar_type": str(obj.bar_type),
            "open": str(obj.open),
            "high": str(obj.high),
            "low": str(obj.low),
            "close": str(obj.close),
            "volume": str(obj.volume),
            "ts_event": obj._mem.ts_event,
            "ts_init": obj._mem.ts_init,
        }

    @property
    def bar_type(self) -> BarType:
        """
        Return the bar type of bar.

        Returns
        -------
        BarType

        """
        return BarType.from_mem_c(self._mem.bar_type)

    @property
    def open(self) -> Price:
        """
        Return the open price of the bar.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.open.raw, self._mem.open.precision)

    @property
    def high(self) -> Price:
        """
        Return the high price of the bar.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.high.raw, self._mem.high.precision)

    @property
    def low(self) -> Price:
        """
        Return the low price of the bar.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.low.raw, self._mem.low.precision)

    @property
    def close(self) -> Price:
        """
        Return the close price of the bar.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.close.raw, self._mem.close.precision)

    @property
    def volume(self) -> Quantity:
        """
        Return the volume of the bar.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.volume.raw, self._mem.volume.precision)

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

    cpdef bint is_single_price(self):
        """
        If the OHLC are all equal to a single price.

        Returns
        -------
        bool

        """
        return self._mem.open.raw == self._mem.high.raw == self._mem.low.raw == self._mem.close.raw
