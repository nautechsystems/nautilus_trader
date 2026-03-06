from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CashAccount(Account):
    cdef dict _balances_locked

    cdef readonly bint allow_borrowing
    """If borrowing is allowed (negative balances).\n\n:returns: `bool`"""

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void update_balance_locked(self, InstrumentId instrument_id, Money locked)
    cpdef void clear_balance_locked(self, InstrumentId instrument_id)

# -- CALCULATIONS ---------------------------------------------------------------------------------

    cpdef Money calculate_balance_locked(
        self,
        Instrument instrument,
        OrderSide side,
        Quantity quantity,
        Price price,
        bint use_quote_for_inverse=*,
    )

    @staticmethod
    cdef dict to_dict_c(CashAccount obj)

    @staticmethod
    cdef CashAccount from_dict_c(dict values)
