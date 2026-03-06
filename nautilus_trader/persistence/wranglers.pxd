from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport BookAction
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.core.rust.model cimport QuantityRaw
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.instruments.base cimport Instrument


cdef class OrderBookDeltaDataWrangler:
    cdef readonly Instrument instrument

    cpdef OrderBookDelta _build_delta(
        self,
        BookAction action,
        OrderSide side,
        double price,
        double size,
        uint64_t order_id,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    )


cdef class QuoteTickDataWrangler:
    cdef readonly Instrument instrument

    cpdef QuoteTick _build_tick(
        self,
        double bid_price,
        double ask_price,
        double bid_size,
        double ask_size,
        uint64_t ts_event,
        uint64_t ts_init,
    )


cdef class TradeTickDataWrangler:
    cdef readonly Instrument instrument
    cdef readonly processed_data

    cpdef TradeTick _build_tick(
        self,
        double price,
        double size,
        AggressorSide aggressor_side,
        str trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    )


cdef class BarDataWrangler:
    cdef readonly BarType bar_type
    cdef readonly Instrument instrument

    cpdef Bar _build_bar(self, double[:] values, uint64_t ts_event, uint64_t ts_init_delta)
