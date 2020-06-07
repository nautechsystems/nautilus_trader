# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.model.identifiers cimport TraderId, AccountId, StrategyId, Symbol


cdef class IdentifierCache:
    cdef ObjectCache _cached_trader_ids
    cdef ObjectCache _cached_account_ids
    cdef ObjectCache _cached_strategy_ids
    cdef ObjectCache _cached_symbols

    cpdef TraderId get_trader_id(self, str value)
    cpdef AccountId get_account_id(self, str value)
    cpdef StrategyId get_strategy_id(self, str value)
    cpdef Symbol get_symbol(self, str value)
