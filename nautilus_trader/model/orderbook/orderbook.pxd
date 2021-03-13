from nautilus_trader.model.orderbook.ladder import Ladder

cdef class Orderbook:
    cdef Ladder bids
    cdef Ladder asks
