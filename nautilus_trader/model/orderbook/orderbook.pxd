from nautilus_trader.model.orderbook.ladder import Ladder

cdef class OrderbookProxy:
    cdef Ladder bids
    cdef Ladder asks
