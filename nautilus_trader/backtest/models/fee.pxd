from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class FeeModel:
    cpdef Money get_commission(self, Order order, Quantity fill_qty, Price fill_px, Instrument instrument)


cdef class MakerTakerFeeModel(FeeModel):
    pass


cdef class FixedFeeModel(FeeModel):
    cdef Money _commission
    cdef Money _zero_commission
    cdef bint _charge_commission_once


cdef class PerContractFeeModel(FeeModel):
    cdef Money _commission
