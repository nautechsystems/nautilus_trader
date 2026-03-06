from libc.stdint cimport uint64_t

from nautilus_trader.model.book cimport BookOrder
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class FillModel:
    cdef readonly double prob_fill_on_limit
    """The probability of limit orders filling on the limit price.\n\n:returns: `bool`"""
    cdef readonly double prob_slippage
    """The probability of aggressive order execution slipping.\n\n:returns: `bool`"""

    cpdef bint fill_limit_inside_spread(self)
    cpdef bint is_limit_filled(self)
    cpdef bint is_slipped(self)
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )

    cdef bint _event_success(self, double probability)


cdef class BestPriceFillModel(FillModel):
    cpdef bint fill_limit_inside_spread(self)
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class OneTickSlippageFillModel(FillModel):
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class TwoTierFillModel(FillModel):
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class ProbabilisticFillModel(FillModel):
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class SizeAwareFillModel(FillModel):
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class LimitOrderPartialFillModel(FillModel):
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class ThreeTierFillModel(FillModel):
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class MarketHoursFillModel(FillModel):
    cdef bint _is_low_liquidity

    cpdef bint is_low_liquidity_period(self)
    cpdef void set_low_liquidity_period(self, bint is_low_liquidity)
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class VolumeSensitiveFillModel(FillModel):
    cdef double _recent_volume

    cpdef void set_recent_volume(self, double volume)
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class CompetitionAwareFillModel(FillModel):
    cdef double liquidity_factor

    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )
