# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pickle
import warnings

import numpy as np

from nautilus_trader.core import nautilus_pyo3

from cpython.datetime cimport timedelta
from cpython.mem cimport PyMem_Free
from cpython.mem cimport PyMem_Malloc
from cpython.pycapsule cimport PyCapsule_Destructor
from cpython.pycapsule cimport PyCapsule_GetPointer
from cpython.pycapsule cimport PyCapsule_New
from libc.stdint cimport uint8_t
from libc.stdint cimport uint32_t
from libc.stdint cimport uint64_t
from libc.stdint cimport uintptr_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport DEPTH10_LEN
from nautilus_trader.core.rust.model cimport AggregationSource
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport Bar_t
from nautilus_trader.core.rust.model cimport BarSpecification_t
from nautilus_trader.core.rust.model cimport BarType_t
from nautilus_trader.core.rust.model cimport BookAction
from nautilus_trader.core.rust.model cimport BookOrder_t
from nautilus_trader.core.rust.model cimport Data_t
from nautilus_trader.core.rust.model cimport Data_t_Tag
from nautilus_trader.core.rust.model cimport InstrumentCloseType
from nautilus_trader.core.rust.model cimport MarketStatusAction
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.rust.model cimport Quantity_t
from nautilus_trader.core.rust.model cimport QuantityRaw
from nautilus_trader.core.rust.model cimport RecordFlag
from nautilus_trader.core.rust.model cimport bar_eq
from nautilus_trader.core.rust.model cimport bar_hash
from nautilus_trader.core.rust.model cimport bar_new
from nautilus_trader.core.rust.model cimport bar_specification_eq
from nautilus_trader.core.rust.model cimport bar_specification_ge
from nautilus_trader.core.rust.model cimport bar_specification_gt
from nautilus_trader.core.rust.model cimport bar_specification_hash
from nautilus_trader.core.rust.model cimport bar_specification_le
from nautilus_trader.core.rust.model cimport bar_specification_lt
from nautilus_trader.core.rust.model cimport bar_specification_new
from nautilus_trader.core.rust.model cimport bar_specification_to_cstr
from nautilus_trader.core.rust.model cimport bar_to_cstr
from nautilus_trader.core.rust.model cimport bar_type_aggregation_source
from nautilus_trader.core.rust.model cimport bar_type_check_parsing
from nautilus_trader.core.rust.model cimport bar_type_composite
from nautilus_trader.core.rust.model cimport bar_type_eq
from nautilus_trader.core.rust.model cimport bar_type_from_cstr
from nautilus_trader.core.rust.model cimport bar_type_ge
from nautilus_trader.core.rust.model cimport bar_type_gt
from nautilus_trader.core.rust.model cimport bar_type_hash
from nautilus_trader.core.rust.model cimport bar_type_instrument_id
from nautilus_trader.core.rust.model cimport bar_type_is_composite
from nautilus_trader.core.rust.model cimport bar_type_is_standard
from nautilus_trader.core.rust.model cimport bar_type_le
from nautilus_trader.core.rust.model cimport bar_type_lt
from nautilus_trader.core.rust.model cimport bar_type_new
from nautilus_trader.core.rust.model cimport bar_type_new_composite
from nautilus_trader.core.rust.model cimport bar_type_spec
from nautilus_trader.core.rust.model cimport bar_type_standard
from nautilus_trader.core.rust.model cimport bar_type_to_cstr
from nautilus_trader.core.rust.model cimport book_order_debug_to_cstr
from nautilus_trader.core.rust.model cimport book_order_eq
from nautilus_trader.core.rust.model cimport book_order_exposure
from nautilus_trader.core.rust.model cimport book_order_hash
from nautilus_trader.core.rust.model cimport book_order_new
from nautilus_trader.core.rust.model cimport book_order_signed_size
from nautilus_trader.core.rust.model cimport instrument_id_from_cstr
from nautilus_trader.core.rust.model cimport orderbook_delta_eq
from nautilus_trader.core.rust.model cimport orderbook_delta_hash
from nautilus_trader.core.rust.model cimport orderbook_delta_new
from nautilus_trader.core.rust.model cimport orderbook_deltas_clone
from nautilus_trader.core.rust.model cimport orderbook_deltas_drop
from nautilus_trader.core.rust.model cimport orderbook_deltas_flags
from nautilus_trader.core.rust.model cimport orderbook_deltas_instrument_id
from nautilus_trader.core.rust.model cimport orderbook_deltas_is_snapshot
from nautilus_trader.core.rust.model cimport orderbook_deltas_new
from nautilus_trader.core.rust.model cimport orderbook_deltas_sequence
from nautilus_trader.core.rust.model cimport orderbook_deltas_ts_event
from nautilus_trader.core.rust.model cimport orderbook_deltas_ts_init
from nautilus_trader.core.rust.model cimport orderbook_deltas_vec_deltas
from nautilus_trader.core.rust.model cimport orderbook_deltas_vec_drop
from nautilus_trader.core.rust.model cimport orderbook_depth10_ask_counts_array
from nautilus_trader.core.rust.model cimport orderbook_depth10_asks_array
from nautilus_trader.core.rust.model cimport orderbook_depth10_bid_counts_array
from nautilus_trader.core.rust.model cimport orderbook_depth10_bids_array
from nautilus_trader.core.rust.model cimport orderbook_depth10_clone
from nautilus_trader.core.rust.model cimport orderbook_depth10_eq
from nautilus_trader.core.rust.model cimport orderbook_depth10_hash
from nautilus_trader.core.rust.model cimport orderbook_depth10_new
from nautilus_trader.core.rust.model cimport quote_tick_eq
from nautilus_trader.core.rust.model cimport quote_tick_hash
from nautilus_trader.core.rust.model cimport quote_tick_new
from nautilus_trader.core.rust.model cimport quote_tick_to_cstr
from nautilus_trader.core.rust.model cimport symbol_new
from nautilus_trader.core.rust.model cimport trade_id_new
from nautilus_trader.core.rust.model cimport trade_tick_eq
from nautilus_trader.core.rust.model cimport trade_tick_hash
from nautilus_trader.core.rust.model cimport trade_tick_new
from nautilus_trader.core.rust.model cimport trade_tick_to_cstr
from nautilus_trader.core.rust.model cimport venue_new
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.string cimport ustr_to_pystr
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.functions cimport aggregation_source_from_str
from nautilus_trader.model.functions cimport aggressor_side_from_str
from nautilus_trader.model.functions cimport aggressor_side_to_str
from nautilus_trader.model.functions cimport bar_aggregation_from_str
from nautilus_trader.model.functions cimport bar_aggregation_to_str
from nautilus_trader.model.functions cimport book_action_from_str
from nautilus_trader.model.functions cimport book_action_to_str
from nautilus_trader.model.functions cimport instrument_close_type_from_str
from nautilus_trader.model.functions cimport instrument_close_type_to_str
from nautilus_trader.model.functions cimport market_status_action_from_str
from nautilus_trader.model.functions cimport market_status_action_to_str
from nautilus_trader.model.functions cimport order_side_from_str
from nautilus_trader.model.functions cimport order_side_to_str
from nautilus_trader.model.functions cimport price_type_from_str
from nautilus_trader.model.functions cimport price_type_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.objects cimport price_new
from nautilus_trader.model.objects cimport quantity_new


cdef inline BookOrder order_from_mem_c(BookOrder_t mem):
    cdef BookOrder order = BookOrder.__new__(BookOrder)
    order._mem = mem
    return order


cdef inline OrderBookDelta delta_from_mem_c(OrderBookDelta_t mem):
    cdef OrderBookDelta delta = OrderBookDelta.__new__(OrderBookDelta)
    delta._mem = mem
    return delta


cdef inline OrderBookDeltas deltas_from_mem_c(OrderBookDeltas_API mem):
    cdef OrderBookDeltas deltas = OrderBookDeltas.__new__(OrderBookDeltas)
    deltas._mem = orderbook_deltas_clone(&mem)
    return deltas


cdef inline OrderBookDepth10 depth10_from_mem_c(OrderBookDepth10_t mem):
    cdef OrderBookDepth10 depth10 = OrderBookDepth10.__new__(OrderBookDepth10)
    depth10._mem = mem
    return depth10


cdef inline QuoteTick quote_from_mem_c(QuoteTick_t mem):
    cdef QuoteTick quote = QuoteTick.__new__(QuoteTick)
    quote._mem = mem
    return quote


cdef inline TradeTick trade_from_mem_c(TradeTick_t mem):
    cdef TradeTick trade = TradeTick.__new__(TradeTick)
    trade._mem = mem
    return trade


cdef inline Bar bar_from_mem_c(Bar_t mem):
    cdef Bar bar = Bar.__new__(Bar)
    bar._mem = mem
    return bar


# SAFETY: Do NOT deallocate the capsule here
cpdef list capsule_to_list(capsule):
    cdef CVec* data = <CVec*>PyCapsule_GetPointer(capsule, NULL)
    cdef Data_t* ptr = <Data_t*>data.ptr
    cdef list objects = []

    cdef uint64_t i
    for i in range(0, data.len):
        if ptr[i].tag == Data_t_Tag.DELTA:
            objects.append(delta_from_mem_c(ptr[i].delta))
        elif ptr[i].tag == Data_t_Tag.DELTAS:
            objects.append(deltas_from_mem_c(ptr[i].deltas))
        elif ptr[i].tag == Data_t_Tag.DEPTH10:
            objects.append(depth10_from_mem_c(orderbook_depth10_clone(ptr[i].depth10)))
        elif ptr[i].tag == Data_t_Tag.QUOTE:
            objects.append(quote_from_mem_c(ptr[i].quote))
        elif ptr[i].tag == Data_t_Tag.TRADE:
            objects.append(trade_from_mem_c(ptr[i].trade))
        elif ptr[i].tag == Data_t_Tag.BAR:
            objects.append(bar_from_mem_c(ptr[i].bar))

    return objects


# SAFETY: Do NOT deallocate the capsule here
cpdef Data capsule_to_data(capsule):
    cdef Data_t* ptr = <Data_t*>PyCapsule_GetPointer(capsule, NULL)

    if ptr.tag == Data_t_Tag.DELTA:
        return delta_from_mem_c(ptr.delta)
    elif ptr.tag == Data_t_Tag.DELTAS:
        return deltas_from_mem_c(ptr.deltas)
    elif ptr.tag == Data_t_Tag.DEPTH10:
        return depth10_from_mem_c(orderbook_depth10_clone(ptr.depth10))
    elif ptr.tag == Data_t_Tag.QUOTE:
        return quote_from_mem_c(ptr.quote)
    elif ptr.tag == Data_t_Tag.TRADE:
        return trade_from_mem_c(ptr.trade)
    elif ptr.tag == Data_t_Tag.BAR:
        return bar_from_mem_c(ptr.bar)
    else:
        raise RuntimeError("Invalid data element to convert from `PyCapsule`")


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
    ) -> None:
        Condition.positive_int(step, "step")

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

    cpdef bint is_time_aggregated(self):
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

    cpdef bint is_threshold_aggregated(self):
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

    cpdef bint is_information_aggregated(self):
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
        The bar type's instrument ID.
    bar_spec : BarSpecification
        The bar type's specification.
    aggregation_source : AggregationSource, default EXTERNAL
        The bar type aggregation source. If ``INTERNAL`` the `DataEngine`
        will subscribe to the necessary ticks and aggregate bars accordingly.
        Else if ``EXTERNAL`` then bars will be subscribed to directly from
        the venue / data provider.

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
    ) -> None:
        self._mem = bar_type_new(
            instrument_id._mem,
            bar_spec._mem,
            aggregation_source
        )

    def __getstate__(self):
        if self.is_standard():
            spec = self.spec

            return (
                self.instrument_id.value,
                spec.step,
                spec.aggregation,
                spec.price_type,
                self.aggregation_source,
            )
        else:
            composite = self.composite()
            spec = self.spec
            spec_composite = composite.spec

            return (
                self.instrument_id.value,
                spec.step,
                spec.aggregation,
                spec.price_type,
                self.aggregation_source,

                spec_composite.step,
                spec_composite.aggregation,
                composite.aggregation_source
            )

    def __setstate__(self, state):
        if len(state) == 5:
            instrument_id = InstrumentId.from_str_c(state[0])

            self._mem = bar_type_new(
                instrument_id._mem,
                bar_specification_new(
                    state[1],
                    state[2],
                    state[3]
                ),
                state[4],
            )
        else:
            instrument_id = InstrumentId.from_str_c(state[0])
            composite_instrument_id = InstrumentId.from_str_c(state[5])

            self._mem = bar_type_new_composite(
                instrument_id._mem,
                bar_specification_new(
                    state[1],
                    state[2],
                    state[3]
                ),
                state[4],

                state[5],
                state[6],
                state[7],
            )

    cdef str to_str(self):
        return cstr_to_pystr(bar_type_to_cstr(&self._mem))

    def __eq__(self, BarType other) -> bool:
        return self.to_str() == other.to_str()

    def __lt__(self, BarType other) -> bool:
        return self.to_str() < other.to_str()

    def __le__(self, BarType other) -> bool:
        return self.to_str() <= other.to_str()

    def __gt__(self, BarType other) -> bool:
        return self.to_str() > other.to_str()

    def __ge__(self, BarType other) -> bool:
        return self.to_str() >= other.to_str()

    def __hash__(self) -> int:
        return hash(self.to_str())

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the instrument ID for the bar type.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(bar_type_instrument_id(&self._mem))

    @property
    def spec(self) -> BarSpecification:
        """
        Return the specification for the bar type.

        Returns
        -------
        BarSpecification

        """
        return BarSpecification.from_mem_c(bar_type_spec(&self._mem))

    @property
    def aggregation_source(self) -> AggregationSource:
        """
        Return the aggregation source for the bar type.

        Returns
        -------
        AggregationSource

        """
        return bar_type_aggregation_source(&self._mem)

    @staticmethod
    cdef BarType from_mem_c(BarType_t mem):
        cdef BarType bar_type = BarType.__new__(BarType)
        bar_type._mem = mem
        return bar_type

    @staticmethod
    cdef BarType from_str_c(str value):
        Condition.valid_string(value, "value")

        cdef str parse_err = cstr_to_pystr(bar_type_check_parsing(pystr_to_cstr(value)))
        if parse_err:
            raise ValueError(parse_err)

        cdef BarType bar_type = BarType.__new__(BarType)
        bar_type._mem = bar_type_from_cstr(pystr_to_cstr(value))
        return bar_type

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

    cpdef bint is_externally_aggregated(self):
        """
        Return a value indicating whether the bar aggregation source is ``EXTERNAL``.

        Returns
        -------
        bool

        """
        return self.aggregation_source == AggregationSource.EXTERNAL

    cpdef bint is_internally_aggregated(self):
        """
        Return a value indicating whether the bar aggregation source is ``INTERNAL``.

        Returns
        -------
        bool

        """
        return self.aggregation_source == AggregationSource.INTERNAL

    @staticmethod
    def new_composite(
        InstrumentId instrument_id,
        BarSpecification bar_spec,
        AggregationSource aggregation_source,

        int composite_step,
        BarAggregation composite_aggregation,
        AggregationSource composite_aggregation_source,
    ) -> BarType:
        return BarType.from_mem_c(
            bar_type_new_composite(
                instrument_id._mem,
                bar_spec._mem,
                aggregation_source,

                composite_step,
                composite_aggregation,
                composite_aggregation_source,
            )
        )

    cpdef bint is_standard(self):
        """
        Return a value indicating whether the bar type corresponds to `BarType::Standard` in Rust.

        Returns
        -------
        bool

        """
        return bar_type_is_standard(&self._mem)

    cpdef bint is_composite(self):
        """
        Return a value indicating whether the bar type corresponds to `BarType::Composite` in Rust.

        Returns
        -------
        bool

        """
        return bar_type_is_composite(&self._mem)

    cpdef BarType standard(self):
        cdef BarType bar_type = BarType.__new__(BarType)
        bar_type._mem = bar_type_standard(&self._mem)
        return bar_type

    cpdef BarType  composite(self):
        cdef BarType bar_type = BarType.__new__(BarType)
        bar_type._mem = bar_type_composite(&self._mem)
        return bar_type


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
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.
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
    ) -> None:
        Condition.is_true(high._mem.raw >= open._mem.raw, "high was < open")
        Condition.is_true(high._mem.raw >= low._mem.raw, "high was < low")
        Condition.is_true(high._mem.raw >= close._mem.raw, "high was < close")
        Condition.is_true(low._mem.raw <= close._mem.raw, "low was > close")
        Condition.is_true(low._mem.raw <= open._mem.raw, "low was > open")

        self._mem = bar_new(
            bar_type._mem,
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
        bar_type = BarType.from_mem_c(self._mem.bar_type)
        bart_type_state = bar_type.__getstate__()

        return bart_type_state + (
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
        cdef InstrumentId instrument_id
        cdef uint8_t price_prec
        cdef uint8_t size_prec

        if len(state) == 14:
            instrument_id = InstrumentId.from_str_c(state[0])
            price_prec = state[9]
            size_prec = state[11]

            self._mem = bar_new(
                bar_type_new(
                    instrument_id._mem,
                    bar_specification_new(
                        state[1],
                        state[2],
                        state[3],
                    ),
                    state[4],
                ),
                price_new(state[5], price_prec),
                price_new(state[6], price_prec),
                price_new(state[7], price_prec),
                price_new(state[8], price_prec),
                quantity_new(state[10], size_prec),
                state[12],
                state[13],
            )
        else:
            instrument_id = InstrumentId.from_str_c(state[0])
            price_prec = state[12]
            size_prec = state[14]

            self._mem = bar_new(
                bar_type_new_composite(
                    instrument_id._mem,
                    bar_specification_new(
                        state[1],
                        state[2],
                        state[3]
                    ),
                    state[4],

                    state[5],
                    state[6],
                    state[7]
                ),
                price_new(state[8], price_prec),
                price_new(state[9], price_prec),
                price_new(state[10], price_prec),
                price_new(state[11], price_prec),
                quantity_new(state[13], size_prec),
                state[15],
                state[16],
            )

    def __eq__(self, Bar other) -> bool:
        return self.to_str() == other.to_str()

    def __hash__(self) -> int:
        return hash(self.to_str())

    cdef str to_str(self):
        return cstr_to_pystr(bar_to_cstr(&self._mem))

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

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

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._mem.ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._mem.ts_init

    @staticmethod
    cdef Bar from_mem_c(Bar_t mem):
        return bar_from_mem_c(mem)

    @staticmethod
    cdef Bar from_raw_c(
        BarType bar_type,
        PriceRaw open,
        PriceRaw high,
        PriceRaw low,
        PriceRaw close,
        uint8_t price_prec,
        QuantityRaw volume,
        uint8_t size_prec,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        cdef Price_t open_price = price_new(open, price_prec)
        cdef Price_t high_price = price_new(high, price_prec)
        cdef Price_t low_price = price_new(low, price_prec)
        cdef Price_t close_price = price_new(close, price_prec)
        cdef Quantity_t volume_qty = quantity_new(volume, size_prec)
        cdef Bar bar = Bar.__new__(Bar)
        bar._mem = bar_new(
            bar_type._mem,
            open_price,
            high_price,
            low_price,
            close_price,
            volume_qty,
            ts_event,
            ts_init,
        )

        return bar

    @staticmethod
    cdef list[Bar] from_raw_arrays_to_list_c(
        BarType bar_type,
        uint8_t price_prec,
        uint8_t size_prec,
        double[:] opens,
        double[:] highs,
        double[:] lows,
        double[:] closes,
        double[:] volumes,
        uint64_t[:] ts_events,
        uint64_t[:] ts_inits,
    ):
        Condition.is_true(
            len(opens) == len(highs) == len(lows) == len(lows) ==
            len(closes) == len(volumes) == len(ts_events) == len(ts_inits),
            "Array lengths must be equal",
        )

        cdef int count = ts_events.shape[0]
        cdef list[Bar] bars = []

        cdef:
            int i
            Price open_price
            Price high_price
            Price low_price
            Price close_price
            Quantity volume_qty
            Bar bar
        for i in range(count):
            open_price = Price(opens[i], price_prec)
            high_price = Price(highs[i], price_prec)
            low_price = Price(lows[i], price_prec)
            close_price = Price(closes[i], price_prec)
            volume_qty = Quantity(volumes[i], size_prec)
            bar = Bar.__new__(Bar)
            bar._mem = bar_new(
                bar_type._mem,
                open_price._mem,
                high_price._mem,
                low_price._mem,
                close_price._mem,
                volume_qty._mem,
                ts_events[i],
                ts_inits[i],
            )
            bars.append(bar)

        return bars

    @staticmethod
    def from_raw_arrays_to_list(
        BarType bar_type,
        uint8_t price_prec,
        uint8_t size_prec,
        double[:] opens,
        double[:] highs,
        double[:] lows,
        double[:] closes,
        double[:] volumes,
        uint64_t[:] ts_events,
        uint64_t[:] ts_inits,
    ) -> list[Bar]:
        return Bar.from_raw_arrays_to_list_c(
            bar_type,
            price_prec,
            size_prec,
            opens,
            highs,
            lows,
            closes,
            volumes,
            ts_events,
            ts_inits,
        )

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

    @staticmethod
    cdef Bar from_pyo3_c(pyo3_bar):
        # SAFETY: Do NOT deallocate the capsule here
        # It is supposed to be deallocated by the creator
        capsule = pyo3_bar.as_pycapsule()
        cdef Data_t* ptr = <Data_t*>PyCapsule_GetPointer(capsule, NULL)
        return bar_from_mem_c(ptr.bar)

    @staticmethod
    def from_raw(
        BarType bar_type,
        PriceRaw open,
        PriceRaw high,
        PriceRaw low,
        PriceRaw close,
        uint8_t price_prec,
        QuantityRaw volume,
        uint8_t size_prec,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> Bar:
        return Bar.from_raw_c(
            bar_type,
            open,
            high,
            low,
            close,
            price_prec,
            volume,
            size_prec,
            ts_event,
            ts_init,
        )

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

    @staticmethod
    def to_pyo3_list(list bars) -> list[nautilus_pyo3.Bar]:
        """
        Return pyo3 Rust bars converted from the given legacy Cython objects.

        Parameters
        ----------
        bars : list[Bar]
            The legacy Cython bars to convert from.

        Returns
        -------
        list[nautilus_pyo3.Bar]

        """
        cdef list output = []

        pyo3_bar_type = None
        cdef uint8_t price_prec = 0
        cdef uint8_t volume_prec = 0

        cdef:
            Bar bar
            BarType bar_type
        for bar in bars:
            if pyo3_bar_type is None:
                bar_type = bar.bar_type
                pyo3_bar_type = nautilus_pyo3.BarType.from_str(bar_type.to_str())
                price_prec = bar._mem.open.precision
                volume_prec = bar._mem.volume.precision

            pyo3_bar = nautilus_pyo3.Bar(
                pyo3_bar_type,
                nautilus_pyo3.Price.from_raw(bar._mem.open.raw, price_prec),
                nautilus_pyo3.Price.from_raw(bar._mem.high.raw, price_prec),
                nautilus_pyo3.Price.from_raw(bar._mem.low.raw, price_prec),
                nautilus_pyo3.Price.from_raw(bar._mem.close.raw, price_prec),
                nautilus_pyo3.Quantity.from_raw(bar._mem.volume.raw, volume_prec),
                bar._mem.ts_event,
                bar._mem.ts_init,
            )
            output.append(pyo3_bar)

        return output

    @staticmethod
    def from_pyo3_list(list pyo3_bars) -> list[Bar]:
        """
        Return legacy Cython bars converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_bars : list[nautilus_pyo3.Bar]
            The pyo3 Rust bars to convert from.

        Returns
        -------
        list[Bar]

        """
        cdef list[Bar] output = []

        for pyo3_bar in pyo3_bars:
            output.append(Bar.from_pyo3_c(pyo3_bar))

        return output

    @staticmethod
    def from_pyo3(pyo3_bar) -> Bar:
        """
        Return a legacy Cython bar converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_bar : nautilus_pyo3.Bar
            The pyo3 Rust bar to convert from.

        Returns
        -------
        Bar

        """
        return Bar.from_pyo3_c(pyo3_bar)

    def to_pyo3(self) -> nautilus_pyo3.Bar:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.Bar

        """
        return nautilus_pyo3.Bar(
            nautilus_pyo3.BarType.from_str(BarType.from_mem_c(self._mem.bar_type).to_str()),
            nautilus_pyo3.Price.from_raw(self._mem.open.raw, self._mem.open.precision),
            nautilus_pyo3.Price.from_raw(self._mem.high.raw, self._mem.high.precision),
            nautilus_pyo3.Price.from_raw(self._mem.low.raw, self._mem.low.precision),
            nautilus_pyo3.Price.from_raw(self._mem.close.raw, self._mem.close.precision),
            nautilus_pyo3.Quantity.from_raw(self._mem.volume.raw, self._mem.volume.precision),
            self._mem.ts_event,
            self._mem.ts_init,
        )

    cpdef bint is_single_price(self):
        """
        If the OHLC are all equal to a single price.

        Returns
        -------
        bool

        """
        return self._mem.open.raw == self._mem.high.raw == self._mem.low.raw == self._mem.close.raw


cdef class DataType:
    """
    Represents a data type including metadata.

    Parameters
    ----------
    type : type
        The `Data` type of the data.
    metadata : dict
        The data types metadata.

    Raises
    ------
    ValueError
        If `type` is not either a subclass of `Data` or meets the `Data` contract.
    TypeError
        If `metadata` contains a key or value which is not hashable.

    Warnings
    --------
    This class may be used as a key in hash maps throughout the system, thus
    the key and value contents of metadata must themselves be hashable.

    """

    def __init__(self, type type not None, dict metadata = None) -> None:  # noqa (shadows built-in type)
        if not issubclass(type, Data):
            if not (hasattr(type, "ts_event") and hasattr(type, "ts_init")):
                raise TypeError("`type` was not a subclass of `Data`")

        self.type = type
        self.metadata = metadata or {}
        self.topic = self.type.__name__ + '.' + '.'.join([
            f'{k}={v if v is not None else "*"}' for k, v in self.metadata.items()
        ]) if self.metadata else self.type.__name__ + "*"

        self._key = frozenset(self.metadata.items())
        self._hash = hash((self.type, self._key))  # Assign hash for improved time complexity

    def __eq__(self, DataType other) -> bool:
        return self.type == other.type and self._key == other._key  # noqa

    def __lt__(self, DataType other) -> bool:
        return str(self) < str(other)

    def __le__(self, DataType other) -> bool:
        return str(self) <= str(other)

    def __gt__(self, DataType other) -> bool:
        return str(self) > str(other)

    def __ge__(self, DataType other) -> bool:
        return str(self) >= str(other)

    def __hash__(self) -> int:
        return self._hash

    def __str__(self) -> str:
        return f"{self.type.__name__}{self.metadata if self.metadata else ''}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}(type={self.type.__name__}, metadata={self.metadata})"


cdef class CustomData(Data):
    """
    Provides a wrapper for custom data which includes data type information.

    Parameters
    ----------
    data_type : DataType
        The data type.
    data : Data
        The data object to wrap.

    """

    def __init__(
        self,
        DataType data_type not None,
        Data data not None,
    ) -> None:
        self.data_type = data_type
        self.data = data

    def __repr__(self) -> str:
        return f"{type(self).__name__}(data_type={self.data_type}, data={self.data})"

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self.data.ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self.data.ts_init


# Represents a 'NULL' order (used for 'CLEAR' actions by OrderBookDelta)
NULL_ORDER = BookOrder(
    side=OrderSide.NO_ORDER_SIDE,
    price=Price(0, 0),
    size=Quantity(0, 0),
    order_id=0,
)


cdef class BookOrder:
    """
    Represents an order in a book.

    Parameters
    ----------
    side : OrderSide {``BUY``, ``SELL``}
        The order side.
    price : Price
        The order price.
    size : Quantity
        The order size.
    order_id : uint64_t
        The order ID.

    """

    def __init__(
        self,
        OrderSide side,
        Price price not None,
        Quantity size not None,
        uint64_t order_id,
    ) -> None:
        self._mem = book_order_new(
            side,
            price._mem,
            size._mem,
            order_id,
        )

    def __getstate__(self):
        return (
            self._mem.side,
            self._mem.price.raw,
            self._mem.price.precision,
            self._mem.size.raw,
            self._mem.size.precision,
            self._mem.order_id,
        )

    def __setstate__(self, state):
        cdef Price_t price = price_new(state[1], state[2])
        cdef Quantity_t size = quantity_new(state[3], state[4])
        self._mem = book_order_new(
            state[0],
            price,
            size,
            state[5],
        )

    def __eq__(self, BookOrder other) -> bool:
        return book_order_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return book_order_hash(&self._mem)

    def __repr__(self) -> str:
        return cstr_to_pystr(book_order_debug_to_cstr(&self._mem))

    @staticmethod
    cdef BookOrder from_raw_c(
        OrderSide side,
        PriceRaw price_raw,
        uint8_t price_prec,
        QuantityRaw size_raw,
        uint8_t size_prec,
        uint64_t order_id,
    ):
        cdef Price_t price = price_new(price_raw, price_prec)
        cdef Quantity_t size = quantity_new(size_raw, size_prec)
        cdef BookOrder order = BookOrder.__new__(BookOrder)
        order._mem = book_order_new(
            side,
            price,
            size,
            order_id,
        )
        return order

    @property
    def price(self) -> Price:
        """
        Return the book orders price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.price.raw, self._mem.price.precision)

    @property
    def size(self) -> Quantity:
        """
        Return the book orders size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.size.raw, self._mem.size.precision)

    @property
    def side(self) -> OrderSide:
        """
        Return the book orders side.

        Returns
        -------
        OrderSide

        """
        return <OrderSide>self._mem.side

    @property
    def order_id(self) -> uint64_t:
        """
        Return the book orders side.

        Returns
        -------
        uint64_t

        """
        return self._mem.order_id

    @staticmethod
    cdef BookOrder from_mem_c(BookOrder_t mem):
        return order_from_mem_c(mem)

    cpdef double exposure(self):
        """
        Return the total exposure for this order (price * size).

        Returns
        -------
        double

        """
        return book_order_exposure(&self._mem)

    cpdef double signed_size(self):
        """
        Return the signed size of the order (negative for ``SELL``).

        Returns
        -------
        double

        """
        return book_order_signed_size(&self._mem)

    @staticmethod
    def from_raw(
        OrderSide side,
        PriceRaw price_raw,
        uint8_t price_prec,
        QuantityRaw size_raw,
        uint8_t size_prec,
        uint64_t order_id,
    ) -> BookOrder:
        """
        Return an book order from the given raw values.

        Parameters
        ----------
        side : OrderSide {``BUY``, ``SELL``}
            The order side.
        price_raw : int
            The order raw price (as a scaled fixed-point integer).
        price_prec : uint8_t
            The order price precision.
        size_raw : int
            The order raw size (as a scaled fixed-point integer).
        size_prec : uint8_t
            The order size precision.
        order_id : uint64_t
            The order ID.

        Returns
        -------
        BookOrder

        """
        return BookOrder.from_raw_c(
            side,
            price_raw,
            price_prec,
            size_raw,
            size_prec,
            order_id,
        )

    @staticmethod
    cdef BookOrder from_dict_c(dict values):
        Condition.not_none(values, "values")
        return BookOrder(
            side=order_side_from_str(values["side"]),
            price=Price.from_str(values["price"]),
            size=Quantity.from_str(values["size"]),
            order_id=values["order_id"],
        )

    @staticmethod
    cdef dict to_dict_c(BookOrder obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "BookOrder",
            "side": order_side_to_str(obj.side),
            "price": str(obj.price),
            "size": str(obj.size),
            "order_id": obj.order_id,
        }

    @staticmethod
    def from_dict(dict values) -> BookOrder:
        """
        Return an order from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        BookOrder

        """
        return BookOrder.from_dict_c(values)

    @staticmethod
    def to_dict(BookOrder obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return BookOrder.to_dict_c(obj)


cdef class OrderBookDelta(Data):
    """
    Represents a single update/difference on an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    action : BookAction {``ADD``, ``UPDATE``, ``DELETE``, ``CLEAR``}
        The order book delta action.
    order : BookOrder or ``None``
        The book order for the delta.
    flags : uint8_t
        The record flags bit field, indicating event end and data information.
        A value of zero indicates no flags.
    sequence : uint64_t
        The unique sequence number for the update.
        If no sequence number provided in the source data then use a value of zero.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `action` is `ADD` or `UPDATE` and `order.size` is not positive (> 0).

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookAction action,
        BookOrder order: BookOrder | None,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> None:
        # Placeholder for now
        cdef BookOrder_t book_order = order._mem if order is not None else book_order_new(
            OrderSide.NO_ORDER_SIDE,
            price_new(0, 0),
            quantity_new(0, 0),
            0,
        )

        if action == BookAction.ADD or action == BookAction.UPDATE:
            Condition.positive_int(book_order.size.raw, "size")

        self._mem = orderbook_delta_new(
            instrument_id._mem,
            action,
            book_order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )

    def __getstate__(self):
        return (
            self.instrument_id.value,
            self._mem.action,
            self._mem.order.side,
            self._mem.order.price.raw,
            self._mem.order.price.precision,
            self._mem.order.size.raw,
            self._mem.order.size.precision,
            self._mem.order.order_id,
            self._mem.flags,
            self._mem.sequence,
            self._mem.ts_event,
            self._mem.ts_init,
        )

    def __setstate__(self, state):
        cdef BookAction action = state[1]
        cdef Price_t price = price_new(state[3], state[4])
        cdef Quantity_t size = quantity_new(state[5], state[6])

        if action == BookAction.ADD or action == BookAction.UPDATE:
            Condition.positive_int(size.raw, "size")

        cdef InstrumentId instrument_id = InstrumentId.from_str_c(state[0])
        cdef BookOrder_t book_order = book_order_new(
            state[2],
            price,
            size,
            state[7],
        )
        self._mem = orderbook_delta_new(
            instrument_id._mem,
            action,
            book_order,
            state[8],
            state[9],
            state[10],
            state[11],
        )

    def __eq__(self, OrderBookDelta other) -> bool:
        return orderbook_delta_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return orderbook_delta_hash(&self._mem)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"action={book_action_to_str(self.action)}, "
            f"order={self.order}, "
            f"flags={self._mem.flags}, "
            f"sequence={self._mem.sequence}, "
            f"ts_event={self._mem.ts_event}, "
            f"ts_init={self._mem.ts_init})"
        )

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the deltas book instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(self._mem.instrument_id)

    @property
    def action(self) -> BookAction:
        """
        Return the deltas book action {``ADD``, ``UPDATE``, ``DELETE``, ``CLEAR``}

        Returns
        -------
        BookAction

        """
        return <BookAction>self._mem.action

    @property
    def is_add(self) -> BookAction:
        """
        If the deltas book action is an ``ADD``.

        Returns
        -------
        bool

        """
        return <BookAction>self._mem.action == BookAction.ADD

    @property
    def is_update(self) -> BookAction:
        """
        If the deltas book action is an ``UPDATE``.

        Returns
        -------
        bool

        """
        return <BookAction>self._mem.action == BookAction.UPDATE

    @property
    def is_delete(self) -> BookAction:
        """
        If the deltas book action is a ``DELETE``.

        Returns
        -------
        bool

        """
        return <BookAction>self._mem.action == BookAction.DELETE

    @property
    def is_clear(self) -> BookAction:
        """
        If the deltas book action is a ``CLEAR``.

        Returns
        -------
        bool

        """
        return <BookAction>self._mem.action == BookAction.CLEAR

    @property
    def order(self) -> BookOrder | None:
        """
        Return the deltas book order for the action.

        Returns
        -------
        BookOrder

        """
        cdef BookOrder_t order = self._mem.order
        if order is None:
            return None
        return order_from_mem_c(order)

    @property
    def flags(self) -> uint8_t:
        """
        Return the flags for the delta.

        Returns
        -------
        uint8_t

        """
        return self._mem.flags

    @property
    def sequence(self) -> uint64_t:
        """
        Return the sequence number for the delta.

        Returns
        -------
        uint64_t

        """
        return self._mem.sequence

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._mem.ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._mem.ts_init

    @staticmethod
    cdef OrderBookDelta from_raw_c(
        InstrumentId instrument_id,
        BookAction action,
        OrderSide side,
        PriceRaw price_raw,
        uint8_t price_prec,
        QuantityRaw size_raw,
        uint8_t size_prec,
        uint64_t order_id,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        cdef Price_t price = price_new(price_raw, price_prec)
        cdef Quantity_t size = quantity_new(size_raw, size_prec)
        cdef BookOrder_t book_order = book_order_new(
            side,
            price,
            size,
            order_id,
        )

        if action == BookAction.ADD or action == BookAction.UPDATE:
            Condition.positive_int(size_raw, "size_raw")

        cdef OrderBookDelta delta = OrderBookDelta.__new__(OrderBookDelta)
        delta._mem = orderbook_delta_new(
            instrument_id._mem,
            action,
            book_order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
        return delta

    @staticmethod
    cdef OrderBookDelta from_mem_c(OrderBookDelta_t mem):
        return delta_from_mem_c(mem)

    @staticmethod
    cdef OrderBookDelta from_pyo3_c(pyo3_delta):
        # SAFETY: Do NOT deallocate the capsule here
        # It is supposed to be deallocated by the creator
        capsule = pyo3_delta.as_pycapsule()
        cdef Data_t* ptr = <Data_t*>PyCapsule_GetPointer(capsule, NULL)
        return delta_from_mem_c(ptr.delta)

    @staticmethod
    cdef OrderBookDelta from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef BookAction action = book_action_from_str(values["action"])
        cdef BookOrder order = BookOrder.from_dict_c(values["order"])
        return OrderBookDelta(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            action=action,
            order=order,
            flags=values["flags"],
            sequence=values["sequence"],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDelta obj):
        Condition.not_none(obj, "obj")
        cdef BookOrder order = obj.order
        return {
            "type": "OrderBookDelta",
            "instrument_id": obj.instrument_id.value,
            "action": book_action_to_str(obj._mem.action),
            "order": {
                "side": order_side_to_str(order._mem.side),
                "price": str(order.price),
                "size": str(order.size),
                "order_id": order._mem.order_id,
            },
            "flags": obj._mem.flags,
            "sequence": obj._mem.sequence,
            "ts_event": obj._mem.ts_event,
            "ts_init": obj._mem.ts_init,
        }

    @staticmethod
    cdef OrderBookDelta clear_c(
        InstrumentId instrument_id,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        return OrderBookDelta(
            instrument_id=instrument_id,
            action=BookAction.CLEAR,
            order=None,
            flags=0,
            sequence=sequence,
            ts_event=ts_event,
            ts_init=ts_init,
        )

    @staticmethod
    cdef list[OrderBookDelta] capsule_to_list_c(object capsule):
        # SAFETY: Do NOT deallocate the capsule here
        # It is supposed to be deallocated by the creator
        cdef CVec* data = <CVec*>PyCapsule_GetPointer(capsule, NULL)
        cdef OrderBookDelta_t* ptr = <OrderBookDelta_t*>data.ptr
        cdef list[OrderBookDelta] deltas = []

        cdef uint64_t i
        for i in range(0, data.len):
            deltas.append(delta_from_mem_c(ptr[i]))

        return deltas

    @staticmethod
    cdef object list_to_capsule_c(list items):
        # Create a C struct buffer
        cdef uint64_t len_ = len(items)
        cdef OrderBookDelta_t *data = <OrderBookDelta_t *>PyMem_Malloc(len_ * sizeof(OrderBookDelta_t))
        cdef uint64_t i
        for i in range(len_):
            data[i] = (<OrderBookDelta>items[i])._mem
        if not data:
            raise MemoryError()

        # Create CVec
        cdef CVec *cvec = <CVec *>PyMem_Malloc(1 * sizeof(CVec))
        cvec.ptr = data
        cvec.len = len_
        cvec.cap = len_

        # Create PyCapsule
        return PyCapsule_New(cvec, NULL, <PyCapsule_Destructor>capsule_destructor)

    @staticmethod
    def list_from_capsule(capsule) -> list[OrderBookDelta]:
        return OrderBookDelta.capsule_to_list_c(capsule)

    @staticmethod
    def capsule_from_list(list items):
        return OrderBookDelta.list_to_capsule_c(items)

    @staticmethod
    def from_raw(
        InstrumentId instrument_id,
        BookAction action,
        OrderSide side,
        PriceRaw price_raw,
        uint8_t price_prec,
        QuantityRaw size_raw,
        uint8_t size_prec,
        uint64_t order_id,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> OrderBookDelta:
        """
        Return an order book delta from the given raw values.

        Parameters
        ----------
        instrument_id : InstrumentId
            The trade instrument ID.
        action : BookAction {``ADD``, ``UPDATE``, ``DELETE``, ``CLEAR``}
            The order book delta action.
        side : OrderSide {``BUY``, ``SELL``}
            The order side.
        price_raw : int
            The order raw price (as a scaled fixed-point integer).
        price_prec : uint8_t
            The order price precision.
        size_raw : int
            The order raw size (as a scaled fixed-point integer).
        size_prec : uint8_t
            The order size precision.
        order_id : uint64_t
            The order ID.
        flags : uint8_t
            The record flags bit field, indicating event end and data information.
            A value of zero indicates no flags.
        sequence : uint64_t
            The unique sequence number for the update.
            If no sequence number provided in the source data then use a value of zero.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the tick event occurred.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the data object was initialized.

        Returns
        -------
        OrderBookDelta

        """
        return OrderBookDelta.from_raw_c(
            instrument_id,
            action,
            side,
            price_raw,
            price_prec,
            size_raw,
            size_prec,
            order_id,
            flags,
            sequence,
            ts_event,
            ts_init,
        )

    @staticmethod
    def from_dict(dict values) -> OrderBookDelta:
        """
        Return an order book delta from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderBookDelta

        """
        return OrderBookDelta.from_dict_c(values)

    @staticmethod
    def to_dict(OrderBookDelta obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderBookDelta.to_dict_c(obj)

    @staticmethod
    def clear(InstrumentId instrument_id, uint64_t sequence, uint64_t ts_event, uint64_t ts_init):
        """
        Return an order book delta which acts as an initial ``CLEAR``.

        Returns
        -------
        OrderBookDelta

        """
        return OrderBookDelta.clear_c(instrument_id, sequence, ts_event, ts_init)


    @staticmethod
    def to_pyo3_list(list[OrderBookDelta] deltas) -> list[nautilus_pyo3.OrderBookDelta]:
        """
        Return pyo3 Rust order book deltas converted from the given legacy Cython objects.

        Parameters
        ----------
        pyo3_deltas : list[OrderBookDelta]
            The pyo3 Rust order book deltas to convert from.

        Returns
        -------
        list[nautilus_pyo3.OrderBookDelta]

        """
        cdef list output = []

        pyo3_instrument_id = None
        cdef uint8_t price_prec = 0
        cdef uint8_t size_prec = 0

        cdef:
            OrderBookDelta delta
            BookOrder book_order
        for delta in deltas:
            if pyo3_instrument_id is None:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(delta.instrument_id.value)
            if price_prec == 0:
                price_prec = delta._mem.order.price.precision
            if size_prec == 0:
                size_prec = delta._mem.order.size.precision

            pyo3_book_order = nautilus_pyo3.BookOrder(
               nautilus_pyo3.OrderSide(order_side_to_str(delta._mem.order.side)),
               nautilus_pyo3.Price.from_raw(delta._mem.order.price.raw, price_prec),
               nautilus_pyo3.Quantity.from_raw(delta._mem.order.size.raw, size_prec),
               delta._mem.order.order_id,
            )

            pyo3_delta = nautilus_pyo3.OrderBookDelta(
                pyo3_instrument_id,
                nautilus_pyo3.BookAction(book_action_to_str(delta._mem.action)),
                pyo3_book_order,
                delta._mem.flags,
                delta._mem.sequence,
                delta._mem.ts_event,
                delta._mem.ts_init,
            )
            output.append(pyo3_delta)

        return output

    @staticmethod
    def from_pyo3(pyo3_delta) -> OrderBookDelta:
        """
        Return a legacy Cython order book delta converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_delta : nautilus_pyo3.OrderBookDelta
            The pyo3 Rust order book delta to convert from.

        Returns
        -------
        OrderBookDelta

        """
        return OrderBookDelta.from_pyo3_c(pyo3_delta)

    @staticmethod
    def from_pyo3_list(list pyo3_deltas) -> list[OrderBookDelta]:
        """
        Return legacy Cython order book deltas converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_deltas : list[nautilus_pyo3.OrderBookDelta]
            The pyo3 Rust order book deltas to convert from.

        Returns
        -------
        list[OrderBookDelta]

        """
        cdef list[OrderBookDelta] output = []

        for pyo3_delta in pyo3_deltas:
            output.append(OrderBookDelta.from_pyo3_c(pyo3_delta))

        return output


cdef class OrderBookDeltas(Data):
    """
    Represents a batch of `OrderBookDelta` updates for an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    deltas : list[OrderBookDelta]
        The batch of order book changes.

    Raises
    ------
    ValueError
        If `deltas` is an empty list.

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        list deltas not None,
    ) -> None:
        Condition.not_empty(deltas, "deltas")

        cdef uint64_t len_ = len(deltas)

        # Create a C OrderBookDeltas_t buffer
        cdef OrderBookDelta_t *data = <OrderBookDelta_t *>PyMem_Malloc(len_ * sizeof(OrderBookDelta_t))
        if not data:
            raise MemoryError()

        cdef uint64_t i
        cdef OrderBookDelta delta
        for i in range(len_):
            delta = deltas[i]
            data[i] = <OrderBookDelta_t>delta._mem

        # Create CVec
        cdef CVec *cvec = <CVec *>PyMem_Malloc(1 * sizeof(CVec))
        if not cvec:
            raise MemoryError()

        cvec.ptr = data
        cvec.len = len_
        cvec.cap = len_

        # Transfer data to Rust
        self._mem = orderbook_deltas_new(
            instrument_id._mem,
            cvec,
        )

        PyMem_Free(cvec.ptr) # De-allocate buffer
        PyMem_Free(cvec) # De-allocate cvec

    def __getstate__(self):
        return (
            self.instrument_id.value,
            pickle.dumps(self.deltas),
        )

    def __setstate__(self, state):
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(state[0])

        cdef list deltas = pickle.loads(state[1])

        cdef uint64_t len_ = len(deltas)

        # Create a C OrderBookDeltas_t buffer
        cdef OrderBookDelta_t *data = <OrderBookDelta_t *>PyMem_Malloc(len_ * sizeof(OrderBookDelta_t))
        if not data:
            raise MemoryError()

        cdef uint64_t i
        cdef OrderBookDelta delta
        for i in range(len_):
            delta = deltas[i]
            data[i] = <OrderBookDelta_t>delta._mem

        # Create CVec
        cdef CVec *cvec = <CVec *>PyMem_Malloc(1 * sizeof(CVec))
        if not cvec:
            raise MemoryError()

        cvec.ptr = data
        cvec.len = len_
        cvec.cap = len_

        # Transfer data to Rust
        self._mem = orderbook_deltas_new(
            instrument_id._mem,
            cvec,
        )

        PyMem_Free(cvec.ptr) # De-allocate buffer
        PyMem_Free(cvec) # De-allocate cvec

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            orderbook_deltas_drop(self._mem)

    def __eq__(self, OrderBookDeltas other) -> bool:
        return OrderBookDeltas.to_dict_c(self) == OrderBookDeltas.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(OrderBookDeltas.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"{self.deltas}, "
            f"is_snapshot={self.is_snapshot}, "
            f"sequence={self.sequence}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the deltas book instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(orderbook_deltas_instrument_id(&self._mem))

    @property
    def deltas(self) -> list[OrderBookDelta]:
        """
        Return the contained deltas.

        Returns
        -------
        list[OrderBookDeltas]

        """
        cdef CVec raw_deltas_vec = orderbook_deltas_vec_deltas(&self._mem)
        cdef OrderBookDelta_t* raw_deltas = <OrderBookDelta_t*>raw_deltas_vec.ptr

        cdef list[OrderBookDelta] deltas = []

        cdef:
            uint64_t i
        for i in range(raw_deltas_vec.len):
            deltas.append(delta_from_mem_c(raw_deltas[i]))

        orderbook_deltas_vec_drop(raw_deltas_vec)

        return deltas

    @property
    def is_snapshot(self) -> bool:
        """
        If the deltas is a snapshot.

        Returns
        -------
        bool

        """
        return <bint>orderbook_deltas_is_snapshot(&self._mem)

    @property
    def flags(self) -> uint8_t:
        """
        Return the flags for the last delta.

        Returns
        -------
        uint8_t

        """
        return orderbook_deltas_flags(&self._mem)

    @property
    def sequence(self) -> uint64_t:
        """
        Return the sequence number for the last delta.

        Returns
        -------
        uint64_t

        """
        return orderbook_deltas_sequence(&self._mem)

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return orderbook_deltas_ts_event(&self._mem)

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return orderbook_deltas_ts_init(&self._mem)

    @staticmethod
    cdef OrderBookDeltas from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderBookDeltas(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            deltas=[OrderBookDelta.from_dict_c(d) for d in values["deltas"]],
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDeltas obj):
        Condition.not_none(obj, "obj")
        return {
            "type": obj.__class__.__name__,
            "instrument_id": obj.instrument_id.value,
            "deltas": [OrderBookDelta.to_dict_c(d) for d in obj.deltas],
        }

    @staticmethod
    def from_dict(dict values) -> OrderBookDeltas:
        """
        Return order book deltas from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderBookDeltas

        """
        return OrderBookDeltas.from_dict_c(values)

    @staticmethod
    def to_dict(OrderBookDeltas obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderBookDeltas.to_dict_c(obj)

    @staticmethod
    def batch(list data: list[OrderBookDelta]) -> list[OrderBookDeltas]:
        """
        Groups the given list of `OrderBookDelta` records into batches, creating `OrderBookDeltas`
        objects when an `F_LAST` flag is encountered.

        The method iterates through the `data` list and appends each `OrderBookDelta` to the current
        batch. When an `F_LAST` flag is found, it indicates the end of a batch. The batch is then
        appended to the list of completed batches and a new batch is started.

        Returns
        -------
        list[OrderBookDeltas]

        Raises
        ------
        ValueError
            If `data` is empty.
        TypeError
            If `data` is not a list of `OrderBookDelta`.

        Warnings
        --------
        UserWarning
            If there are remaining deltas in the final batch after the last `F_LAST` flag.

        """
        Condition.not_empty(data, "data")
        cdef OrderBookDelta first = data[0]

        cdef InstrumentId instrument_id = first.instrument_id
        cdef list[list[OrderBookDelta]] batches = []
        cdef list[OrderBookDelta] batch = []

        cdef:
            OrderBookDelta delta
        for delta in data:
            batch.append(delta)
            if delta.flags == RecordFlag.F_LAST:
                batches.append(batch)
                batch = []

        cdef list[OrderBookDeltas] deltas = [OrderBookDeltas(instrument_id, deltas=batch) for batch in batches]

        if batch:
            warnings.warn(
                f"Batched {len(batches):_} `OrderBookDeltas`, but found {len(batch):_} remaining deltas "
                "without an 'F_LAST' flag. This can indicate incomplete data processing, as deltas "
                "should typically end with an 'F_LAST' flag to signal the end of a batch. If using streaming, "
                "this warning can occur if the last chunk did not include a final 'F_LAST' delta.",
                UserWarning,
            )
            deltas.append(OrderBookDeltas(instrument_id, deltas=batch))

        return deltas

    cpdef to_capsule(self):
        cdef OrderBookDeltas_API *data = <OrderBookDeltas_API *>PyMem_Malloc(sizeof(OrderBookDeltas_API))
        data[0] = self._mem
        capsule = PyCapsule_New(data, NULL, <PyCapsule_Destructor>capsule_destructor_deltas)
        return capsule

    cpdef to_pyo3(self):
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.OrderBookDeltas

        """
        capsule = self.to_capsule()
        deltas = nautilus_pyo3.OrderBookDeltas.from_pycapsule(capsule)
        return deltas



cdef class OrderBookDepth10(Data):
    """
    Represents a self-contained order book update with a fixed depth of 10 levels per side.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    bids : list[BookOrder]
        The bid side orders for the update.
    asks : list[BookOrder]
        The ask side orders for the update.
    bid_counts : list[uint32_t]
        The count of bid orders per level for the update. Can be zeros if data not available.
    ask_counts : list[uint32_t]
        The count of ask orders per level for the update. Can be zeros if data not available.
    flags : uint8_t
        The record flags bit field, indicating event end and data information.
        A value of zero indicates no flags.
    sequence : uint64_t
        The unique sequence number for the update.
        If no sequence number provided in the source data then use a value of zero.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the tick event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `bids`, `asks`, `bid_counts`, `ask_counts` lengths are greater than 10.
    ValueError
        If `bids`, `asks`, `bid_counts`, `ask_counts` lengths are not equal.

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        list bids not None,
        list asks not None,
        list bid_counts not None,
        list ask_counts not None,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> None:
        cdef uint32_t bids_len = len(bids)
        cdef uint32_t asks_len = len(asks)
        Condition.is_true(bids_len <= 10, f"bids length greater than maximum 10, was {bids_len}")
        Condition.is_true(asks_len <= 10, f"asks length greater than maximum 10, was {asks_len}")
        Condition.equal(bids_len, asks_len, "bids length", "asks length")
        Condition.equal(bids_len, len(bid_counts), "len(bids)", "len(bid_counts)")
        Condition.equal(asks_len, len(ask_counts), "len(asks)", "len(ask_counts)")

        if bids_len < 10:
            # Fill remaining levels with with null orders and zero counts
            bids.extend([NULL_ORDER] * (10 - bids_len))
            asks.extend([NULL_ORDER] * (10 - asks_len))
            bid_counts.extend([0] * (10 - bids_len))
            ask_counts.extend([0] * (10 - asks_len))

        # Create temporary arrays to copy data to Rust
        cdef BookOrder_t *bids_array = <BookOrder_t *>PyMem_Malloc(DEPTH10_LEN * sizeof(BookOrder_t))
        cdef BookOrder_t *asks_array = <BookOrder_t *>PyMem_Malloc(DEPTH10_LEN * sizeof(BookOrder_t))
        cdef uint32_t *bid_counts_array = <uint32_t *>PyMem_Malloc(DEPTH10_LEN * sizeof(uint32_t))
        cdef uint32_t *ask_counts_array = <uint32_t *>PyMem_Malloc(DEPTH10_LEN * sizeof(uint32_t))
        if bids_array == NULL or asks_array == NULL or bid_counts_array == NULL or ask_counts_array == NULL:
            raise MemoryError("Failed to allocate memory for data transfer array")

        cdef uint64_t i
        cdef BookOrder order
        try:
            for i in range(DEPTH10_LEN):
                order = bids[i]
                bids_array[i] = <BookOrder_t>order._mem
                bid_counts_array[i] = bid_counts[i]
                order = asks[i]
                asks_array[i] = <BookOrder_t>order._mem
                ask_counts_array[i] = ask_counts[i]

            self._mem = orderbook_depth10_new(
                instrument_id._mem,
                bids_array,
                asks_array,
                bid_counts_array,
                ask_counts_array,
                flags,
                sequence,
                ts_event,
                ts_init,
            )
        finally:
            # Deallocate temporary data transfer arrays
            PyMem_Free(bids_array)
            PyMem_Free(asks_array)
            PyMem_Free(bid_counts_array)
            PyMem_Free(ask_counts_array)

    def __getstate__(self):
        return (
            self.instrument_id.value,
            pickle.dumps(self.bids),
            pickle.dumps(self.asks),
            pickle.dumps(self.bid_counts),
            pickle.dumps(self.ask_counts),
            self._mem.flags,
            self._mem.sequence,
            self._mem.ts_event,
            self._mem.ts_init,
        )

    def __setstate__(self, state):
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(state[0])

        # Create temporary arrays to copy data to Rust
        cdef BookOrder_t *bids_array = <BookOrder_t *>PyMem_Malloc(DEPTH10_LEN * sizeof(BookOrder_t))
        cdef BookOrder_t *asks_array = <BookOrder_t *>PyMem_Malloc(DEPTH10_LEN * sizeof(BookOrder_t))
        cdef uint32_t *bid_counts_array = <uint32_t *>PyMem_Malloc(DEPTH10_LEN * sizeof(uint32_t))
        cdef uint32_t *ask_counts_array = <uint32_t *>PyMem_Malloc(DEPTH10_LEN * sizeof(uint32_t))
        if bids_array == NULL or asks_array == NULL or bid_counts_array == NULL or ask_counts_array == NULL:
            raise MemoryError("Failed to allocate memory for data transfer array")

        cdef list[BookOrder] bids = pickle.loads(state[1])
        cdef list[BookOrder] asks = pickle.loads(state[2])
        cdef list[uint32_t] bid_counts = pickle.loads(state[3])
        cdef list[uint32_t] ask_counts = pickle.loads(state[4])

        cdef uint64_t i
        cdef BookOrder order
        try:
            for i in range(DEPTH10_LEN):
                order = bids[i]
                bids_array[i] = <BookOrder_t>order._mem
                bid_counts_array[i] = bid_counts[i]
                order = asks[i]
                asks_array[i] = <BookOrder_t>order._mem
                ask_counts_array[i] = ask_counts[i]

            self._mem = orderbook_depth10_new(
                instrument_id._mem,
                bids_array,
                asks_array,
                bid_counts_array,
                ask_counts_array,
                state[5],
                state[6],
                state[7],
                state[8],
            )
        finally:
            # Deallocate temporary data transfer arrays
            PyMem_Free(bids_array)
            PyMem_Free(asks_array)
            PyMem_Free(bid_counts_array)
            PyMem_Free(ask_counts_array)

    def __eq__(self, OrderBookDepth10 other) -> bool:
        return orderbook_depth10_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return orderbook_depth10_hash(&self._mem)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"bids={self.bids}, "
            f"asks={self.asks}, "
            f"bid_counts={self.bid_counts}, "
            f"ask_counts={self.ask_counts}, "
            f"flags={self.flags}, "
            f"sequence={self.sequence}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the depth updates book instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(self._mem.instrument_id)

    @property
    def bids(self) -> list[BookOrder]:
        """
        Return the bid orders for the update.

        Returns
        -------
        list[BookOrder]

        """
        cdef const BookOrder_t* bids_array = orderbook_depth10_bids_array(&self._mem)
        cdef list[BookOrder] bids = [];

        cdef uint64_t i
        cdef BookOrder order
        for i in range(DEPTH10_LEN):
            order = order_from_mem_c(bids_array[i])
            bids.append(order)

        return bids

    @property
    def asks(self) -> list[BookOrder]:
        """
        Return the ask orders for the update.

        Returns
        -------
        list[BookOrder]

        """
        cdef const BookOrder_t* asks_array = orderbook_depth10_asks_array(&self._mem)
        cdef list[BookOrder] asks = [];

        cdef uint64_t i
        cdef BookOrder order
        for i in range(DEPTH10_LEN):
            order = order_from_mem_c(asks_array[i])
            asks.append(order)

        return asks

    @property
    def bid_counts(self) -> list[uint32_t]:
        """
        Return the count of bid orders per level for the update.

        Returns
        -------
        list[uint32_t]

        """
        cdef const uint32_t* bid_counts_array = orderbook_depth10_bid_counts_array(&self._mem)
        cdef list[uint32_t] bid_counts = [];

        cdef uint64_t i
        for i in range(DEPTH10_LEN):
            bid_counts.append(<uint32_t>bid_counts_array[i])

        return bid_counts

    @property
    def ask_counts(self) -> list[uint32_t]:
        """
        Return the count of ask orders level for the update.

        Returns
        -------
        list[uint32_t]

        """
        cdef const uint32_t* ask_counts_array = orderbook_depth10_ask_counts_array(&self._mem)
        cdef list[uint32_t] ask_counts = [];

        cdef uint64_t i
        for i in range(DEPTH10_LEN):
            ask_counts.append(<uint32_t>ask_counts_array[i])

        return ask_counts

    @property
    def flags(self) -> uint8_t:
        """
        Return the flags for the depth update.

        Returns
        -------
        uint8_t

        """
        return self._mem.flags

    @property
    def sequence(self) -> uint64_t:
        """
        Return the sequence number for the depth update.

        Returns
        -------
        uint64_t

        """
        return self._mem.sequence

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._mem.ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._mem.ts_init

    @staticmethod
    cdef OrderBookDepth10 from_mem_c(OrderBookDepth10_t mem):
        return depth10_from_mem_c(mem)

    @staticmethod
    cdef OrderBookDepth10 from_pyo3_c(pyo3_depth10):
        # SAFETY: Do NOT deallocate the capsule here
        # It is supposed to be deallocated by the creator
        capsule = pyo3_depth10.as_pycapsule()
        cdef Data_t* ptr = <Data_t*>PyCapsule_GetPointer(capsule, NULL)
        return depth10_from_mem_c(orderbook_depth10_clone(ptr.depth10))

    @staticmethod
    cdef OrderBookDepth10 from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OrderBookDepth10(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            bids=[BookOrder.from_dict_c(o) for o in values["bids"]],
            asks=[BookOrder.from_dict_c(o) for o in values["asks"]],
            bid_counts=values["bid_counts"],
            ask_counts=values["ask_counts"],
            flags=values["flags"],
            sequence=values["sequence"],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(OrderBookDepth10 obj):
        Condition.not_none(obj, "obj")
        return {
            "type": obj.__class__.__name__,
            "instrument_id": obj.instrument_id.value,
            "bids": [BookOrder.to_dict_c(o) for o in obj.bids],
            "asks": [BookOrder.to_dict_c(o) for o in obj.asks],
            "bid_counts": obj.bid_counts,
            "ask_counts": obj.ask_counts,
            "flags": obj.flags,
            "sequence": obj.sequence,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    cdef list[OrderBookDepth10] capsule_to_list_c(object capsule):
        # SAFETY: Do NOT deallocate the capsule here
        # It is supposed to be deallocated by the creator
        cdef CVec* data = <CVec*>PyCapsule_GetPointer(capsule, NULL)
        cdef OrderBookDepth10_t* ptr = <OrderBookDepth10_t*>data.ptr
        cdef list[OrderBookDepth10] depths = []

        cdef uint64_t i
        for i in range(0, data.len):
            depths.append(depth10_from_mem_c(ptr[i]))

        return depths

    @staticmethod
    cdef object list_to_capsule_c(list items):
        # Create a C struct buffer
        cdef uint64_t len_ = len(items)
        cdef OrderBookDepth10_t * data = <OrderBookDepth10_t *>PyMem_Malloc(len_ * sizeof(OrderBookDepth10_t))
        cdef uint64_t i
        for i in range(len_):
            data[i] = (<OrderBookDepth10>items[i])._mem
        if not data:
            raise MemoryError()

        # Create CVec
        cdef CVec * cvec = <CVec *>PyMem_Malloc(1 * sizeof(CVec))
        cvec.ptr = data
        cvec.len = len_
        cvec.cap = len_

        # Create PyCapsule
        return PyCapsule_New(cvec, NULL, <PyCapsule_Destructor>capsule_destructor)

    @staticmethod
    def list_from_capsule(capsule) -> list[OrderBookDepth10]:
        return OrderBookDepth10.capsule_to_list_c(capsule)

    @staticmethod
    def capsule_from_list(list items):
        return OrderBookDepth10.list_to_capsule_c(items)

    @staticmethod
    def from_dict(dict values) -> OrderBookDepth10:
        """
        Return order book depth from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderBookDepth10

        """
        return OrderBookDepth10.from_dict_c(values)

    @staticmethod
    def to_dict(OrderBookDepth10 obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OrderBookDepth10.to_dict_c(obj)

    @staticmethod
    def from_pyo3(pyo3_depth) -> OrderBookDepth10:
        """
        Return a legacy Cython order book depth converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_depth : nautilus_pyo3.OrderBookDepth10
            The pyo3 Rust order book depth to convert from.

        Returns
        -------
        OrderBookDepth10

        """
        return OrderBookDepth10.from_pyo3_c(pyo3_depth)

    @staticmethod
    def from_pyo3_list(pyo3_depths) -> list[OrderBookDepth10]:
        """
        Return legacy Cython order book depths converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_depths : nautilus_pyo3.OrderBookDepth10
            The pyo3 Rust order book depths to convert from.

        Returns
        -------
        list[OrderBookDepth10]

        """
        cdef list[OrderBookDepth10] output = []

        for pyo3_depth in pyo3_depths:
            output.append(OrderBookDepth10.from_pyo3_c(pyo3_depth))

        return output


cdef class InstrumentStatus(Data):
    """
    Represents an event that indicates a change in an instrument market status.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the status change.
    action : MarketStatusAction
        The instrument market status action.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the status event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reason : str, optional
        Additional details about the cause of the status change.
    trading_event : str, optional
        Further information about the status change (if provided).
    is_trading : bool, optional
        The state of trading in the instrument.
    is_quoting : bool, optional
        The state of quoting in the instrument.
    is_short_sell_restricted : bool, optional
        The state of short sell restrictions for the instrument (if applicable).

    """

    def __init__(
        self,
        InstrumentId instrument_id,
        MarketStatusAction action,
        uint64_t ts_event,
        uint64_t ts_init,
        str reason = None,
        str trading_event = None,
        is_trading: bool | None = None,
        is_quoting: bool | None = None,
        is_short_sell_restricted: bool | None = None,
    ) -> None:
        self.instrument_id = instrument_id
        self.action = action
        self.ts_event = ts_event
        self.ts_init = ts_init
        self.reason = reason
        self.trading_event = trading_event
        self._is_trading = is_trading
        self._is_quoting = is_quoting
        self._is_short_sell_restricted = is_short_sell_restricted

    def __eq__(self, InstrumentStatus other) -> bool:
        return InstrumentStatus.to_dict_c(self) == InstrumentStatus.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(InstrumentStatus.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"action={market_status_action_to_str(self.action)}, "
            f"reason={self.reason}, "
            f"trading_event={self.trading_event}, "
            f"is_trading={self.is_trading}, "
            f"is_quoting={self.is_quoting}, "
            f"is_short_sell_restricted={self.is_short_sell_restricted}, "
            f"ts_event={self.ts_event})"
        )

    @property
    def is_trading(self) -> bool | None:
        """
        Return the state of trading in the instrument (if known).

        returns
        -------
        bool or ``None``

        """
        return self._is_trading

    @property
    def is_quoting(self) -> bool | None:
        """
        Return the state of quoting in the instrument (if known).

        returns
        -------
        bool or ``None``

        """
        return self._is_quoting

    @property
    def is_short_sell_restricted(self) -> bool | None:
        """
        Return the state of short sell restrictions for the instrument (if known and applicable).

        returns
        -------
        bool or ``None``

        """
        return self._is_short_sell_restricted

    @staticmethod
    cdef InstrumentStatus from_dict_c(dict values):
        Condition.not_none(values, "values")
        return InstrumentStatus(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            action=market_status_action_from_str(values["action"]),
            reason=values["reason"],
            trading_event=values["trading_event"],
            is_trading=values["is_trading"],
            is_quoting=values["is_quoting"],
            is_short_sell_restricted=values["is_short_sell_restricted"],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(InstrumentStatus obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "InstrumentStatus",
            "instrument_id": obj.instrument_id.to_str(),
            "action": market_status_action_to_str(obj.action),
            "reason": obj.reason,
            "trading_event": obj.trading_event,
            "is_trading": obj.is_trading,
            "is_quoting": obj.is_quoting,
            "is_short_sell_restricted": obj.is_short_sell_restricted,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> InstrumentStatus:
        """
        Return an instrument status update from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        InstrumentStatus

        """
        return InstrumentStatus.from_dict_c(values)

    @staticmethod
    def to_dict(InstrumentStatus obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return InstrumentStatus.to_dict_c(obj)

    @staticmethod
    def from_pyo3_list(list pyo3_status_list) -> list[QuoteTick]:
        """
        Return legacy Cython instrument status converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_status_list : list[nautilus_pyo3.InstrumentStatus]
            The pyo3 Rust instrument status list to convert from.

        Returns
        -------
        list[InstrumentStatus]

        """
        cdef list[InstrumentStatus] output = []

        for pyo3_status in pyo3_status_list:
            output.append(InstrumentStatus.from_pyo3(pyo3_status))

        return output

    @staticmethod
    def from_pyo3(pyo3_status) -> InstrumentStatus:
        """
        Return a legacy Cython quote tick converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_status : nautilus_pyo3.InstrumentStatus
            The pyo3 Rust instrument status to convert from.

        Returns
        -------
        InstrumentStatus

        """
        return InstrumentStatus(
            instrument_id=InstrumentId.from_str(pyo3_status.instrument_id.value),
            action=pyo3_status.action.value,
            ts_event=pyo3_status.ts_event,
            ts_init=pyo3_status.ts_init,
            reason=pyo3_status.reason,
            trading_event=pyo3_status.trading_event,
            is_trading=pyo3_status.is_trading,
            is_quoting=pyo3_status.is_quoting,
            is_short_sell_restricted=pyo3_status.is_short_sell_restricted,
        )

    def to_pyo3(self) -> nautilus_pyo3.InstrumentStatus:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.InstrumentStatus

        """
        return nautilus_pyo3.InstrumentStatus(
            nautilus_pyo3.InstrumentId.from_str(self.instrument_id.value),
            nautilus_pyo3.MarketStatusAction(market_status_action_to_str(self.action)),
            self.ts_event,
            self.ts_init,
            self.reason,
            self.trading_event,
            self.is_trading,
            self.is_quoting,
            self.is_short_sell_restricted,
        )


cdef class InstrumentClose(Data):
    """
    Represents an instrument close at a venue.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    close_price : Price
        The closing price for the instrument.
    close_type : InstrumentCloseType
        The type of closing price.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the close price event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price close_price not None,
        InstrumentCloseType close_type,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> None:
        self.instrument_id = instrument_id
        self.close_price = close_price
        self.close_type = close_type
        self.ts_event = ts_event
        self.ts_init = ts_init

    def __eq__(self, InstrumentClose other) -> bool:
        return InstrumentClose.to_dict_c(self) == InstrumentClose.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(InstrumentClose.to_dict_c(self)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"close_price={self.close_price}, "
            f"close_type={instrument_close_type_to_str(self.close_type)})"
        )

    @staticmethod
    cdef InstrumentClose from_dict_c(dict values):
        Condition.not_none(values, "values")
        return InstrumentClose(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            close_price=Price.from_str_c(values["close_price"]),
            close_type=instrument_close_type_from_str(values["close_type"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(InstrumentClose obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "InstrumentClose",
            "instrument_id": obj.instrument_id.to_str(),
            "close_price": str(obj.close_price),
            "close_type": instrument_close_type_to_str(obj.close_type),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> InstrumentClose:
        """
        Return an instrument close price event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        InstrumentClose

        """
        return InstrumentClose.from_dict_c(values)

    @staticmethod
    def to_dict(InstrumentClose obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return InstrumentClose.to_dict_c(obj)

    @staticmethod
    def to_pyo3_list(list[InstrumentClose] closes) -> list[nautilus_pyo3.InstrumentClose]:
        """
        Return pyo3 Rust index prices converted from the given legacy Cython objects.

        Parameters
        ----------
        closes : list[InstrumentClose]
            The legacy Cython Rust instrument closes to convert from.

        Returns
        -------
        list[nautilus_pyo3.InstrumentClose]

        """
        cdef list output = []

        pyo3_instrument_id = None
        cdef uint8_t price_prec = 0

        cdef:
            InstrumentClose close
        for close in closes:
            if pyo3_instrument_id is None:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(close.instrument_id.value)
                price_prec = close.close_price.precision

            pyo3_close = nautilus_pyo3.InstrumentClose(
                pyo3_instrument_id,
                nautilus_pyo3.Price(float(close.close_price), price_prec),
                nautilus_pyo3.InstrumentCloseType(instrument_close_type_to_str(close.close_type)),
                close.ts_event,
                close.ts_init,
            )
            output.append(pyo3_close)

        return output

    @staticmethod
    def from_pyo3_list(list pyo3_closes) -> list[InstrumentClose]:
        """
        Return legacy Cython instrument closes converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_closes : list[nautilus_pyo3.InstrumentClose]
            The pyo3 Rust instrument closes to convert from.

        Returns
        -------
        list[InstrumentClose]

        """
        cdef list[InstrumentClose] output = []

        for pyo3_close in pyo3_closes:
            output.append(InstrumentClose.from_pyo3_c(pyo3_close))

        return output

    @staticmethod
    def from_pyo3(pyo3_close) -> InstrumentClose:
        """
        Return a legacy Cython instrument close converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_close : nautilus_pyo3InstrumentClose.
            The pyo3 Rust instrument close to convert from.

        Returns
        -------
        InstrumentClose

        """
        return InstrumentClose(
            instrument_id=InstrumentId.from_str(pyo3_close.instrument_id.value),
            close_price=Price(float(pyo3_close.close_price), pyo3_close.close_price.precision),
            close_type=instrument_close_type_from_str(pyo3_close.close_type.value),
            ts_event=pyo3_close.ts_event,
            ts_init=pyo3_close.ts_init,
        )

    def to_pyo3(self) -> nautilus_pyo3.IndexPriceUpdate:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.InstrumentClose

        """
        return nautilus_pyo3.InstrumentClose(
            nautilus_pyo3.InstrumentId.from_str(self.instrument_id.value),
            nautilus_pyo3.Price(float(self.close_price), self.close_price.precision),
            nautilus_pyo3.InstrumentCloseType(instrument_close_type_to_str(self.close_type)),
            self.ts_event,
            self.ts_init,
        )


cdef class QuoteTick(Data):
    """
    Represents a single quote tick in a market.

    Contains information about the best top-of-book bid and ask.

    Parameters
    ----------
    instrument_id : InstrumentId
        The quotes instrument ID.
    bid_price : Price
        The top-of-book bid price.
    ask_price : Price
        The top-of-book ask price.
    bid_size : Quantity
        The top-of-book bid size.
    ask_size : Quantity
        The top-of-book ask size.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the tick event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `bid.precision` != `ask.precision`.
    ValueError
        If `bid_size.precision` != `ask_size.precision`.

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price bid_price not None,
        Price ask_price not None,
        Quantity bid_size not None,
        Quantity ask_size not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> None:
        Condition.equal(bid_price._mem.precision, ask_price._mem.precision, "bid_price.precision", "ask_price.precision")
        Condition.equal(bid_size._mem.precision, ask_size._mem.precision, "bid_size.precision", "ask_size.precision")

        self._mem = quote_tick_new(
            instrument_id._mem,
            bid_price._mem,
            ask_price._mem,
            bid_size._mem,
            ask_size._mem,
            ts_event,
            ts_init,
        )

    def __getstate__(self):
        return (
            self.instrument_id.value,
            self._mem.bid_price.raw,
            self._mem.ask_price.raw,
            self._mem.bid_price.precision,
            self._mem.ask_price.precision,
            self._mem.bid_size.raw,
            self._mem.ask_size.raw,
            self._mem.bid_size.precision,
            self._mem.ask_size.precision,
            self.ts_event,
            self.ts_init,
        )

    def __setstate__(self, state):
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(state[0])
        cdef Price_t bid_price = price_new(state[1], state[3])
        cdef Price_t ask_price = price_new(state[2], state[4])
        cdef Quantity_t bid_size = quantity_new(state[5], state[7])
        cdef Quantity_t ask_size = quantity_new(state[6], state[8])
        self._mem = quote_tick_new(
            instrument_id._mem,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            state[9],
            state[10],
        )

    def __eq__(self, QuoteTick other) -> bool:
        return quote_tick_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return quote_tick_hash(&self._mem)

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cdef str to_str(self):
        return cstr_to_pystr(quote_tick_to_cstr(&self._mem))

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the tick instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(self._mem.instrument_id)

    @property
    def bid_price(self) -> Price:
        """
        Return the top-of-book bid price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.bid_price.raw, self._mem.bid_price.precision)

    @property
    def ask_price(self) -> Price:
        """
        Return the top-of-book ask price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.ask_price.raw, self._mem.ask_price.precision)

    @property
    def bid_size(self) -> Quantity:
        """
        Return the top-of-book bid size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.bid_size.raw, self._mem.bid_size.precision)

    @property
    def ask_size(self) -> Quantity:
        """
        Return the top-of-book ask size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.ask_size.raw, self._mem.ask_size.precision)

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._mem.ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._mem.ts_init

    @staticmethod
    cdef QuoteTick from_mem_c(QuoteTick_t mem):
        return quote_from_mem_c(mem)

    @staticmethod
    cdef QuoteTick from_pyo3_c(pyo3_quote):
        # SAFETY: Do NOT deallocate the capsule here
        # It is supposed to be deallocated by the creator
        capsule = pyo3_quote.as_pycapsule()
        cdef Data_t* ptr = <Data_t*>PyCapsule_GetPointer(capsule, NULL)
        return quote_from_mem_c(ptr.quote)

    @staticmethod
    cdef QuoteTick from_dict_c(dict values):
        Condition.not_none(values, "values")
        return QuoteTick(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            bid_price=Price.from_str_c(values["bid_price"]),
            ask_price=Price.from_str_c(values["ask_price"]),
            bid_size=Quantity.from_str_c(values["bid_size"]),
            ask_size=Quantity.from_str_c(values["ask_size"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(QuoteTick obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": str(obj.instrument_id),
            "bid_price": str(obj.bid_price),
            "ask_price": str(obj.ask_price),
            "bid_size": str(obj.bid_size),
            "ask_size": str(obj.ask_size),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    cdef QuoteTick from_raw_c(
        InstrumentId instrument_id,
        PriceRaw bid_price_raw,
        PriceRaw ask_price_raw,
        uint8_t bid_price_prec,
        uint8_t ask_price_prec,
        QuantityRaw bid_size_raw,
        QuantityRaw ask_size_raw,
        uint8_t bid_size_prec,
        uint8_t ask_size_prec,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        cdef Price_t bid_price = price_new(bid_price_raw, bid_price_prec)
        cdef Price_t ask_price = price_new(ask_price_raw, ask_price_prec)
        cdef Quantity_t bid_size = quantity_new(bid_size_raw, bid_size_prec)
        cdef Quantity_t ask_size = quantity_new(ask_size_raw, ask_size_prec)
        cdef QuoteTick quote = QuoteTick.__new__(QuoteTick)
        quote._mem = quote_tick_new(
            instrument_id._mem,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        )
        return quote

    @staticmethod
    cdef list[QuoteTick] from_raw_arrays_to_list_c(
        InstrumentId instrument_id,
        uint8_t price_prec,
        uint8_t size_prec,
        double[:] bid_prices_raw,
        double[:] ask_prices_raw,
        double[:] bid_sizes_raw,
        double[:] ask_sizes_raw,
        uint64_t[:] ts_events,
        uint64_t[:] ts_inits,
    ):
        Condition.is_true(len(bid_prices_raw) == len(ask_prices_raw) == len(bid_sizes_raw) == len(ask_sizes_raw)
                       == len(ts_events) == len(ts_inits), "Array lengths must be equal")

        cdef int count = ts_events.shape[0]
        cdef list[QuoteTick] quotes = []

        cdef:
            int i
            cdef Price bid_price
            cdef Price ask_price
            cdef Quantity bid_size
            cdef Quantity ask_size
            QuoteTick quote
        for i in range(count):
            bid_price = Price(bid_prices_raw[i], price_prec)
            ask_price = Price(ask_prices_raw[i], price_prec)
            bid_size = Quantity(bid_sizes_raw[i], size_prec)
            ask_size = Quantity(ask_sizes_raw[i], size_prec)

            quote = QuoteTick.__new__(QuoteTick)
            quote._mem = quote_tick_new(
                instrument_id._mem,
                bid_price._mem,
                ask_price._mem,
                bid_size._mem,
                ask_size._mem,
                ts_events[i],
                ts_inits[i],
            )
            quotes.append(quote)

        return quotes

    @staticmethod
    def from_raw_arrays_to_list(
        instrument_id: InstrumentId,
        price_prec: int,
        size_prec: int,
        bid_prices_raw: np.ndarray,
        ask_prices_raw: np.ndarray,
        bid_sizes_raw: np.ndarray,
        ask_sizes_raw: np.ndarray,
        ts_events: np.ndarray,
        ts_inits: np.ndarray,
    ) -> list[QuoteTick]:
        return QuoteTick.from_raw_arrays_to_list_c(
            instrument_id,
            price_prec,
            size_prec,
            bid_prices_raw,
            ask_prices_raw,
            bid_sizes_raw,
            ask_sizes_raw,
            ts_events,
            ts_inits,
        )

    @staticmethod
    cdef list[QuoteTick] capsule_to_list_c(object capsule):
        # SAFETY: Do NOT deallocate the capsule here
        # It is supposed to be deallocated by the creator
        cdef CVec* data = <CVec*>PyCapsule_GetPointer(capsule, NULL)
        cdef QuoteTick_t* ptr = <QuoteTick_t*>data.ptr
        cdef list[QuoteTick] quotes = []

        cdef uint64_t i
        for i in range(0, data.len):
            quotes.append(quote_from_mem_c(ptr[i]))

        return quotes

    @staticmethod
    cdef object list_to_capsule_c(list items):
        # Create a C struct buffer
        cdef uint64_t len_ = len(items)
        cdef QuoteTick_t * data = <QuoteTick_t *>PyMem_Malloc(len_ * sizeof(QuoteTick_t))
        cdef uint64_t i
        for i in range(len_):
            data[i] = (<QuoteTick>items[i])._mem
        if not data:
            raise MemoryError()

        # Create CVec
        cdef CVec *cvec = <CVec *>PyMem_Malloc(1 * sizeof(CVec))
        cvec.ptr = data
        cvec.len = len_
        cvec.cap = len_

        # Create PyCapsule
        return PyCapsule_New(cvec, NULL, <PyCapsule_Destructor>capsule_destructor)

    @staticmethod
    def list_from_capsule(capsule) -> list[QuoteTick]:
        return QuoteTick.capsule_to_list_c(capsule)

    @staticmethod
    def capsule_from_list(list items):
        return QuoteTick.list_to_capsule_c(items)

    @staticmethod
    def from_raw(
        InstrumentId instrument_id,
        PriceRaw bid_price_raw,
        PriceRaw ask_price_raw,
        uint8_t bid_price_prec,
        uint8_t ask_price_prec,
        QuantityRaw bid_size_raw ,
        QuantityRaw ask_size_raw,
        uint8_t bid_size_prec,
        uint8_t ask_size_prec,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> QuoteTick:
        """
        Return a quote tick from the given raw values.

        Parameters
        ----------
        instrument_id : InstrumentId
            The quotes instrument ID.
        bid_price_raw : int
            The raw top-of-book bid price (as a scaled fixed-point integer).
        ask_price_raw : int
            The raw top-of-book ask price (as a scaled fixed-point integer).
        bid_price_prec : uint8_t
            The bid price precision.
        ask_price_prec : uint8_t
            The ask price precision.
        bid_size_raw : int
            The raw top-of-book bid size (as a scaled fixed-point integer).
        ask_size_raw : int
            The raw top-of-book ask size (as a scaled fixed-point integer).
        bid_size_prec : uint8_t
            The bid size precision.
        ask_size_prec : uint8_t
            The ask size precision.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the tick event occurred.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the data object was initialized.

        Returns
        -------
        QuoteTick

        Raises
        ------
        ValueError
            If `bid_price_prec` != `ask_price_prec`.
        ValueError
            If `bid_size_prec` != `ask_size_prec`.

        """
        Condition.equal(bid_price_prec, ask_price_prec, "bid_price_prec", "ask_price_prec")
        Condition.equal(bid_size_prec, ask_size_prec, "bid_size_prec", "ask_size_prec")

        return QuoteTick.from_raw_c(
            instrument_id,
            bid_price_raw,
            ask_price_raw,
            bid_price_prec,
            ask_price_prec,
            bid_size_raw,
            ask_size_raw,
            bid_size_prec,
            ask_size_prec,
            ts_event,
            ts_init,
        )

    @staticmethod
    def from_dict(dict values) -> QuoteTick:
        """
        Return a quote tick parsed from the given values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        QuoteTick

        """
        return QuoteTick.from_dict_c(values)

    @staticmethod
    def to_dict(QuoteTick obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return QuoteTick.to_dict_c(obj)

    @staticmethod
    def from_pyo3_list(list pyo3_quotes) -> list[QuoteTick]:
        """
        Return legacy Cython quotes converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_quotes : list[nautilus_pyo3.QuoteTick]
            The pyo3 Rust quotes to convert from.

        Returns
        -------
        list[QuoteTick]

        """
        cdef list[QuoteTick] output = []

        for pyo3_quote in pyo3_quotes:
            output.append(QuoteTick.from_pyo3_c(pyo3_quote))

        return output

    @staticmethod
    def to_pyo3_list(list[QuoteTick] quotes) -> list[nautilus_pyo3.QuoteTick]:
        """
        Return pyo3 Rust quotes converted from the given legacy Cython objects.

        Parameters
        ----------
        quotes : list[QuoteTick]
            The legacy Cython quotes to convert from.

        Returns
        -------
        list[nautilus_pyo3.QuoteTick]

        """
        cdef list output = []

        pyo3_instrument_id = None
        cdef uint8_t bid_prec = 0
        cdef uint8_t ask_prec = 0
        cdef uint8_t bid_size_prec = 0
        cdef uint8_t ask_size_prec = 0

        cdef:
            QuoteTick quote
        for quote in quotes:
            if pyo3_instrument_id is None:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(quote.instrument_id.value)
                bid_prec = quote.bid_price.precision
                ask_prec = quote.ask_price.precision
                bid_size_prec = quote.bid_size.precision
                ask_size_prec = quote.ask_size.precision

            pyo3_quote = nautilus_pyo3.QuoteTick(
                pyo3_instrument_id,
                nautilus_pyo3.Price.from_raw(quote._mem.bid_price.raw, bid_prec),
                nautilus_pyo3.Price.from_raw(quote._mem.ask_price.raw, ask_prec),
                nautilus_pyo3.Quantity.from_raw(quote._mem.bid_size.raw, bid_size_prec),
                nautilus_pyo3.Quantity.from_raw(quote._mem.ask_size.raw, ask_size_prec),
                quote._mem.ts_event,
                quote._mem.ts_init,
            )
            output.append(pyo3_quote)

        return output

    @staticmethod
    def from_pyo3(pyo3_quote) -> QuoteTick:
        """
        Return a legacy Cython quote tick converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_quote : nautilus_pyo3.QuoteTick
            The pyo3 Rust quote tick to convert from.

        Returns
        -------
        QuoteTick

        """
        return QuoteTick.from_pyo3_c(pyo3_quote)

    def to_pyo3(self) -> nautilus_pyo3.QuoteTick:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.QuoteTick

        """
        return nautilus_pyo3.QuoteTick(
            nautilus_pyo3.InstrumentId.from_str(self.instrument_id.value),
            nautilus_pyo3.Price.from_raw(self._mem.bid_price.raw, self._mem.bid_price.precision),
            nautilus_pyo3.Price.from_raw(self._mem.ask_price.raw, self._mem.ask_price.precision),
            nautilus_pyo3.Quantity.from_raw(self._mem.bid_size.raw, self._mem.bid_size.precision),
            nautilus_pyo3.Quantity.from_raw(self._mem.ask_size.raw, self._mem.ask_size.precision),
            self._mem.ts_event,
            self._mem.ts_init,
        )

    cpdef Price extract_price(self, PriceType price_type):
        """
        Extract the price for the given price type.

        Parameters
        ----------
        price_type : PriceType
            The price type to extract.

        Returns
        -------
        Price

        """
        if price_type == PriceType.MID:
            return Price.from_raw_c(((self._mem.bid_price.raw + self._mem.ask_price.raw) / 2), self._mem.bid_price.precision + 1)
        elif price_type == PriceType.BID:
            return self.bid_price
        elif price_type == PriceType.ASK:
            return self.ask_price
        else:
            raise ValueError(f"Cannot extract with PriceType {price_type_to_str(price_type)}")

    cpdef Quantity extract_size(self, PriceType price_type):
        """
        Extract the size for the given price type.

        Parameters
        ----------
        price_type : PriceType
            The price type to extract.

        Returns
        -------
        Quantity

        """
        if price_type == PriceType.MID:
            return Quantity.from_raw_c((self._mem.bid_size.raw + self._mem.ask_size.raw) / 2, self._mem.bid_size.precision + 1)
        elif price_type == PriceType.BID:
            return self.bid_size
        elif price_type == PriceType.ASK:
            return self.ask_size
        else:
            raise ValueError(f"Cannot extract with PriceType {price_type_to_str(price_type)}")


cdef class TradeTick(Data):
    """
    Represents a single trade tick in a market.

    Contains information about a single unique trade which matched buyer and
    seller counterparties.

    Parameters
    ----------
    instrument_id : InstrumentId
        The trade instrument ID.
    price : Price
        The traded price.
    size : Quantity
        The traded size.
    aggressor_side : AggressorSide
        The trade aggressor side.
    trade_id : TradeId
        The trade match ID (assigned by the venue).
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the tick event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `trade_id` is not a valid string.
    ValueError
        If `size` is not positive (> 0).

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price price not None,
        Quantity size not None,
        AggressorSide aggressor_side,
        TradeId trade_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> None:
        Condition.positive_int(size._mem.raw, "size")

        self._mem = trade_tick_new(
            instrument_id._mem,
            price._mem,
            size._mem,
            aggressor_side,
            trade_id._mem,
            ts_event,
            ts_init,
        )

    def __getstate__(self):
        return (
            self.instrument_id.value,
            self._mem.price.raw,
            self._mem.price.precision,
            self._mem.size.raw,
            self._mem.size.precision,
            self._mem.aggressor_side,
            self.trade_id.value,
            self.ts_event,
            self.ts_init,
        )

    def __setstate__(self, state):
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(state[0])
        cdef Price_t price = price_new(state[1], state[2])
        cdef Quantity_t size = quantity_new(state[3], state[4])
        Condition.positive_int(size.raw, "size")

        self._mem = trade_tick_new(
            instrument_id._mem,
            price,
            size,
            state[5],
            TradeId(state[6])._mem,
            state[7],
            state[8],
        )

    def __eq__(self, TradeTick other) -> bool:
        return trade_tick_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return trade_tick_hash(&self._mem)

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.to_str()})"

    cdef str to_str(self):
        return cstr_to_pystr(trade_tick_to_cstr(&self._mem))

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the ticks instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(self._mem.instrument_id)

    @property
    def trade_id(self) -> InstrumentId:
        """
        Return the ticks trade match ID.

        Returns
        -------
        Price

        """
        return TradeId.from_mem_c(self._mem.trade_id)

    @property
    def price(self) -> Price:
        """
        Return the ticks price.

        Returns
        -------
        Price

        """
        return Price.from_raw_c(self._mem.price.raw, self._mem.price.precision)

    @property
    def size(self) -> Quantity:
        """
        Return the ticks size.

        Returns
        -------
        Quantity

        """
        return Quantity.from_raw_c(self._mem.size.raw, self._mem.size.precision)

    @property
    def aggressor_side(self) -> AggressorSide:
        """
        Return the ticks aggressor side.

        Returns
        -------
        AggressorSide

        """
        return <AggressorSide>self._mem.aggressor_side

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._mem.ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._mem.ts_init

    @staticmethod
    cdef TradeTick from_mem_c(TradeTick_t mem):
        return trade_from_mem_c(mem)

    @staticmethod
    cdef TradeTick from_pyo3_c(pyo3_trade):
        # SAFETY: Do NOT deallocate the capsule here
        # It is supposed to be deallocated by the creator
        capsule = pyo3_trade.as_pycapsule()
        cdef Data_t* ptr = <Data_t*>PyCapsule_GetPointer(capsule, NULL)
        return trade_from_mem_c(ptr.trade)

    @staticmethod
    cdef TradeTick from_raw_c(
        InstrumentId instrument_id,
        PriceRaw price_raw,
        uint8_t price_prec,
        QuantityRaw size_raw,
        uint8_t size_prec,
        AggressorSide aggressor_side,
        TradeId trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        Condition.positive_int(size_raw, "size_raw")

        cdef Price_t price = price_new(price_raw, price_prec)
        cdef Quantity_t size = quantity_new(size_raw, size_prec)

        cdef TradeTick trade = TradeTick.__new__(TradeTick)
        trade._mem = trade_tick_new(
            instrument_id._mem,
            price,
            size,
            aggressor_side,
            trade_id._mem,
            ts_event,
            ts_init,
        )
        return trade

    @staticmethod
    cdef list[TradeTick] from_raw_arrays_to_list_c(
        InstrumentId instrument_id,
        uint8_t price_prec,
        uint8_t size_prec,
        double[:] prices_raw,
        double[:] sizes_raw,
        uint8_t[:] aggressor_sides,
        list[str] trade_ids,
        uint64_t[:] ts_events,
        uint64_t[:] ts_inits,
    ):
        Condition.is_true(len(prices_raw) == len(sizes_raw) == len(aggressor_sides) == len(trade_ids) ==
                       len(ts_events) == len(ts_inits), "Array lengths must be equal")

        cdef int count = ts_events.shape[0]
        cdef list[TradeTick] trades = []

        cdef:
            int i
            Price price
            Quantity size
            AggressorSide aggressor_side
            TradeId trade_id
            TradeTick trade
        for i in range(count):
            price = Price(prices_raw[i], price_prec)
            size = Quantity(sizes_raw[i], size_prec)
            Condition.positive_int(size.raw, "size")
            aggressor_side = <AggressorSide>aggressor_sides[i]
            trade_id = TradeId(trade_ids[i])
            trade = TradeTick.__new__(TradeTick)
            trade._mem = trade_tick_new(
                instrument_id._mem,
                price._mem,
                size._mem,
                aggressor_side,
                trade_id._mem,
                ts_events[i],
                ts_inits[i],
            )
            trades.append(trade)

        return trades

    @staticmethod
    def from_raw_arrays_to_list(
        InstrumentId instrument_id,
        uint8_t price_prec,
        uint8_t size_prec,
        double[:] prices_raw,
        double[:] sizes_raw,
        uint8_t[:] aggressor_sides,
        list[str] trade_ids,
        uint64_t[:] ts_events,
        uint64_t[:] ts_inits,
    ) -> list[TradeTick]:
        return TradeTick.from_raw_arrays_to_list_c(
            instrument_id,
            price_prec,
            size_prec,
            prices_raw,
            sizes_raw,
            aggressor_sides,
            trade_ids,
            ts_events,
            ts_inits,
        )

    @staticmethod
    cdef list[TradeTick] capsule_to_list_c(capsule):
        # SAFETY: Do NOT deallocate the capsule here
        # It is supposed to be deallocated by the creator
        cdef CVec* data = <CVec *>PyCapsule_GetPointer(capsule, NULL)
        cdef TradeTick_t* ptr = <TradeTick_t *>data.ptr
        cdef list[TradeTick] trades = []

        cdef uint64_t i
        for i in range(0, data.len):
            trades.append(trade_from_mem_c(ptr[i]))

        return trades

    @staticmethod
    cdef object list_to_capsule_c(list items):
        # Create a C struct buffer
        cdef uint64_t len_ = len(items)
        cdef TradeTick_t *data = <TradeTick_t *>PyMem_Malloc(len_ * sizeof(TradeTick_t))
        cdef uint64_t i
        for i in range(len_):
            data[i] = (<TradeTick>items[i])._mem
        if not data:
            raise MemoryError()

        # Create CVec
        cdef CVec *cvec = <CVec *>PyMem_Malloc(1 * sizeof(CVec))
        cvec.ptr = data
        cvec.len = len_
        cvec.cap = len_

        # Create PyCapsule
        return PyCapsule_New(cvec, NULL, <PyCapsule_Destructor>capsule_destructor)

    @staticmethod
    def list_from_capsule(capsule) -> list[TradeTick]:
        return TradeTick.capsule_to_list_c(capsule)

    @staticmethod
    def capsule_from_list(items):
        return TradeTick.list_to_capsule_c(items)

    @staticmethod
    cdef TradeTick from_dict_c(dict values):
        Condition.not_none(values, "values")
        return TradeTick(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            price=Price.from_str_c(values["price"]),
            size=Quantity.from_str_c(values["size"]),
            aggressor_side=aggressor_side_from_str(values["aggressor_side"]),
            trade_id=TradeId(values["trade_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(TradeTick obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": str(obj.instrument_id),
            "price": str(obj.price),
            "size": str(obj.size),
            "aggressor_side": aggressor_side_to_str(obj._mem.aggressor_side),
            "trade_id": str(obj.trade_id),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_raw(
        InstrumentId instrument_id,
        PriceRaw price_raw,
        uint8_t price_prec,
        QuantityRaw size_raw,
        uint8_t size_prec,
        AggressorSide aggressor_side,
        TradeId trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> TradeTick:
        """
        Return a trade tick from the given raw values.

        Parameters
        ----------
        instrument_id : InstrumentId
            The trade instrument ID.
        price_raw : int
            The traded raw price (as a scaled fixed-point integer).
        price_prec : uint8_t
            The traded price precision.
        size_raw : int
            The traded raw size (as a scaled fixed-point integer).
        size_prec : uint8_t
            The traded size precision.
        aggressor_side : AggressorSide
            The trade aggressor side.
        trade_id : TradeId
            The trade match ID (assigned by the venue).
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the tick event occurred.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the data object was initialized.

        Returns
        -------
        TradeTick

        """
        return TradeTick.from_raw_c(
            instrument_id,
            price_raw,
            price_prec,
            size_raw,
            size_prec,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        )

    @staticmethod
    def from_dict(dict values) -> TradeTick:
        """
        Return a trade tick from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        TradeTick

        """
        return TradeTick.from_dict_c(values)

    @staticmethod
    def to_dict(TradeTick obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return TradeTick.to_dict_c(obj)

    @staticmethod
    def to_pyo3_list(list[TradeTick] trades) -> list[nautilus_pyo3.TradeTick]:
        """
        Return pyo3 Rust trades converted from the given legacy Cython objects.

        Parameters
        ----------
        ticks : list[TradeTick]
            The legacy Cython Rust trades to convert from.

        Returns
        -------
        list[nautilus_pyo3.TradeTick]

        """
        cdef list output = []

        pyo3_instrument_id = None
        cdef uint8_t price_prec = 0
        cdef uint8_t size_prec = 0

        cdef:
            TradeTick trade
        for trade in trades:
            if pyo3_instrument_id is None:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(trade.instrument_id.value)
                price_prec = trade.price.precision
                size_prec = trade.size.precision

            pyo3_trade = nautilus_pyo3.TradeTick(
                pyo3_instrument_id,
                nautilus_pyo3.Price.from_raw(trade._mem.price.raw, price_prec),
                nautilus_pyo3.Quantity.from_raw(trade._mem.size.raw, size_prec),
                nautilus_pyo3.AggressorSide(aggressor_side_to_str(trade._mem.aggressor_side)),
                nautilus_pyo3.TradeId(trade.trade_id.value),
                trade._mem.ts_event,
                trade._mem.ts_init,
            )
            output.append(pyo3_trade)

        return output

    @staticmethod
    def from_pyo3_list(list pyo3_trades) -> list[TradeTick]:
        """
        Return legacy Cython trades converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_trades : list[nautilus_pyo3.TradeTick]
            The pyo3 Rust trades to convert from.

        Returns
        -------
        list[TradeTick]

        """
        cdef list[TradeTick] output = []

        for pyo3_trade in pyo3_trades:
            output.append(TradeTick.from_pyo3_c(pyo3_trade))

        return output

    @staticmethod
    def from_pyo3(pyo3_trade) -> TradeTick:
        """
        Return a legacy Cython trade tick converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_trade : nautilus_pyo3.TradeTick
            The pyo3 Rust trade tick to convert from.

        Returns
        -------
        TradeTick

        """
        return TradeTick.from_pyo3_c(pyo3_trade)

    def to_pyo3(self) -> nautilus_pyo3.TradeTick:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.TradeTick

        """
        return nautilus_pyo3.TradeTick(
            nautilus_pyo3.InstrumentId.from_str(self.instrument_id.value),
            nautilus_pyo3.Price.from_raw(self._mem.price.raw, self._mem.price.precision),
            nautilus_pyo3.Quantity.from_raw(self._mem.size.raw, self._mem.size.precision),
            nautilus_pyo3.AggressorSide(aggressor_side_to_str(self._mem.aggressor_side)),
            nautilus_pyo3.TradeId(self.trade_id.value),
            self._mem.ts_event,
            self._mem.ts_init,
        )


cdef class MarkPriceUpdate(Data):
    """
    Represents a mark price update.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the mark price.
    value : Price
        The mark price.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the update occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price value not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> None:
        self.instrument_id = instrument_id
        self.value = value
        self.ts_event = ts_event
        self.ts_init = ts_init

    def __eq__(self, MarkPriceUpdate other) -> bool:
        return self.to_str() == other.to_str()

    def __hash__(self) -> int:
        return hash(self.to_str())

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.to_str()})"

    cdef str to_str(self):
        return f"{self.instrument_id},{self.value},{self.ts_event},{self.ts_init}"

    @staticmethod
    cdef MarkPriceUpdate from_dict_c(dict values):
        Condition.not_none(values, "values")
        return MarkPriceUpdate(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            value=Price.from_str_c(values["value"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(MarkPriceUpdate obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": str(obj.instrument_id),
            "value": str(obj.value),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> MarkPriceUpdate:
        """
        Return a mark price from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        MarkPriceUpdate

        """
        return MarkPriceUpdate.from_dict_c(values)

    @staticmethod
    def to_dict(MarkPriceUpdate obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return MarkPriceUpdate.to_dict_c(obj)

    @staticmethod
    def to_pyo3_list(list[MarkPriceUpdate] mark_prices) -> list[nautilus_pyo3.MarkPriceUpdate]:
        """
        Return pyo3 Rust mark prices converted from the given legacy Cython objects.

        Parameters
        ----------
        mark_prices : list[MarkPriceUpdate]
            The legacy Cython Rust mark prices to convert from.

        Returns
        -------
        list[nautilus_pyo3.MarkPriceUpdate]

        """
        cdef list output = []

        pyo3_instrument_id = None
        cdef uint8_t price_prec = 0

        cdef:
            MarkPriceUpdate mark_price
        for mark_price in mark_prices:
            if pyo3_instrument_id is None:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(mark_price.instrument_id.value)
                price_prec = mark_price.value.precision

            pyo3_mark_price = nautilus_pyo3.MarkPriceUpdate(
                pyo3_instrument_id,
                nautilus_pyo3.Price(float(mark_price.value), price_prec),
                mark_price.ts_event,
                mark_price.ts_init,
            )
            output.append(pyo3_mark_price)

        return output

    @staticmethod
    def from_pyo3_list(list pyo3_mark_prices) -> list[MarkPriceUpdate]:
        """
        Return legacy Cython trades converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_mark_prices : list[nautilus_pyo3.MarkPriceUpdate]
            The pyo3 Rust mark prices to convert from.

        Returns
        -------
        list[MarkPriceUpdate]

        """
        cdef list[MarkPriceUpdate] output = []

        for pyo3_mark_price in pyo3_mark_prices:
            output.append(MarkPriceUpdate.from_pyo3_c(pyo3_mark_price))

        return output

    @staticmethod
    def from_pyo3(pyo3_mark_price) -> MarkPriceUpdate:
        """
        Return a legacy Cython mark price converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_trade : nautilus_pyo3.MarkPriceUpdate
            The pyo3 Rust mark price to convert from.

        Returns
        -------
        MarkPriceUpdate

        """
        return MarkPriceUpdate(
            instrument_id=InstrumentId.from_str(pyo3_mark_price.instrument_id.value),
            value=Price(float(pyo3_mark_price.value), pyo3_mark_price.value.precision),
            ts_event=pyo3_mark_price.ts_event,
            ts_init=pyo3_mark_price.ts_init,
        )

    def to_pyo3(self) -> nautilus_pyo3.MarkPriceUpdate:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.MarkPriceUpdate

        """
        return nautilus_pyo3.MarkPriceUpdate(
            nautilus_pyo3.InstrumentId.from_str(self.instrument_id.value),
            nautilus_pyo3.Price(float(self.value), self.value.precision),
            self.ts_event,
            self.ts_init,
        )


cdef class IndexPriceUpdate(Data):
    """
    Represents an index price update.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the index price.
    value : Price
        The index price.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the update occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price value not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> None:
        self.instrument_id = instrument_id
        self.value = value
        self.ts_event = ts_event
        self.ts_init = ts_init

    def __eq__(self, IndexPriceUpdate other) -> bool:
        return self.to_str() == other.to_str()

    def __hash__(self) -> int:
        return hash(self.to_str())

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.to_str()})"

    cdef str to_str(self):
        return f"{self.instrument_id},{self.value},{self.ts_event},{self.ts_init}"

    @staticmethod
    cdef IndexPriceUpdate from_dict_c(dict values):
        Condition.not_none(values, "values")
        return IndexPriceUpdate(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            value=Price.from_str_c(values["value"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(IndexPriceUpdate obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": str(obj.instrument_id),
            "value": str(obj.value),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> IndexPriceUpdate:
        """
        Return an index price from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        IndexPriceUpdate

        """
        return IndexPriceUpdate.from_dict_c(values)

    @staticmethod
    def to_dict(IndexPriceUpdate obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return IndexPriceUpdate.to_dict_c(obj)

    @staticmethod
    def to_pyo3_list(list[IndexPriceUpdate] index_prices) -> list[nautilus_pyo3.IndexPriceUpdate]:
        """
        Return pyo3 Rust index prices converted from the given legacy Cython objects.

        Parameters
        ----------
        mark_prices : list[IndexPriceUpdate]
            The legacy Cython Rust index prices to convert from.

        Returns
        -------
        list[nautilus_pyo3.IndexPriceUpdate]

        """
        cdef list output = []

        pyo3_instrument_id = None
        cdef uint8_t price_prec = 0

        cdef:
            IndexPriceUpdate index_price
        for index_price in index_prices:
            if pyo3_instrument_id is None:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(index_price.instrument_id.value)
                price_prec = index_price.value.precision

            pyo3_index_price = nautilus_pyo3.IndexPriceUpdate(
                pyo3_instrument_id,
                nautilus_pyo3.Price(float(index_price.value), price_prec),
                index_price.ts_event,
                index_price.ts_init,
            )
            output.append(pyo3_index_price)

        return output

    @staticmethod
    def from_pyo3_list(list pyo3_index_prices) -> list[IndexPriceUpdate]:
        """
        Return legacy Cython index prices converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_index_prices : list[nautilus_pyo3.IndexPriceUpdate]
            The pyo3 Rust index prices to convert from.

        Returns
        -------
        list[IndexPriceUpdate]

        """
        cdef list[IndexPriceUpdate] output = []

        for pyo3_index_price in pyo3_index_prices:
            output.append(IndexPriceUpdate.from_pyo3_c(pyo3_index_price))

        return output

    @staticmethod
    def from_pyo3(pyo3_index_price) -> IndexPriceUpdate:
        """
        Return a legacy Cython index price converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_trade : nautilus_pyo3.IndexPriceUpdate
            The pyo3 Rust index price to convert from.

        Returns
        -------
        IndexPriceUpdate

        """
        return IndexPriceUpdate(
            instrument_id=InstrumentId.from_str(pyo3_index_price.instrument_id.value),
            value=Price(float(pyo3_index_price.value), pyo3_index_price.value.precision),
            ts_event=pyo3_index_price.ts_event,
            ts_init=pyo3_index_price.ts_init,
        )

    def to_pyo3(self) -> nautilus_pyo3.IndexPriceUpdate:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.IndexPriceUpdate

        """
        return nautilus_pyo3.IndexPriceUpdate(
            nautilus_pyo3.InstrumentId.from_str(self.instrument_id.value),
            nautilus_pyo3.Price(float(self.value), self.value.precision),
            self.ts_event,
            self.ts_init,
        )
