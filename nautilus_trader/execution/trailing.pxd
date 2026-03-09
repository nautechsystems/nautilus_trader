from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport TrailingOffsetType
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class TrailingStopCalculator:

    @staticmethod
    cdef tuple calculate(
        Price price_increment,
        Order order,
        Price bid,
        Price ask,
        Price last,
    )

    @staticmethod
    cdef Price calculate_with_last(
        Price price_increment,
        TrailingOffsetType trailing_offset_type,
        OrderSide side,
        double offset,
        Price last,
    )

    @staticmethod
    cdef Price calculate_with_bid_ask(
        Price price_increment,
        TrailingOffsetType trailing_offset_type,
        OrderSide side,
        double offset,
        Price bid,
        Price ask,
    )
