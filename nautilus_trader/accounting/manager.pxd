from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.accounts.cash cimport CashAccount
from nautilus_trader.accounting.accounts.margin cimport MarginAccount
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money


cdef class AccountsManager:
    cdef Clock _clock
    cdef Logger _log
    cdef CacheFacade _cache

    cpdef AccountState generate_account_state(self, Account account, uint64_t ts_event)
    cpdef void update_balances(self, Account account, Instrument instrument, OrderFilled fill)
    cpdef bint update_orders(self, Account account, Instrument instrument, list orders_open, uint64_t ts_event)
    cpdef bint update_positions(self, MarginAccount account, Instrument instrument, list positions_open, uint64_t ts_event)
    cdef bint _update_balance_locked(self, CashAccount account, Instrument instrument, list orders_open, uint64_t ts_event)
    cdef bint _update_margin_init(self, MarginAccount account, Instrument instrument, list orders_open, uint64_t ts_event)
    cdef bint _update_balance_single_currency(self, Account account, OrderFilled fill, Money pnl)
    cdef bint _update_balance_multi_currency(self, Account account, OrderFilled fill, list pnls)
    cdef object _calculate_xrate_to_base(self, Account account, Instrument instrument, OrderSide side)
