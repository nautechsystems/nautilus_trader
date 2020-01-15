# -------------------------------------------------------------------------------------------------
# <copyright file="account.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.types cimport ValidString
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.model.identifiers cimport Brokerage, AccountNumber, AccountId
from nautilus_trader.model.objects cimport Decimal, Money


cdef class Account:
    cdef list _events
    cdef readonly AccountStateEvent last_event
    cdef readonly int event_count

    cdef readonly AccountId id
    cdef readonly Brokerage broker
    cdef readonly AccountNumber account_number
    cdef readonly AccountType account_type
    cdef readonly Currency currency
    cdef readonly Money cash_balance
    cdef readonly Money cash_start_day
    cdef readonly Money cash_activity_day
    cdef readonly Money margin_used_liquidation
    cdef readonly Money margin_used_maintenance
    cdef readonly Decimal margin_ratio
    cdef readonly ValidString margin_call_status
    cdef readonly free_equity

    cdef readonly datetime last_updated

    cpdef list get_events(self)
    cpdef void apply(self, AccountStateEvent event) except *

    cdef Money _calculate_free_equity(self)


cdef class NullAccount(Account):
    pass
