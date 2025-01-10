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

import msgspec

from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money


class OKXAssetInformationDetails(msgspec.Struct, dict=True):
    ccy: str
    eq: str  # equity of currency
    cashBal: str
    uTime: str  # update time of currency balance
    isoEq: str  # isolated margin equity of currency
    availEq: str  # available equity of currency
    disEq: str  # discount equity of currency in USD
    fixedBal: str  # Frozen balance for Dip Sniper and Peak Sniper
    availBal: str  # available balance of currency
    frozenBal: str  # frozen balance of currency
    ordFrozen: str  # frozen margin for open orders
    liab: str  # liabilities of currency
    upl: str  # sum of unrealized profit & loss of all margin and derivative positions of currency
    uplLiab: str  # liabilities due to unrealized loss of currency
    crossLiab: str  # cross liabilities of currency
    rewardBal: str  # trial fund balance
    isoLiab: str  # isolated liabilities of currency
    mgnRatio: str  # margin ratio of currency
    interest: str  # accrued interest of currency
    twap: str  # risk auto liability payment, 0-5 in increasing risk of auto payment trigger risk
    maxLoan: str  # max loan of currency
    eqUsd: str  # equity of currency in USD
    borrowFroz: str  # potential borrowing IMR of currency in USD
    notionalLever: str  # leverage of currency
    stgyEq: str  # strategy equity
    isoUpl: str  # isolated unrealized profit & loss of currency
    spotInUseAmt: str  # spot in use amount
    clSpotInUseAmt: str  # user-defined spot risk offset amount
    maxSpotInUse: str  # Max possible spot risk offset amount
    spotIsoBal: str  # spot isolated balance, applicable to copy trading, applicable to spot/futures
    imr: str  # initial margin requirement at the currency level, applicable to spot/futures
    mmr: str  # maintenance margin requirement at the currency level, applicable to spot/futures
    smtSyncEq: str  # smart sync equity, The default is "0", only applicable to copy trader

    def parse_to_account_balance(self) -> AccountBalance | None:
        if not self.eq or not self.availEq:
            return None

        currency = Currency.from_str(self.ccy)
        format_spec = f".{currency.precision}f"
        total = Decimal(format(float(self.eq), format_spec))
        free = Decimal(format(float(self.availEq), format_spec))
        locked = total - free

        return AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )

    def parse_to_margin_balance(self) -> MarginBalance | None:
        if not self.imr or not self.mmr:
            return None

        currency: Currency = Currency.from_str(self.ccy)
        format_spec = f".{currency.precision}f"
        imr = Decimal(format(float(self.imr), format_spec))
        mmr = Decimal(format(float(self.mmr), format_spec))

        return MarginBalance(
            initial=Money(imr, currency),
            maintenance=Money(mmr, currency),
        )


class OKXAccountBalanceData(msgspec.Struct):
    uTime: str  # update time of account information
    totalEq: str  # total USD
    isoEq: str  # isolated margin in USD
    adjEq: str  # net USD value of all assets contributing to margin rqts, discounted for mkt risk
    ordFroz: str  # cross-margin USD frozen for pending orders
    imr: str  # total USD init margins of all open positions & pending orders in cross-margin mode
    mmr: str  # total USD maint margins of all open positions & pending orders in cross-margin mode
    borrowFroz: str  # potential borrowing IMR in USD
    mgnRatio: str  # margin ratio in USD
    notionalUsd: str  # notional value of positions in USD
    upl: str  # cross-margin info of unrealized profit & loss at account level in USD
    details: list[OKXAssetInformationDetails]

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str("USD")
        format_spec = f".{currency.precision}f"
        total = Decimal(format(float(self.totalEq), format_spec))
        free = Decimal(format(float(self.adjEq), format_spec))

        locked = total - free

        return AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )

    def parse_to_margin_balance(self) -> MarginBalance:
        currency: Currency = Currency.from_str("USD")
        format_spec = f".{currency.precision}f"
        imr = Decimal(format(float(self.imr), format_spec))
        mmr = Decimal(format(float(self.mmr), format_spec))

        return MarginBalance(
            initial=Money(imr, currency),
            maintenance=Money(mmr, currency),
        )


class OKXAccountBalanceResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXAccountBalanceData]
