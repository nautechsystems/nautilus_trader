from nautilus_trader.accounting.accounts.cash cimport CashAccount
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BettingAccount(CashAccount):
    cpdef Money calculate_balance_locked(
        self,
        Instrument instrument,
        OrderSide side,
        Quantity quantity,
        Price price,
        bint use_quote_for_inverse=*,
    )


cpdef stake(Quantity quantity, Price price)
cpdef liability(Quantity quantity, Price price, OrderSide side)
cpdef win_payoff(Quantity quantity, Price price, OrderSide side)
cpdef lose_payoff(Quantity quantity, OrderSide side)
cpdef exposure(Quantity quantity, Price price, OrderSide side)
