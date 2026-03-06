from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position


cdef class Account:
    cdef list _events
    cdef dict _balances
    cdef dict _balances_starting
    cdef dict _commissions

    cdef readonly AccountId id
    """The accounts ID.\n\n:returns: `AccountId`"""
    cdef readonly AccountType type
    """The accounts type.\n\n:returns: `AccountType`"""
    cdef readonly Currency base_currency
    """The accounts base currency (``None`` for multi-currency accounts).\n\n:returns: `Currency` or ``None``"""
    cdef readonly bint is_cash_account
    """If the account is a type of ``CASH`` account."""
    cdef readonly bint is_margin_account
    """If the account is a type of ``MARGIN`` account."""
    cdef readonly bint calculate_account_state
    """If the accounts state should be calculated by Nautilus.\n\n:returns: `bool`"""

# -- QUERIES ---------------------------------------------------------------------------------------

    cdef AccountState last_event_c(self)
    cdef list events_c(self)
    cdef int event_count_c(self)

    cpdef list currencies(self)
    cpdef dict starting_balances(self)
    cpdef dict balances(self)
    cpdef dict balances_total(self)
    cpdef dict balances_free(self)
    cpdef dict balances_locked(self)
    cpdef dict commissions(self)
    cpdef AccountBalance balance(self, Currency currency=*)
    cpdef Money balance_total(self, Currency currency=*)
    cpdef Money balance_free(self, Currency currency=*)
    cpdef Money balance_locked(self, Currency currency=*)
    cpdef Money commission(self, Currency currency)

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void apply(self, AccountState event)
    cpdef void update_balances(self, list balances)
    cpdef void update_commissions(self, Money commission)
    cpdef void purge_account_events(self, uint64_t ts_now, uint64_t lookback_secs=*)

# -- CALCULATIONS ----------------------------------------------------------------------------------

    cpdef bint is_unleveraged(self, InstrumentId instrument_id)
    cdef void _recalculate_balance(self, Currency currency)

    cpdef Money calculate_commission(
        self,
        Instrument instrument,
        Quantity last_qty,
        Price last_px,
        LiquiditySide liquidity_side,
        bint use_quote_for_inverse=*,
    )

    cpdef list calculate_pnls(
        self,
        Instrument instrument,
        OrderFilled fill,
        Position position=*,
    )

    cpdef Money balance_impact(
        self,
        Instrument instrument,
        Quantity quantity,
        Price price,
        OrderSide order_side,
    )
