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
from typing import Literal, Optional

import msgspec
from ibapi.common import UNSET_DECIMAL

from nautilus_trader.config.common import NautilusConfig
from nautilus_trader.model.identifiers import Venue


IB_VENUE = Venue("InteractiveBrokers")


class ContractId(int):
    """
    ContractId type.
    """


# https://interactivebrokers.github.io/tws-api/tick_types.html
TickTypeMapping = {
    0: "Bid Size",
    1: "Bid Price",
    2: "Ask Price",
    3: "Ask Size",
    4: "Last Price",
    5: "Last Size",
    6: "High",
    7: "Low",
    8: "Volume",
    9: "Close Price",
}


class ComboLeg(NautilusConfig, omit_defaults=True):  # repr_omit_defaults=True
    """
    Class representing a leg within combo orders.
    """

    conId: int = 0
    ratio: int = 0
    action: str = ""  # Literal["BUY", "SELL"]
    exchange: str = ""
    openClose: int = 0  # LegOpenClose enum values
    # for stock legs when doing short sale
    shortSaleSlot: int = 0
    designatedLocation: str = ""
    exemptCode: int = -1


class DeltaNeutralContract(NautilusConfig, omit_defaults=True):  # repr_omit_defaults=True
    """
    Delta-Neutral Contract.
    """

    conId: int = 0
    delta: float = 0.0
    price: float = 0.0


class IBContract(NautilusConfig, omit_defaults=True):  # repr_omit_defaults=True
    """
    Class describing an instrument's definition with additional fields for options/futures.

    Parameters
    ----------
    secType: str
        Security Type of the contract i.e STK, OPT, FUT, CONTFUT
    exchange: str
        Exchange where security is traded. Will be SMART for Stocks.
    primaryExchange: str
        Exchange where security is registered. Applies to Stocks.
    localSymbol: str
        Unique Symbol registered in Exchange.
    build_options_chain: bool (default: None)
        Search for full option chain
    build_futures_chain: bool (default: None)
        Search for full futures chain
    min_expiry_days: int (default: None)
        Filters the options_chain and futures_chain which are expiring after number of days specified.
    max_expiry_days: int (default: None)
        Filters the options_chain and futures_chain which are expiring before number of days specified.
    lastTradeDateOrContractMonth: str (%Y%m%d or %Y%m) (default: '')
        Filters the options_chain and futures_chain specific for this expiry date
    """

    secType: Literal["CASH", "STK", "OPT", "FUT", "FOP", "CONTFUT", ""] = ""
    conId: int = 0
    exchange: str = ""
    primaryExchange: str = ""
    symbol: str = ""
    localSymbol: str = ""
    currency: str = ""
    tradingClass: str = ""

    # options and futures
    lastTradeDateOrContractMonth: str = ""
    multiplier: str = ""

    # options
    strike: float = 0.0
    right: str = ""

    # If set to true, contract details requests and historical data queries can be performed pertaining
    # to expired futures contracts. Expired options or other instrument types are not available.
    includeExpired: bool = False

    # common
    secIdType: str = ""
    secId: str = ""
    description: str = ""
    issuerId: str = ""

    # combos
    comboLegsDescrip: str = ""
    comboLegs: list[ComboLeg] = None
    deltaNeutralContract: Optional[DeltaNeutralContract] = None

    # nautilus specific parameters
    build_futures_chain: Optional[bool] = None
    build_options_chain: Optional[bool] = None
    min_expiry_days: Optional[int] = None
    max_expiry_days: Optional[int] = None

    def __repr__(self):  # Remove once repr_omit_defaults is available in msgspec next release
        kwargs = ", ".join(f"{k}={v!r}" for k, v in msgspec.json.decode(self.json()).items())
        return f"IBContract({kwargs})"


class IBOrderTags(NautilusConfig, omit_defaults=True):  # repr_omit_defaults=True
    """
    Used to attach to Nautilus Order Tags for IB specific order parameters.
    """

    # Pre-order and post-order Margin analysis with commission
    whatIf: bool = False

    # Order Group conditions (One)
    ocaGroup: str = ""  # one cancels all group name
    ocaType: int = 0  # 1 = CANCEL_WITH_BLOCK, 2 = REDUCE_WITH_BLOCK, 3 = REDUCE_NON_BLOCK

    # Order Group conditions (All)
    allOrNone: bool = False

    # Time conditions
    activeStartTime: str = ""  # for GTC orders, Format: "%Y%m%d %H:%M:%S %Z"
    activeStopTime: str = ""  # for GTC orders, Format: "%Y%m%d %H:%M:%S %Z"
    goodAfterTime: str = ""  # Format: "%Y%m%d %H:%M:%S %Z"

    # extended order fields
    blockOrder = False  # If set to true, specifies that the order is an ISE Block order.
    sweepToFill = False
    outsideRth: bool = False

    @property
    def value(self):
        return self.json().decode()

    def __str__(self):
        return self.value


class IBContractDetails(NautilusConfig, omit_defaults=True):  # repr_omit_defaults=True
    """
    ContractDetails class to be used internally in Nautilus for ease of encoding/decoding.
    """

    contract: IBContract = None
    marketName: str = ""
    minTick: float = 0
    orderTypes: str = ""
    validExchanges: str = ""
    priceMagnifier: float = 0
    underConId: int = 0
    longName: str = ""
    contractMonth: str = ""
    industry: str = ""
    category: str = ""
    subcategory: str = ""
    timeZoneId: str = ""
    tradingHours: str = ""
    liquidHours: str = ""
    evRule: str = ""
    evMultiplier: int = 0
    mdSizeMultiplier: int = 1  # obsolete
    aggGroup: int = 0
    underSymbol: str = ""
    underSecType: str = ""
    marketRuleIds: str = ""
    secIdList: Optional[list] = None
    realExpirationDate: str = ""
    lastTradeTime: str = ""
    stockType: str = ""
    minSize: Decimal = UNSET_DECIMAL
    sizeIncrement: Decimal = UNSET_DECIMAL
    suggestedSizeIncrement: Decimal = UNSET_DECIMAL

    # BOND values
    cusip: str = ""
    ratings: str = ""
    descAppend: str = ""
    bondType: str = ""
    couponType: str = ""
    callable: bool = False
    putable: bool = False
    coupon: int = 0
    convertible: bool = False
    maturity: str = ""
    issueDate: str = ""
    nextOptionDate: str = ""
    nextOptionType: str = ""
    nextOptionPartial: bool = False
    notes: str = ""
