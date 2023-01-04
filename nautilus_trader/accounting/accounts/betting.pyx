# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

from nautilus_trader.accounting.accounts.cash cimport CashAccount
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.data.bet cimport Bet
from nautilus_trader.model.data.bet cimport nautilus_to_bet
from nautilus_trader.model.enums_c cimport AccountType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BettingAccount(CashAccount):
    """
    Provides a betting account.
    """
    ACCOUNT_TYPE = AccountType.BETTING

# -- CALCULATIONS ---------------------------------------------------------------------------------

    cpdef Money calculate_balance_locked(
        self,
        Instrument instrument,
        OrderSide side,
        Quantity quantity,
        Price price,
        bint inverse_as_quote=False,
    ):
        """
        Calculate the locked balance.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        side : OrderSide {``BUY``, ``SELL``}
            The order side.
        quantity : Quantity
            The order quantity.
        price : Price
            The order price.
        inverse_as_quote : bool
            Not applicable for betting accounts.

        Returns
        -------
        Money

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")
        Condition.not_equal(inverse_as_quote, True, "inverse_as_quote", "True")

        cdef Bet bet = nautilus_to_bet(
            price=price,
            quantity=quantity,
            side=side
        )
        locked: Decimal = bet.liability()
        return Money(locked, instrument.quote_currency)
