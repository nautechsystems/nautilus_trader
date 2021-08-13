# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.accounting.base cimport Account
from nautilus_trader.accounting.cash cimport CashAccount
from nautilus_trader.accounting.margin cimport MarginAccount
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.account_type cimport AccountType


cdef class AccountFactory:
    """
    Provides a factory for creating different account types.
    """

    @staticmethod
    cdef Account create_c(AccountState event):
        Condition.not_none(event, "event")

        if event.account_type == AccountType.CASH:
            return CashAccount(event)
        elif event.account_type == AccountType.MARGIN:
            return MarginAccount(event)
        else:
            raise RuntimeError("invalid account type")

    @staticmethod
    def create(AccountState event) -> Account:
        """
        Create an account based on the events account type.

        Parameters
        ----------
        event : AccountState
            The account state event for the creation.

        Returns
        -------
        Account

        """
        return AccountFactory.create_c(event)
