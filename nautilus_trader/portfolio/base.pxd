from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price


cdef class PortfolioFacade:

# -- QUERIES --------------------------------------------------------------------------------------  # noqa

    cdef readonly bint initialized
    """If the portfolio is initialized.\n\n:returns: `bool`"""

    cdef readonly analyzer
    """The portfolios analyzer.\n\n:returns: `PortfolioAnalyzer`"""

    cpdef Account account(self, Venue venue=*, AccountId account_id=*)

    cpdef dict balances_locked(self, Venue venue=*, AccountId account_id=*)
    cpdef dict margins_init(self, Venue venue=*, AccountId account_id=*)
    cpdef dict margins_maint(self, Venue venue=*, AccountId account_id=*)
    cpdef dict realized_pnls(self, Venue venue=*, AccountId account_id=*, Currency target_currency=*)
    cpdef dict unrealized_pnls(self, Venue venue=*, AccountId account_id=*, Currency target_currency=*)
    cpdef dict total_pnls(self, Venue venue=*, AccountId account_id=*, Currency target_currency=*)
    cpdef dict net_exposures(self, Venue venue=*, AccountId account_id=*, Currency target_currency=*)

    cpdef Money realized_pnl(self, InstrumentId instrument_id, AccountId account_id=*, Currency target_currency=*)
    cpdef Money unrealized_pnl(self, InstrumentId instrument_id, Price price=*, AccountId account_id=*, Currency target_currency=*)
    cpdef Money total_pnl(self, InstrumentId instrument_id, Price price=*, AccountId account_id=*, Currency target_currency=*)
    cpdef Money net_exposure(self, InstrumentId instrument_id, Price price=*, AccountId account_id=*, Currency target_currency=*)
    cpdef object net_position(self, InstrumentId instrument_id, AccountId account_id=*)

    cpdef bint is_net_long(self, InstrumentId instrument_id, AccountId account_id=*)
    cpdef bint is_net_short(self, InstrumentId instrument_id, AccountId account_id=*)
    cpdef bint is_flat(self, InstrumentId instrument_id, AccountId account_id=*)
    cpdef bint is_completely_flat(self, AccountId account_id=*)
