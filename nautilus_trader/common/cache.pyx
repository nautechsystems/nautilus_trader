# -------------------------------------------------------------------------------------------------
# <copyright file="cache.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cdef class IdentifierCache:
    """
    Provides an identifier cache.
    """

    def __init__(self):
        """
        Initializes a new instance of the IdentifierCache class.
        """
        self._cached_trader_ids = ObjectCache(TraderId, TraderId.from_string)
        self._cached_account_ids = ObjectCache(AccountId, AccountId.from_string)
        self._cached_strategy_ids = ObjectCache(StrategyId, StrategyId.from_string)
        self._cached_symbols = ObjectCache(Symbol, Symbol.from_string)

    cpdef TraderId get_trader_id(self, str value):
        """
        Return the cached trader_id.
        
        :param value: The value to be parsed to a trader_id.
        :return: TraderId.
        """
        return self._cached_trader_ids.get(value)

    cpdef AccountId get_account_id(self, str value):
        """
        Return the cached account.
        
        :param value: The value to be parsed to a account_id.
        :return: AccountId.
        """
        return self._cached_account_ids.get(value)

    cpdef StrategyId get_strategy_id(self, str value):
        """
        Return the cached strategy_id.
        
        :param value: The value to be parsed to a strategy_id.
        :return: StrategyId.
        """
        return self._cached_strategy_ids.get(value)

    cpdef Symbol get_symbol(self, str value):
        """
        Return the cached symbol.
        
        :param value: The value to be parsed to a symbol.
        :return: Symbol.
        """
        return self._cached_symbols.get(value)
