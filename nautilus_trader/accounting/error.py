# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.objects import Currency


class AccountError(Exception):
    """
    The base class for all account type errors.
    """


class AccountBalanceNegative(AccountError):
    """
    Raised when the account balance for a currency becomes negative.
    """

    def __init__(self, balance: Decimal, currency: Currency):
        super().__init__()

        self.balance = balance
        self.currency = currency

    def __str__(self) -> str:
        return f"{type(self).__name__}(balance={self.balance}, currency={self.currency})"


class AccountMarginExceeded(AccountError):
    """
    Raised when the account margin for a currency is exceeded.

    In this scenario some form of liquidation event will occur.

    """

    def __init__(self, balance: Decimal, margin: Decimal, currency: Currency):
        super().__init__()

        self.balance = balance
        self.margin = margin
        self.currency = currency

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"balance={self.balance}, "
            f"margin={self.margin}, "
            f"free={self.balance - self.margin}, "
            f"currency={self.currency})"
        )
