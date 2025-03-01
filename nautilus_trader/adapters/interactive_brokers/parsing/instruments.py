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

import datetime
import re
import time
from decimal import Decimal
from typing import Final

import pandas as pd
from ibapi.contract import ContractDetails

# fmt: off
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails
from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import asset_class_from_str
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Cfd
from nautilus_trader.model.instruments import Commodity
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import IndexInstrument
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


# fmt: on

VENUE_MEMBERS: Final[dict[str, list[str]]] = {
    # CME Group Exchanges
    "GLBX": ["CME", "CBOT", "NYMEX", "NYBOT"],
    # ICE Europe Exchanges
    "IFEU": ["ICEEU", "ICEEUSOFT", "IPE"],
    # ICE Endex
    "NDEX": ["ENDEX"],
    # Chicago Mercantile Exchange Segments
    "XCME": ["CME"],
    "XCEC": ["CME"],
    "XFXS": ["CME"],
    # Chicago Board of Trade Segments
    "XCBT": ["CBOT"],
    "CBCM": ["CBOT"],
    # New York Mercantile Exchange Segments
    "XNYM": ["NYMEX"],
    "NYUM": ["NYMEX"],
    # ICE Futures US (formerly NYBOT)
    "IFUS": ["NYBOT"],
    # US Major Exchanges
    "XNAS": ["NASDAQ"],
    "XNYS": ["NYSE"],
    "ARCX": ["ARCA"],
    "BATS": ["BATS"],
    "IEXG": ["IEX"],
    # European Exchanges
    "XLON": ["LSE"],  # London Stock Exchange
    "XPAR": ["SBF"],  # Euronext Paris
    "XETR": ["IBIS"],  # Deutsche Börse
    # Canadian Exchanges
    "XTSE": ["TSE"],  # Toronto Stock Exchange
    "XTSX": ["VENTURE"],  # TSX Venture Exchange
    # Asia-Pacific Exchanges
    "XASX": ["ASX"],  # Australian Securities Exchange
    "XHKF": ["HKFE"],  # Hong Kong Futures Exchange
    "XSES": ["SGX"],  # Singapore Exchange
    "XOSE": ["OSE.JPN"],  # Osaka Securities Exchange
    # Other Derivatives Exchanges
    "XEUR": ["SOFFEX"],  # Eurex
    "XSFE": ["SNFE"],  # Sydney Futures Exchange
    "XMEX": ["MEXDER"],  # Mexican Derivatives Exchange
}

FUTURES_MONTH_TO_CODE: dict[str, str] = {
    "JAN": "F",
    "FEB": "G",
    "MAR": "H",
    "APR": "J",
    "MAY": "K",
    "JUN": "M",
    "JUL": "N",
    "AUG": "Q",
    "SEP": "U",
    "OCT": "V",
    "NOV": "X",
    "DEC": "Z",
}
FUTURES_CODE_TO_MONTH = dict(
    zip(FUTURES_MONTH_TO_CODE.values(), FUTURES_MONTH_TO_CODE.keys(), strict=False),
)

VENUES_CASH = ["IDEALPRO"]
VENUES_CRYPTO = ["PAXOS"]
VENUES_OPT = ["SMART"]
VENUES_FUT = [
    "CBOT",  # US
    "CME",  # US
    "COMEX",  # US
    "KCBT",  # US
    "MGE",  # US
    "NYMEX",  # US
    "NYBOT",  # US
    "SNFE",  # AU
]
VENUES_CFD = [
    "IBCFD",  # self named, in fact mapping to "SMART" when parsing
]
VENUES_CMDTY = ["IBCMDTY"]  # self named, in fact mapping to "SMART" when parsing

RE_CASH = re.compile(r"^(?P<symbol>[A-Z]{3})\/(?P<currency>[A-Z]{3})$")  # "EUR/USD"
RE_CFD_CASH = re.compile(r"^(?P<symbol>[A-Z]{3})\.(?P<currency>[A-Z]{3})$")  # "EUR.USD"
RE_OPT = re.compile(
    r"^(?P<symbol>^[A-Z. ]{1,6})(?P<expiry>\d{6})(?P<right>[CP])(?P<strike>\d{5})(?P<decimal>\d{3})$",
)  # "AAPL220617C00155000"
RE_FUT_UNDERLYING = re.compile(r"^(?P<symbol>\w{1,3})$")  # "ES"
RE_FUT = re.compile(r"^(?P<symbol>\w{1,3})(?P<month>[FGHJKMNQUVXZ])(?P<year>\d{2})$")  # "ESM23"
RE_FUT_ORIGINAL = re.compile(
    r"^(?P<symbol>\w{1,3})(?P<month>[FGHJKMNQUVXZ])(?P<year>\d)$",
)  # "ESM3"
RE_FUT2 = re.compile(
    r"^(?P<symbol>\w{1,4})(?P<month>(JAN|FEB|MAR|APR|MAY|JUN|JUL|AUG|SEP|OCT|NOV|DEC))(?P<year>\d{2})$",
)  # "ESMAR23"
RE_FUT2_ORIGINAL = re.compile(
    r"^(?P<symbol>\w{1,4}) *(?P<month>(JAN|FEB|MAR|APR|MAY|JUN|JUL|AUG|SEP|OCT|NOV|DEC)) (?P<year>\d{2})$",
)  # "ES MAR 23"
RE_FUT3_ORIGINAL = re.compile(
    r"^(?P<symbol>[A-Z]+)(?P<year>\d{2})(?P<month>(JAN|FEB|MAR|APR|MAY|JUN|JUL|AUG|SEP|OCT|NOV|DEC))FUT$",
)  # "NIFTY25MARFUT"
RE_FOP = re.compile(
    r"^(?P<symbol>\w{1,3})(?P<month>[FGHJKMNQUVXZ])(?P<year>\d{2})(?P<right>[CP])(?P<strike>.{4,5})$",
)  # "ESM23C4200"
RE_FOP_ORIGINAL = re.compile(
    r"^(?P<symbol>\w{1,3})(?P<month>[FGHJKMNQUVXZ])(?P<year>\d)\s(?P<right>[CP])(?P<strike>\d{1,4}(?:\.\d)?)$",
)  # "ESM3 C4420"
RE_CRYPTO = re.compile(r"^(?P<symbol>[A-Z]*)\/(?P<currency>[A-Z]{3})$")  # "BTC/USD"


def _extract_isin(details: IBContractDetails) -> int:
    if details.secIdList:
        for tag_value in details.secIdList:
            if tag_value.tag == "ISIN":
                return tag_value.value
    raise ValueError("No ISIN found")


def _tick_size_to_precision(tick_size: float | Decimal) -> int:
    tick_size_str = f"{tick_size:.10f}"
    return len(tick_size_str.partition(".")[2].rstrip("0"))


def sec_type_to_asset_class(sec_type: str) -> AssetClass:
    mapping = {
        "STK": "EQUITY",
        "IND": "INDEX",
        "CASH": "FX",
        "BOND": "DEBT",
        "CMDTY": "COMMODITY",
        "FUT": "INDEX",
    }
    return asset_class_from_str(mapping.get(sec_type, sec_type))


def contract_details_to_ib_contract_details(details: ContractDetails) -> IBContractDetails:
    details.contract = IBContract(**details.contract.__dict__)
    details = IBContractDetails(**details.__dict__)
    return details


def parse_instrument(
    contract_details: IBContractDetails,
    symbology_method: SymbologyMethod = SymbologyMethod.IB_SIMPLIFIED,
    databento_venue: str | None = None,
) -> Instrument:
    security_type = contract_details.contract.secType
    instrument_id = ib_contract_to_instrument_id(
        contract=contract_details.contract,
        symbology_method=symbology_method,
        databento_venue=databento_venue,
    )
    if security_type == "STK":
        return parse_equity_contract(details=contract_details, instrument_id=instrument_id)
    elif security_type == "IND":
        return parse_index_contract(details=contract_details, instrument_id=instrument_id)
    elif security_type in ("FUT", "CONTFUT"):
        return parse_futures_contract(details=contract_details, instrument_id=instrument_id)
    elif security_type in ("OPT", "FOP"):
        return parse_option_contract(details=contract_details, instrument_id=instrument_id)
    elif security_type == "CASH":
        return parse_forex_contract(details=contract_details, instrument_id=instrument_id)
    elif security_type == "CRYPTO":
        return parse_crypto_contract(details=contract_details, instrument_id=instrument_id)
    elif security_type == "CFD":
        return parse_cfd_contract(details=contract_details, instrument_id=instrument_id)
    elif security_type == "CMDTY":
        return parse_commodity_contract(details=contract_details, instrument_id=instrument_id)
    else:
        raise ValueError(f"Unknown {security_type=}")


def contract_details_to_dict(details: IBContractDetails) -> dict:
    dict_details = details.dict().copy()
    dict_details["contract"] = details.contract.dict().copy()
    if dict_details.get("secIdList"):
        dict_details["secIdList"] = {
            tag_value.tag: tag_value.value for tag_value in dict_details["secIdList"]
        }
    return dict_details


def parse_equity_contract(
    details: IBContractDetails,
    instrument_id: InstrumentId,
) -> Equity:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()

    return Equity(
        instrument_id=instrument_id,
        raw_symbol=Symbol(details.contract.localSymbol),
        currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        price_increment=Price(details.minTick, price_precision),
        lot_size=Quantity.from_int(100),
        isin=_extract_isin(details),
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def parse_index_contract(
    details: IBContractDetails,
    instrument_id: InstrumentId,
) -> IndexInstrument:
    price_precision: int = _tick_size_to_precision(details.minTick)
    size_precision: int = _tick_size_to_precision(details.minSize)
    timestamp = time.time_ns()

    return IndexInstrument(
        instrument_id=instrument_id,
        raw_symbol=Symbol(details.contract.localSymbol),
        currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        price_increment=Price(details.minTick, price_precision),
        size_precision=size_precision,
        size_increment=Quantity(details.sizeIncrement, size_precision),
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def expiry_timestring_to_datetime(expiry: str) -> pd.Timestamp:
    """
    Most contract expirations are %Y%m%d format some exchanges have expirations in
    %Y%m%d %H:%M:%S %Z.
    """
    if len(expiry) == 8:
        return pd.Timestamp(expiry, tz="UTC")
    else:
        dt, tz = expiry.rsplit(" ", 1)
        ts = pd.Timestamp(dt, tz=tz)
        return ts.tz_convert("UTC")


def parse_futures_contract(
    details: IBContractDetails,
    instrument_id: InstrumentId,
) -> FuturesContract:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    expiration = expiry_timestring_to_datetime(details.contract.lastTradeDateOrContractMonth)
    activation = expiration - pd.Timedelta(days=90)  # TODO: Make this more accurate

    return FuturesContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol(details.contract.localSymbol),
        asset_class=sec_type_to_asset_class(details.underSecType),
        currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        price_increment=Price(details.minTick, price_precision),
        multiplier=Quantity.from_str(details.contract.multiplier),
        lot_size=Quantity.from_int(1),
        underlying=details.underSymbol,
        activation_ns=activation.value,
        expiration_ns=expiration.value,
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def parse_option_contract(
    details: IBContractDetails,
    instrument_id: InstrumentId,
) -> OptionContract:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    asset_class = sec_type_to_asset_class(details.underSecType)
    option_kind = {
        "C": OptionKind.CALL,
        "P": OptionKind.PUT,
    }[details.contract.right]
    expiration = expiry_timestring_to_datetime(details.contract.lastTradeDateOrContractMonth)
    activation = expiration - pd.Timedelta(days=90)  # TODO: Make this more accurate

    return OptionContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol(details.contract.localSymbol),
        asset_class=asset_class,
        currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        price_increment=Price(details.minTick, price_precision),
        multiplier=Quantity.from_str(details.contract.multiplier),
        lot_size=Quantity.from_int(1),
        underlying=details.underSymbol,
        strike_price=Price(details.contract.strike, price_precision),
        activation_ns=activation.value,
        expiration_ns=expiration.value,
        option_kind=option_kind,
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def parse_forex_contract(
    details: IBContractDetails,
    instrument_id: InstrumentId,
) -> CurrencyPair:
    price_precision: int = _tick_size_to_precision(details.minTick)
    size_precision: int = _tick_size_to_precision(details.minSize)
    timestamp = time.time_ns()

    return CurrencyPair(
        instrument_id=instrument_id,
        raw_symbol=Symbol(details.contract.localSymbol),
        base_currency=Currency.from_str(details.contract.symbol),
        quote_currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        size_precision=size_precision,
        price_increment=Price(details.minTick, price_precision),
        size_increment=Quantity(details.sizeIncrement, size_precision),
        lot_size=None,
        max_quantity=None,
        min_quantity=None,
        max_notional=None,
        min_notional=None,
        max_price=None,
        min_price=None,
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal(0),
        taker_fee=Decimal(0),
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def parse_crypto_contract(
    details: IBContractDetails,
    instrument_id: InstrumentId,
) -> CryptoPerpetual:
    price_precision: int = _tick_size_to_precision(details.minTick)
    size_precision: int = _tick_size_to_precision(details.minSize)
    timestamp = time.time_ns()

    return CryptoPerpetual(
        instrument_id=instrument_id,
        raw_symbol=Symbol(details.contract.localSymbol),
        base_currency=Currency.from_str(details.contract.symbol),
        quote_currency=Currency.from_str(details.contract.currency),
        settlement_currency=Currency.from_str(details.contract.currency),
        is_inverse=True,
        price_precision=price_precision,
        size_precision=size_precision,
        price_increment=Price(details.minTick, price_precision),
        size_increment=Quantity(details.sizeIncrement, size_precision),
        max_quantity=None,
        min_quantity=Quantity(details.minSize, size_precision),
        max_notional=None,
        min_notional=None,
        max_price=None,
        min_price=None,
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal(0),
        taker_fee=Decimal(0),
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def parse_cfd_contract(
    details: IBContractDetails,
    instrument_id: InstrumentId,
) -> Cfd:
    price_precision: int = _tick_size_to_precision(details.minTick)
    size_precision: int = _tick_size_to_precision(details.minSize)
    timestamp = time.time_ns()
    if RE_CFD_CASH.match(details.contract.localSymbol):
        return Cfd(
            instrument_id=instrument_id,
            raw_symbol=Symbol(details.contract.localSymbol),
            asset_class=sec_type_to_asset_class(details.underSecType),
            base_currency=Currency.from_str(details.contract.symbol),
            quote_currency=Currency.from_str(details.contract.currency),
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=Price(details.minTick, price_precision),
            size_increment=Quantity(details.sizeIncrement, size_precision),
            lot_size=None,
            max_quantity=None,
            min_quantity=None,
            max_notional=None,
            min_notional=None,
            max_price=None,
            min_price=None,
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=Decimal(0),
            taker_fee=Decimal(0),
            ts_event=timestamp,
            ts_init=timestamp,
            info=contract_details_to_dict(details),
        )
    else:
        return Cfd(
            instrument_id=instrument_id,
            raw_symbol=Symbol(details.contract.localSymbol),
            asset_class=sec_type_to_asset_class(details.underSecType),
            quote_currency=Currency.from_str(details.contract.currency),
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=Price(details.minTick, price_precision),
            size_increment=Quantity(details.sizeIncrement, size_precision),
            lot_size=None,
            max_quantity=None,
            min_quantity=None,
            max_notional=None,
            min_notional=None,
            max_price=None,
            min_price=None,
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=Decimal(0),
            taker_fee=Decimal(0),
            ts_event=timestamp,
            ts_init=timestamp,
            info=contract_details_to_dict(details),
        )


def parse_commodity_contract(
    details: IBContractDetails,
    instrument_id: InstrumentId,
) -> Commodity:
    price_precision: int = _tick_size_to_precision(details.minTick)
    size_precision: int = _tick_size_to_precision(details.minSize)
    timestamp = time.time_ns()
    return Commodity(
        instrument_id=instrument_id,
        raw_symbol=Symbol(details.contract.localSymbol),
        asset_class=AssetClass.COMMODITY,
        quote_currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        size_precision=size_precision,
        price_increment=Price(details.minTick, price_precision),
        size_increment=Quantity(details.sizeIncrement, size_precision),
        lot_size=None,
        max_quantity=None,
        min_quantity=None,
        max_notional=None,
        min_notional=None,
        max_price=None,
        min_price=None,
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal(0),
        taker_fee=Decimal(0),
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def decade_digit(last_digit: str, contract: IBContract) -> int:
    if year := contract.lastTradeDateOrContractMonth[:4]:
        return int(year[2:3])
    elif int(last_digit) > int(repr(datetime.datetime.now().year)[-1]):
        return int(repr(datetime.datetime.now().year)[-2]) - 1
    else:
        return int(repr(datetime.datetime.now().year)[-2])


def ib_contract_to_instrument_id(
    contract: IBContract,
    symbology_method: SymbologyMethod = SymbologyMethod.IB_SIMPLIFIED,
    databento_venue: str | None = None,
) -> InstrumentId:
    PyCondition.type(contract, IBContract, "IBContract")

    if symbology_method == SymbologyMethod.DATABENTO:
        assert databento_venue is not None
        return InstrumentId.from_str(f"{contract.localSymbol}.{databento_venue}")
    elif symbology_method == SymbologyMethod.IB_SIMPLIFIED:
        return ib_contract_to_instrument_id_simplified_symbology(contract)
    elif symbology_method == SymbologyMethod.IB_RAW:
        return ib_contract_to_instrument_id_raw_symbology(contract)
    else:
        raise NotImplementedError(f"{symbology_method} not implemented")


def ib_contract_to_instrument_id_raw_symbology(contract: IBContract) -> InstrumentId:
    if contract.secType == "CFD":
        symbol = f"{contract.localSymbol}={contract.secType}"
        venue = "IBCFD"
    elif contract.secType == "CMDTY":
        symbol = f"{contract.localSymbol}={contract.secType}"
        venue = "IBCMDTY"
    else:
        symbol = f"{contract.localSymbol}={contract.secType}"
        venue = (contract.primaryExchange or contract.exchange).replace(".", "/")
    return InstrumentId.from_str(f"{symbol}.{venue}")


def ib_contract_to_instrument_id_simplified_symbology(  # noqa: C901 (too complex)
    contract: IBContract,
) -> InstrumentId:
    security_type = contract.secType
    if security_type == "STK":
        symbol = (contract.localSymbol or contract.symbol).replace(" ", "-")
        venue = contract.primaryExchange if contract.exchange == "SMART" else contract.exchange
    elif security_type == "IND":
        symbol = f"^{(contract.localSymbol or contract.symbol)}"
        venue = contract.exchange
    elif security_type == "OPT" or security_type == "CONTFUT":
        symbol = contract.localSymbol.replace(" ", "") or contract.symbol.replace(" ", "")
        venue = contract.exchange
    elif security_type == "FUT" and (m := RE_FUT_ORIGINAL.match(contract.localSymbol)):
        symbol = f"{m['symbol']}{m['month']}{decade_digit(m['year'], contract)}{m['year']}"
        venue = contract.exchange
    elif security_type == "FUT" and (m := RE_FUT2_ORIGINAL.match(contract.localSymbol)):
        symbol = f"{m['symbol']}{FUTURES_MONTH_TO_CODE[m['month']]}{m['year']}"
        venue = contract.exchange
    elif security_type == "FUT" and (m := RE_FUT3_ORIGINAL.match(contract.localSymbol)):
        symbol = f"{m['symbol']}{FUTURES_MONTH_TO_CODE[m['month']]}{m['year']}"
        venue = contract.exchange
    elif security_type == "FOP" and (m := RE_FOP_ORIGINAL.match(contract.localSymbol)):
        symbol = f"{m['symbol']}{m['month']}{decade_digit(m['year'], contract)}{m['year']}{m['right']}{m['strike']}"
        venue = contract.exchange
    elif security_type in ["CASH", "CRYPTO"]:
        symbol = (
            f"{contract.localSymbol}".replace(".", "/") or f"{contract.symbol}/{contract.currency}"
        )
        venue = contract.exchange
    elif security_type == "CFD":
        if m := RE_CFD_CASH.match(contract.localSymbol):
            symbol = (
                f"{contract.localSymbol}".replace(".", "/")
                or f"{contract.symbol}/{contract.currency}"
            )
            venue = "IBCFD"
        else:
            symbol = (contract.symbol).replace(" ", "-")
            venue = "IBCFD"
    elif security_type == "CMDTY":
        symbol = (contract.symbol).replace(" ", "-")
        venue = "IBCMDTY"
    else:
        symbol = None
        venue = None
    if symbol and venue:
        return InstrumentId(Symbol(symbol), Venue(venue))
    raise ValueError(f"Unknown {contract=}")


def instrument_id_to_ib_contract(
    instrument_id: InstrumentId,
    symbology_method: SymbologyMethod = SymbologyMethod.IB_SIMPLIFIED,
    exchange: str | None = None,
) -> IBContract:
    PyCondition.type(instrument_id, InstrumentId, "InstrumentId")

    if symbology_method == SymbologyMethod.DATABENTO:
        return instrument_id_to_ib_contract_databento_symbology(
            instrument_id,
            exchange=exchange or "SMART",
        )
    elif symbology_method == SymbologyMethod.IB_SIMPLIFIED:
        return instrument_id_to_ib_contract_simplified_symbology(instrument_id)
    elif symbology_method == SymbologyMethod.IB_RAW:
        return instrument_id_to_ib_contract_raw_symbology(instrument_id)
    else:
        raise NotImplementedError(f"{symbology_method} not implemented")


def instrument_id_to_ib_contract_raw_symbology(instrument_id: InstrumentId) -> IBContract:
    local_symbol, security_type = instrument_id.symbol.value.rsplit("=", 1)
    exchange = instrument_id.venue.value.replace("/", ".")
    if security_type == "STK":
        return IBContract(
            secType=security_type,
            exchange="SMART",
            primaryExchange=exchange,
            localSymbol=local_symbol,
        )
    elif security_type == "CFD":
        return IBContract(
            secType=security_type,
            exchange="SMART",
            localSymbol=local_symbol,  # by IB is a cfd's local symbol of STK with a "n" as tail, e.g. "NVDAn". "
        )
    elif security_type == "CMDTY":
        return IBContract(
            secType=security_type,
            exchange="SMART",
            localSymbol=local_symbol,
        )
    elif security_type == "IND":
        return IBContract(
            secType=security_type,
            exchange=exchange,
            localSymbol=local_symbol,
        )
    else:
        return IBContract(
            secType=security_type,
            exchange=exchange,
            localSymbol=local_symbol,
        )


def instrument_id_to_ib_contract_simplified_symbology(  # noqa: C901 (too complex)
    instrument_id: InstrumentId,
) -> IBContract:
    if instrument_id.venue.value in VENUES_CASH and (
        m := RE_CASH.match(instrument_id.symbol.value)
    ):
        return IBContract(
            secType="CASH",
            exchange=instrument_id.venue.value,
            localSymbol=f"{m['symbol']}.{m['currency']}",
        )
    elif instrument_id.venue.value in VENUES_CRYPTO and (
        m := RE_CRYPTO.match(instrument_id.symbol.value)
    ):
        return IBContract(
            secType="CRYPTO",
            exchange=instrument_id.venue.value,
            localSymbol=f"{m['symbol']}.{m['currency']}",
        )
    elif instrument_id.venue.value in VENUES_OPT and (
        m := RE_OPT.match(instrument_id.symbol.value)
    ):
        return IBContract(
            secType="OPT",
            exchange=instrument_id.venue.value,
            localSymbol=f"{m['symbol'].ljust(6)}{m['expiry']}{m['right']}{m['strike']}{m['decimal']}",
        )
    elif instrument_id.venue.value in VENUES_FUT:
        if m := RE_FUT.match(instrument_id.symbol.value):
            return IBContract(
                secType="FUT",
                exchange=instrument_id.venue.value,
                localSymbol=f"{m['symbol']}{m['month']}{m['year'][-1]}",
            )
        elif m := RE_FUT_UNDERLYING.match(instrument_id.symbol.value):
            return IBContract(
                secType="CONTFUT",
                exchange=instrument_id.venue.value,
                symbol=m["symbol"],
            )
        elif m := RE_FOP.match(instrument_id.symbol.value):
            return IBContract(
                secType="FOP",
                exchange=instrument_id.venue.value,
                localSymbol=f"{m['symbol']}{m['month']}{m['year'][-1]} {m['right']}{m['strike']}",
            )
        else:
            raise ValueError(f"Cannot parse {instrument_id}, use 2-digit year for FUT and FOP")
    elif instrument_id.venue.value in VENUES_CFD:
        if m := RE_CASH.match(instrument_id.symbol.value):
            return IBContract(
                secType="CFD",
                exchange="SMART",
                symbol=m["symbol"],
                localSymbol=f"{m['symbol']}.{m['currency']}",
            )
        else:
            return IBContract(
                secType="CFD",
                exchange="SMART",
                symbol=f"{instrument_id.symbol.value}".replace("-", " "),
            )
    elif instrument_id.venue.value in VENUES_CMDTY:
        return IBContract(
            secType="CMDTY",
            exchange="SMART",
            symbol=f"{instrument_id.symbol.value}".replace("-", " "),
        )
    elif str(instrument_id.symbol).startswith("^"):
        return IBContract(
            secType="IND",
            exchange=instrument_id.venue.value,
            localSymbol=instrument_id.symbol.value[1:],
        )

    # Default to Stock
    return IBContract(
        secType="STK",
        exchange="SMART",
        primaryExchange=instrument_id.venue.value,
        localSymbol=f"{instrument_id.symbol.value}".replace("-", " "),
    )


def instrument_id_to_ib_contract_databento_symbology(
    instrument_id: InstrumentId,
    exchange: str,
) -> IBContract:
    if instrument_id.venue.value in ["GLBX", "IFEU", "NDEX"]:
        assert exchange is not None
        if RE_FUT.match(instrument_id.symbol.value) or RE_FUT_ORIGINAL.match(
            instrument_id.symbol.value,
        ):
            return IBContract(
                secType="FUT",
                exchange=exchange,
                localSymbol=instrument_id.symbol.value,
            )
        elif RE_FOP.match(instrument_id.symbol.value) or RE_FOP_ORIGINAL.match(
            instrument_id.symbol.value,
        ):
            return IBContract(
                secType="FOP",
                exchange=exchange,
                localSymbol=instrument_id.symbol.value,
            )
        else:
            raise ValueError(
                f"Failed to parse ib_contract for {instrument_id}. "
                f"Ensure it is a valid Future InstrumentId",
            )
    else:
        if RE_OPT.match(instrument_id.symbol.value):
            return IBContract(
                secType="OPT",
                exchange=exchange,
                localSymbol=instrument_id.symbol.value,
            )
        else:
            return IBContract(
                secType="STK",
                exchange=exchange,
                localSymbol=instrument_id.symbol.value,
                currency="USD",
            )
