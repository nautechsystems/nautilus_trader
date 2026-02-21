# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Final
from typing import Literal

import msgspec.structs
from ibapi.const import UNSET_DECIMAL
from ibapi.const import UNSET_DOUBLE
from ibapi.contract import FundAssetType
from ibapi.contract import FundDistributionPolicyIndicator
from ibapi.tag_value import TagValue

from nautilus_trader.config import NautilusConfig
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


IB: Final[str] = "INTERACTIVE_BROKERS"
IB_VENUE: Final[Venue] = Venue(IB)
IB_CLIENT_ID: Final[ClientId] = ClientId(IB)


class ContractId(int):
    """
    ContractId type.
    """


class ComboLeg(NautilusConfig, frozen=True, omit_defaults=True, repr_omit_defaults=True):
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


class DeltaNeutralContract(NautilusConfig, frozen=True, repr_omit_defaults=True):
    """
    Delta-Neutral Contract.
    """

    conId: int = 0
    delta: float = 0.0
    price: float = 0.0


class IBContract(NautilusConfig, frozen=True, repr_omit_defaults=True):
    """
    Class describing an instrument's definition with additional fields for
    options/futures.

    Parameters
    ----------
    secType: str
        Security Type of the contract i.e STK, OPT, FUT, CONTFUT
    exchange: str
        Exchange where security is traded. Will be SMART for Stocks.
    primaryExchange: str
        Exchange where security is registered. Applies to Stocks.
    symbol: str
        Unique Symbol registered in Exchange.
    build_options_chain: bool (default: None)
        Search for full option chain
    build_futures_chain: bool (default: None)
        Search for full futures chain
    options_chain_exchange: str (default : None)
        optional exchange for options chain, in place of underlying exchange
    min_expiry_days: int (default: None)
        Filters the options_chain and futures_chain which are expiring after number of days specified.
    max_expiry_days: int (default: None)
        Filters the options_chain and futures_chain which are expiring before number of days specified.
    lastTradeDateOrContractMonth: str (%Y%m%d or %Y%m) (default: '')
        Filters the options_chain and futures_chain specific for this expiry date
    lastTradeDate: str (default: '')
        The contract last trading day.

    """

    secType: Literal[
        "CASH",
        "STK",
        "OPT",
        "FUT",
        "FOP",
        "CONTFUT",
        "CRYPTO",
        "CFD",
        "CMDTY",
        "IND",
        "BAG",
        "",
    ] = ""
    conId: int = 0
    exchange: str = ""
    primaryExchange: str = ""
    symbol: str = ""
    localSymbol: str = ""
    currency: str = ""
    tradingClass: str = ""

    # options and futures
    lastTradeDateOrContractMonth: str = ""
    lastTradeDate: str = ""
    multiplier: str = ""

    # options
    strike: float = UNSET_DOUBLE
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
    comboLegs: list[ComboLeg] | None = None
    deltaNeutralContract: DeltaNeutralContract | None = None

    # nautilus specific parameters
    build_futures_chain: bool | None = None
    build_options_chain: bool | None = None
    options_chain_exchange: str | None = None
    min_expiry_days: int | None = None
    max_expiry_days: int | None = None

    def __post_init__(self):
        # ibapi 10.43 introduced a dual protocol: legacy string-based and protobuf.
        # TWS/Gateway now responds in protobuf format regardless of request format.
        # The protobuf decoder decodes unset double fields as 0.0 rather than
        # UNSET_DOUBLE (sys.float_info.max). When strike=0.0 leaks into a subsequent
        # reqContractDetails call, IB silently ignores the request (zero bytes back).
        # We normalize here in __post_init__ so that all IBContract construction paths
        # (direct `IBContract(**contract.__dict__)` and deserialization) are safe.
        if self.strike == 0.0:
            msgspec.structs.force_setattr(self, "strike", UNSET_DOUBLE)


class IBOrderTags(NautilusConfig, frozen=True, repr_omit_defaults=True):
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

    # If set to true, the order will not be visible when viewing the market depth.
    # This option only applies to orders routed to the NASDAQ exchange.
    hidden: bool = False

    # Order conditions
    conditions: list[dict] = []  # List of condition dictionaries
    conditionsCancelOrder: bool = (
        False  # True = cancel order when condition met, False = transmit order
    )

    # Smart combo routing parameters (for combo orders)
    NonGuaranteed: bool = False  # True = non-guaranteed combo order, False = guaranteed combo order

    @property
    def value(self):
        return f"IBOrderTags:{self.json().decode()}"

    def __str__(self):
        return self.value


class IBContractDetails(NautilusConfig, frozen=True, repr_omit_defaults=True):
    """
    ContractDetails class to be used internally in Nautilus for ease of
    encoding/decoding.

    Reference: https://ibkrcampus.com/campus/ibkr-api-page/twsapi-ref/#contract-pub-func

    """

    contract: IBContract | None = None
    marketName: str = ""
    minTick: float = 0
    orderTypes: str = ""
    validExchanges: str = ""
    priceMagnifier: int = 1
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
    evMultiplier: float = 0
    mdSizeMultiplier: int = 1  # obsolete
    aggGroup: int = 0
    underSymbol: str = ""
    underSecType: str = ""
    marketRuleIds: str = ""
    secIdList: list[TagValue] | None = None
    realExpirationDate: str = ""
    lastTradeTime: str = ""
    stockType: str = ""
    minSize: Decimal = UNSET_DECIMAL
    sizeIncrement: Decimal = UNSET_DECIMAL
    suggestedSizeIncrement: Decimal = UNSET_DECIMAL
    minAlgoSize: Decimal = UNSET_DECIMAL

    # BOND values
    cusip: str = ""
    ratings: str = ""
    descAppend: str = ""
    bondType: str = ""
    couponType: str = ""
    callable: bool = False
    putable: bool = False
    coupon: float = 0
    convertible: bool = False
    maturity: str = ""
    issueDate: str = ""
    nextOptionDate: str = ""
    nextOptionType: str = ""
    nextOptionPartial: bool = False
    notes: str = ""

    # FUND values
    fundName: str = ""
    fundFamily: str = ""
    fundType: str = ""
    fundFrontLoad: str = ""
    fundBackLoad: str = ""
    fundBackLoadTimeInterval: str = ""
    fundManagementFee: str = ""
    fundClosed: bool = False
    fundClosedForNewInvestors: bool = False
    fundClosedForNewMoney: bool = False
    fundNotifyAmount: str = ""
    fundMinimumInitialPurchase: str = ""
    fundSubsequentMinimumPurchase: str = ""
    fundBlueSkyStates: str = ""
    fundBlueSkyTerritories: str = ""
    fundDistributionPolicyIndicator: FundDistributionPolicyIndicator = (
        FundDistributionPolicyIndicator.NoneItem
    )
    fundAssetType: FundAssetType = FundAssetType.NoneItem
    ineligibilityReasonList: list = None
    eventContract1: str = ""
    eventContractDescription1: str = ""
    eventContractDescription2: str = ""

    @classmethod
    def from_contract_details(cls, contract_details) -> "IBContractDetails":
        """
        Create from a raw ibapi ContractDetails, normalizing ibapi 10.43 protobuf
        issues.

        ibapi 10.43's protobuf decoder (decoder.py:decodeContractDetails) has a typo:
        it writes ``contractDetails.underConid`` (lowercase 'i') instead of the
        canonical ``contractDetails.underConId`` (uppercase 'I'). This causes
        ``IBContractDetails(**details.__dict__)`` to fail with "Unexpected keyword
        argument 'underConid'". We normalize the key here before construction.

        Unlike IBContract.strike (where 0.0 is a valid kwarg we fix in __post_init__),
        "underConid" is an invalid kwarg that must be renamed in the dict before
        the constructor sees it — hence this is a classmethod rather than __post_init__.

        """
        contract_details.contract = IBContract(**contract_details.contract.__dict__)
        d = contract_details.__dict__.copy()
        if "underConid" in d:
            if not d.get("underConId"):
                d["underConId"] = d.pop("underConid")
            else:
                d.pop("underConid")
        return cls(**d)


def dict_to_contract_details(dict_details: dict) -> IBContractDetails:
    details_copy = dict_details.copy()

    if "contract" in details_copy and isinstance(details_copy["contract"], dict):
        details_copy["contract"] = IBContract(**details_copy["contract"])

    if details_copy.get("secIdList") and isinstance(details_copy["secIdList"], dict):
        tag_values = [
            TagValue(tag=tag, value=value) for tag, value in details_copy["secIdList"].items()
        ]
        details_copy["secIdList"] = tag_values

    # Deserialize Decimal fields from strings back to Decimal objects
    # These fields are known to be Decimal type in IBContractDetails
    decimal_fields = ["minSize", "sizeIncrement", "suggestedSizeIncrement", "minAlgoSize"]
    for field in decimal_fields:
        if field in details_copy and isinstance(details_copy[field], str):
            try:
                decimal_value = Decimal(details_copy[field])

                # Check if this is the UNSET_DECIMAL value
                if decimal_value == UNSET_DECIMAL:
                    details_copy[field] = UNSET_DECIMAL
                else:
                    details_copy[field] = decimal_value
            except (ValueError, TypeError):
                # If conversion fails, keep the original value
                pass

    # Deserialize Enum fields from their values back to Enum members
    # These fields are known to be Enum type in IBContractDetails
    if "fundDistributionPolicyIndicator" in details_copy:
        details_copy["fundDistributionPolicyIndicator"] = _deserialize_enum_from_value(
            FundDistributionPolicyIndicator,
            details_copy["fundDistributionPolicyIndicator"],
        )

    if "fundAssetType" in details_copy:
        details_copy["fundAssetType"] = _deserialize_enum_from_value(
            FundAssetType,
            details_copy["fundAssetType"],
        )

    return IBContractDetails(**details_copy)


def _deserialize_enum_from_value(enum_class, value):
    """
    Convert an enum value (tuple or string) back to the enum member.
    """
    if value is None:
        return None

    # If already an enum member, return as-is
    if isinstance(value, enum_class):
        return value

    # Try to find enum member by matching value
    for member in enum_class:
        if member.value == value:
            return member

    # If not found, return the original value (might be invalid)
    return value
