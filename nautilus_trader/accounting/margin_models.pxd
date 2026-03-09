from decimal import Decimal

from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class MarginModel:
    cpdef Money calculate_margin_init(
        self,
        Instrument instrument,
        Quantity quantity,
        Price price,
        leverage,
        bint use_quote_for_inverse=*,
    )

    cpdef Money calculate_margin_maint(
        self,
        Instrument instrument,
        PositionSide side,
        Quantity quantity,
        Price price,
        leverage,
        bint use_quote_for_inverse=*,
    )


cdef class StandardMarginModel(MarginModel):
    pass


cdef class LeveragedMarginModel(MarginModel):
    pass
