from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class PositionSizer:
    cdef readonly Instrument instrument
    """The instrument for position sizing.\n\n:returns: `Instrument`"""

    cpdef void update_instrument(self, Instrument instrument)
    cpdef Quantity calculate(
        self,
        Price entry,
        Price stop_loss,
        Money equity,
        risk,
        commission_rate=*,
        exchange_rate=*,
        hard_limit=*,
        unit_batch_size=*,
        int units=*,
    )

    cdef object _calculate_risk_ticks(self, Price entry, Price stop_loss)
    cdef object _calculate_riskable_money(self, equity, risk, commission_rate)


cdef class FixedRiskSizer(PositionSizer):
    pass
