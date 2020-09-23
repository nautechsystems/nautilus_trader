# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.correctness cimport Condition


cdef class IdentifierCache:
    """
    Provides an identifier cache.
    """

    def __init__(self):
        """
        Initialize a new instance of the IdentifierCache class.
        """
        self._cached_trader_ids = ObjectCache(TraderId, TraderId.from_string)
        self._cached_account_ids = ObjectCache(AccountId, AccountId.from_string)
        self._cached_strategy_ids = ObjectCache(StrategyId, StrategyId.from_string)
        self._cached_symbols = ObjectCache(Symbol, Symbol.from_string)

    cpdef TraderId get_trader_id(self, str value):
        """
        Return the cached trader identifier.

        Parameters
        ----------
        value : str
            The value to be parsed to a trader identifier.

        Returns
        -------
        TraderId or None

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        Condition.valid_string(value, "value")

        return self._cached_trader_ids.get(value)

    cpdef AccountId get_account_id(self, str value):
        """
        Return the cached account.

        Parameters
        ----------
        value : str
            The value to be parsed to an account identifier.

        Returns
        -------
        AccountId or None

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        Condition.valid_string(value, "value")

        return self._cached_account_ids.get(value)

    cpdef StrategyId get_strategy_id(self, str value):
        """
        Return the cached strategy identifier.

        value : str
            The value to be parsed to a strategy identifier.

        Returns
        -------
        StrategyId or None

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        Condition.valid_string(value, "value")

        return self._cached_strategy_ids.get(value)

    cpdef Symbol get_symbol(self, str value):
        """
        Return the cached symbol.

        Parameters
        ----------
        value : str
            The value to be parsed to a symbol.

        Returns
        -------
        Symbol or None

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        Condition.valid_string(value, "value")

        return self._cached_symbols.get(value)
