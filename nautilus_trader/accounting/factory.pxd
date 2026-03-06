from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.model.events.account cimport AccountState


cdef class AccountFactory:

    @staticmethod
    cdef Account create_c(AccountState event)
