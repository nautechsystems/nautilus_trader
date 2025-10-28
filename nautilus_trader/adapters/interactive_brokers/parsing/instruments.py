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

import pandas as pd
from ibapi.contract import ContractDetails

from nautilus_trader.adapters.interactive_brokers.common import ComboLeg
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
from nautilus_trader.model.instruments.option_spread import OptionSpread
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


VENUE_MEMBERS: dict[str, list[str]] = {
    # ICE Endex
    "NDEX": ["ENDEX"],  # ICE Endex
    # CME Group Exchanges - Includes related index exchanges
    "XCME": [
        "CME",
    ],  # Chicago Mercantile Exchange (Floor/ClearPort might use this; for ES, RTY, NKD futures etc.)
    "XCEC": ["CME"],  # CME Crypto (related to CME)
    "XFXS": ["CME"],  # CME FX Link, FX Spot (related to CME)
    # Chicago Board of Trade Segments
    "XCBT": [
        "CBOT",
    ],  # Chicago Board of Trade (Floor/ClearPort might use this; for ZN, ZB, ZS futures etc.)
    "CBCM": ["CBOT"],  # CBOT Commodities (specific segment, related to CBOT)
    # New York Mercantile Exchange Segments
    "XNYM": [
        "NYMEX",
    ],  # New York Mercantile Exchange (Floor/ClearPort might use this; for CL, NG futures etc.)
    "NYUM": ["NYMEX"],  # NYMEX Metals (specific segment, related to NYMEX)
    # ICE Futures US (formerly NYBOT)
    "IFUS": ["NYBOT"],  # ICE Futures US (IBKR uses NYBOT for this; for CC, KC, SB futures etc.)
    # GLBX, Name used by databento
    "GLBX": [
        "CBOT",
        "CME",
        "NYBOT",
        "NYMEX",
    ],  # CME Group Globex (Parent MIC for electronic trading on these exchanges)
    # US Major Exchanges & Index Venues
    "XNAS": ["NASDAQ"],  # Nasdaq Stock Market (for IXIC, NDX indices)
    "XNYS": ["NYSE"],  # New York Stock Exchange (for NYA index)
    "ARCX": ["ARCA"],  # NYSE Arca
    "BATS": ["BATS"],  # Cboe BZX Exchange U.S. (formerly BATS)
    "IEXG": ["IEX"],  # Investors Exchange
    "XCBO": [
        "CBOE",
    ],  # Cboe Options Exchange (for SPX, RUT options and indices like SPX.IND, RUT.IND)
    "XCBF": ["CFE"],  # Cboe Futures Exchange (IBKR uses CFE, e.g., for VIX futures)
    # Canadian Exchanges
    "XTSE": ["TSX"],  # Toronto Stock Exchange (for GSPTSE index)
    # ICE Europe Exchanges
    "IFEU": [
        "ICEEU",
        "ICEEUSOFT",
        "IPE",
    ],  # ICE Futures Europe (IBKR uses for products on this exchange: ICEEU (general), ICEEUSOFT (softs), IPE (energy))
    # European Exchanges
    "XLON": ["LSE"],  # London Stock Exchange (for UKX, FTMC indices)
    "XPAR": ["SBF"],  # Euronext Paris (IBKR uses SBF) (for FCHI index)
    "XETR": ["IBIS"],  # Deutsche Börse Xetra (IBKR uses IBIS for Xetra)
    "XEUR": [
        "DTB",
        "EUREX",
        "SOFFEX",
    ],  # Eurex (IBKR uses SOFFEX/DTB/EUREX; SOFFEX was precursor. For STOXX50E, GDAXI derivatives and index reference)
    "XAMS": ["AEB"],  # Euronext Amsterdam (IBKR uses AEB) (for AEX index)
    "XBRU": ["EBS"],  # Euronext Brussels Equities (IBKR uses EBS)
    "XBRD": [
        "BELFOX",
    ],  # Euronext Brussels Derivatives (IBKR uses BELFOX) - XBRD is MIC for "EURONEXT BRUSSELS - DERIVATIVES MARKET"
    "XLIS": ["BVLP"],  # Euronext Lisbon (IBKR uses BVLP)
    "XDUB": ["IRE"],  # Euronext Dublin (IBKR uses IRE)
    "XOSL": ["OSL"],  # Euronext Oslo (Oslo Børs) (IBKR uses OSL)
    "XSWX": [
        "EBS",
        "SIX",
        "SWX",
    ],  # SIX Swiss Exchange (IBKR uses SWX for equities; EBS old IBKR code. For SSMI index)
    "XSVX": [
        "VRTX",
    ],  # SIX Swiss Exchange Derivatives (IBKR uses VRTX) - XSVX is MIC for "SIX SWISS EXCHANGE - DERIVATIVES MARKET"
    "XMIL": [
        "BIT",
        "BVME",
        "IDEM",
    ],  # Borsa Italiana (Euronext Milan) (IBKR uses BIT; BVME for equities, IDEM for derivatives. For FTMIB index)
    "XMAD": [
        "MDRD",
        "BME",
    ],  # Bolsas y Mercados Españoles (BME) - Madrid (IBKR uses MDRD; BME also used. For IBEX index)
    "DXEX": [
        "BATEEN",
    ],  # Cboe Europe Equities - Netherlands (IBKR uses BATEEN) - DXEX is MIC for Cboe NL (post-Brexit main Cboe Europe venue)
    "XWBO": ["WBAG"],  # Wiener Börse (Vienna Stock Exchange) (IBKR uses WBAG)
    "XBUD": ["BUX"],  # Budapest Stock Exchange (IBKR uses BUX)
    "XPRA": ["PRA"],  # Prague Stock Exchange (IBKR uses PRA)
    "XWAR": ["WSE"],  # Warsaw Stock Exchange (IBKR uses WSE)
    "XIST": ["ISE"],  # Bursa Istanbul (IBKR often uses ISE for Istanbul Stock Exchange equities)
    # Nasdaq Nordic Exchanges
    "XSTO": ["SFB"],  # Nasdaq Stockholm (IBKR uses SFB)
    "XCSE": ["KFB"],  # Nasdaq Copenhagen (IBKR uses KFB)
    "XHEL": ["HMB"],  # Nasdaq Helsinki (IBKR uses HMB)
    "XICE": ["ISB"],  # Nasdaq Iceland (IBKR uses ISB)
    # Asia-Pacific Exchanges
    "XASX": ["ASX"],  # Australian Securities Exchange (for S&P/ASX 200 - AXJO index)
    "XHKG": ["SEHK"],  # Stock Exchange of Hong Kong (Equities) (IBKR uses SEHK)
    "XHKF": ["HKFE"],  # Hong Kong Futures Exchange (for H S I derivatives and index reference)
    "XSES": ["SGX"],  # Singapore Exchange (for STI index and some international derivatives)
    "XOSE": [
        "OSE.JPN",
    ],  # Osaka Exchange (IBKR uses OSE.JPN) (for N225 derivatives and index reference)
    "XTKS": [
        "TSEJ",
        "TSE.JPN",
    ],  # Tokyo Stock Exchange (IBKR uses TSEJ for equities; TSE.JPN for TOPX index)
    "XKRX": [
        "KSE",
        "KRX",
    ],  # Korea Exchange (IBKR uses KSE for equities; KRX relevant for KOSPI - KS11 index)
    "XTAI": [
        "TASE",
        "TWSE",
    ],  # Taiwan Stock Exchange (MIC XTAI. IBKR uses TASE for Taiwan equities; TWSE also relevant for
    # TAIEX - TWII index. Note: XTAE is Tel Aviv's MIC)
    "XSHG": [
        "SEHKNTL",
        "SSE",
    ],  # Shanghai Stock Exchange (IBKR uses SEHKNTL for Stock Connect Northbound; SSE for SSEC index direct reference)
    "XSHE": ["SEHKSZSE"],  # Shenzhen Stock Exchange (Stock Connect Northbound) (IBKR uses SEHKSZSE)
    "XNSE": ["NSE"],  # National Stock Exchange of India (IBKR uses NSE) (for NIFTY 50 - NSEI index)
    "XBOM": ["BSE"],  # Bombay Stock Exchange (IBKR uses BSE) (for SENSEX - BSESN index)
    # Other Derivatives Exchanges
    "XSFE": ["SNFE"],  # Sydney Futures Exchange (now ASX 24, IBKR uses SNFE)
    "XMEX": ["MEXDER"],  # Mexican Derivatives Exchange
    # African, Middle Eastern, South American Exchanges
    "XJSE": [
        "JSE",
    ],  # Johannesburg Stock Exchange (IBKR uses JSE) (for FTSE/JSE All Share - JALSH index)
    "XBOG": ["BVC"],  # Bolsa de Valores de Colombia (IBKR uses BVC)
    "XTAE": [
        "TASE",  # Tel Aviv Stock Exchange (MIC XTAE. IBKR uses TASE for Tel Aviv equities; note XTAI is Taiwan)
    ],
    "BVMF": [
        "BVMF",
    ],  # B3 - Brasil Bolsa Balcão (IBKR uses BVMF; for IBOVESPA - BVSP index. BVMF is also the MIC)
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


def sec_type_to_asset_class(sec_type: str) -> AssetClass:
    mapping = {
        "STK": "EQUITY",
        "IND": "INDEX",
        "CASH": "FX",
        "BOND": "DEBT",
        "CMDTY": "COMMODITY",
        "FUT": "INDEX",
    }

    # Handle empty or None sec_type
    if not sec_type:
        return AssetClass.EQUITY  # Default to EQUITY

    mapped_value = mapping.get(sec_type, sec_type)
    # If the mapped value is still not a valid AssetClass, default to EQUITY
    try:
        return asset_class_from_str(mapped_value)
    except Exception:
        return AssetClass.EQUITY


def contract_details_to_ib_contract_details(details: ContractDetails) -> IBContractDetails:
    details.contract = IBContract(**details.contract.__dict__)
    details = IBContractDetails(**details.__dict__)

    return details


def parse_instrument(
    contract_details: IBContractDetails,
    venue: str,
    symbology_method: SymbologyMethod = SymbologyMethod.IB_SIMPLIFIED,
    contract_details_map: dict[int, IBContractDetails] | None = None,
) -> Instrument:
    security_type = contract_details.contract.secType
    instrument_id = ib_contract_to_instrument_id(
        contract_details.contract,
        venue,
        symbology_method,
        contract_details_map,
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
    elif security_type == "BAG":
        return parse_option_spread(details=contract_details, instrument_id=instrument_id)
    else:
        raise ValueError(f"Unknown {security_type=}")


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


def _extract_isin(details: IBContractDetails) -> int:
    if details.secIdList:
        for tag_value in details.secIdList:
            if tag_value.tag == "ISIN":
                return tag_value.value

    raise ValueError("No ISIN found")


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


def parse_futures_contract(
    details: IBContractDetails,
    instrument_id: InstrumentId,
) -> FuturesContract:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    expiration = expiry_timestring_to_datetime(details.contract.lastTradeDateOrContractMonth)
    activation = expiration - pd.Timedelta(days=90)  # TODO: Make this more accurate
    raw_symbol = (
        details.contract.localSymbol
        if details.contract.secType == "FUT"
        else details.contract.symbol
    )  # symbol for CONTFUT

    return FuturesContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol(raw_symbol),
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

    # For options, the multiplier represents the lot size (e.g., 100 shares per contract)
    multiplier = Quantity.from_str(details.contract.multiplier)

    return OptionContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol(details.contract.localSymbol),
        asset_class=asset_class,
        currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        price_increment=Price(details.minTick, price_precision),
        multiplier=multiplier,
        lot_size=multiplier,  # For options, lot size equals multiplier
        underlying=details.underSymbol,
        strike_price=Price(details.contract.strike, price_precision),
        activation_ns=activation.value,
        expiration_ns=expiration.value,
        option_kind=option_kind,
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


def parse_option_spread(
    details: IBContractDetails,
    instrument_id: InstrumentId,
) -> OptionSpread:
    """
    Parse an option spread from BAG contract details.

    Uses only information available from the contract details. For asset class and other
    properties, uses the same information as would be used for individual option legs.

    """
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()

    # Extract underlying symbol from contract details
    underlying = details.underSymbol or details.contract.symbol or "UNKNOWN"

    # Determine asset class from underlying security type
    asset_class = (
        sec_type_to_asset_class(details.underSecType) if details.underSecType else AssetClass.EQUITY
    )

    # For options, the multiplier represents the lot size (e.g., 100 shares per contract)
    multiplier = Quantity.from_str(details.contract.multiplier or "100")

    return OptionSpread(
        instrument_id=instrument_id,
        raw_symbol=Symbol(details.contract.localSymbol or details.contract.symbol),
        asset_class=asset_class,
        currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        price_increment=Price(details.minTick, price_precision),
        multiplier=multiplier,
        lot_size=multiplier,  # For options, lot size equals multiplier
        underlying=underlying,
        strategy_type="SPREAD",
        activation_ns=0,  # BAG contracts don't have single expiration dates
        expiration_ns=0,  # BAG contracts don't have single expiration dates
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def parse_spread_instrument_id(
    instrument_id: InstrumentId,
    leg_contract_details: list[tuple[IBContractDetails, int]],
    clock_timestamp_ns: int | None = None,
) -> OptionSpread:
    """
    Parse a spread instrument ID into an OptionSpread instrument.

    Uses contract details from the first leg to determine spread properties.
    This ensures consistency with how individual option contracts are handled.

    Parameters
    ----------
    instrument_id : InstrumentId
        The spread instrument ID to parse.
    leg_contract_details : list[tuple[IBContractDetails, int]]
        List of (contract_details, ratio) tuples for the spread legs.
        Contract details will be used for instrument properties.
    clock_timestamp_ns : int | None, optional
        Clock timestamp in nanoseconds. If not provided, current time is used.

    Returns
    -------
    OptionSpread
        The parsed option spread instrument.

    Raises
    ------
    ValueError
        If the instrument ID cannot be parsed as a spread or no leg contract details provided.

    """
    try:
        if not leg_contract_details:
            raise ValueError("leg_contract_details must be provided")

        # Use contract details from first leg
        first_details, _ = leg_contract_details[0]
        first_contract = first_details.contract

        # Extract all properties from the first leg contract details
        currency = Currency.from_str(first_contract.currency)
        underlying = first_details.underSymbol or first_contract.symbol

        # Use contract multiplier
        multiplier = Quantity.from_str(str(first_contract.multiplier))

        # Determine asset class based on security type
        if first_contract.secType == "FOP":
            asset_class = AssetClass.INDEX  # Futures options
        else:  # OPT
            asset_class = AssetClass.EQUITY  # Equity options

        # Read price increment from contract details
        price_increment = Price(
            first_details.minTick,
            _tick_size_to_precision(first_details.minTick),
        )
        price_precision = _tick_size_to_precision(first_details.minTick)

        # Use provided timestamp or current time
        timestamp = clock_timestamp_ns if clock_timestamp_ns is not None else time.time_ns()

        # For options spreads, lot size equals multiplier (same as individual option contracts)
        lot_size = multiplier

        # Create info dict with contract details for the first leg
        # This is needed for the data client to create subscription contracts
        info = {
            "contract": {
                "secType": first_contract.secType,
                "symbol": first_contract.symbol,
                "currency": first_contract.currency,
                "multiplier": first_contract.multiplier,
            },
        }

        return OptionSpread(
            instrument_id=instrument_id,
            raw_symbol=Symbol(instrument_id.symbol.value),
            asset_class=asset_class,
            currency=currency,
            price_precision=price_precision,
            price_increment=price_increment,
            multiplier=multiplier,
            lot_size=lot_size,
            underlying=underlying,
            strategy_type="SPREAD",
            activation_ns=0,  # Spreads don't have single activation dates
            expiration_ns=0,  # Spreads don't have single expiration dates
            ts_event=timestamp,
            ts_init=timestamp,
            info=info,
        )
    except Exception as e:
        raise ValueError(f"Failed to parse spread instrument ID {instrument_id}: {e}") from e


def contract_details_to_dict(details: IBContractDetails) -> dict:
    dict_details = details.dict().copy()
    dict_details["contract"] = details.contract.dict().copy()

    if dict_details.get("secIdList"):
        dict_details["secIdList"] = {
            tag_value.tag: tag_value.value for tag_value in dict_details["secIdList"]
        }

    return dict_details


def _tick_size_to_precision(tick_size: float | Decimal) -> int:
    tick_size_str = f"{tick_size:.10f}"

    return len(tick_size_str.partition(".")[2].rstrip("0"))


def decade_digit(last_digit: str, contract: IBContract) -> int:
    if year := contract.lastTradeDateOrContractMonth[:4]:
        return int(year[2:3])
    elif int(last_digit) > int(repr(datetime.datetime.now().year)[-1]):
        return int(repr(datetime.datetime.now().year)[-2]) - 1
    else:
        return int(repr(datetime.datetime.now().year)[-2])


def ib_contract_to_instrument_id(
    contract: IBContract,
    venue: str,
    symbology_method: SymbologyMethod = SymbologyMethod.IB_SIMPLIFIED,
    contract_details_map: dict[int, IBContractDetails] | None = None,
) -> InstrumentId:
    PyCondition.type(contract, IBContract, "IBContract")

    if symbology_method == SymbologyMethod.IB_SIMPLIFIED:
        return ib_contract_to_instrument_id_simplified_symbology(
            contract,
            venue,
            contract_details_map,
        )
    elif symbology_method == SymbologyMethod.IB_RAW:
        return ib_contract_to_instrument_id_raw_symbology(contract, venue)
    else:
        raise NotImplementedError(f"{symbology_method} not implemented")


def ib_contract_to_instrument_id_simplified_symbology(  # noqa: C901 (too complex)
    contract: IBContract,
    venue: str,
    contract_details_map: dict[int, IBContractDetails] | None = None,
) -> InstrumentId:
    security_type = contract.secType

    if security_type == "BAG":
        return bag_contract_to_instrument_id(contract, venue, contract_details_map)
    elif security_type == "STK":
        symbol = (contract.localSymbol or contract.symbol).replace(" ", "-")
    elif security_type == "IND":
        symbol = f"^{(contract.localSymbol or contract.symbol)}"
    elif security_type == "OPT":
        symbol = contract.localSymbol.replace(" ", "")
    elif security_type == "CONTFUT":
        symbol = contract.symbol
    elif security_type == "FUT" and (m := RE_FUT_ORIGINAL.match(contract.localSymbol)):
        symbol = f"{m['symbol']}{m['month']}{m['year']}"
    elif security_type == "FUT" and (m := RE_FUT2_ORIGINAL.match(contract.localSymbol)):
        symbol = f"{m['symbol']}{FUTURES_MONTH_TO_CODE[m['month']]}{m['year'][-1]}"
    elif security_type == "FUT" and (m := RE_FUT3_ORIGINAL.match(contract.localSymbol)):
        symbol = f"{m['symbol']}{FUTURES_MONTH_TO_CODE[m['month']]}{m['year'][-1]}"
    elif security_type == "FOP" and (m := RE_FOP_ORIGINAL.match(contract.localSymbol)):
        symbol = f"{m['symbol']}{m['month']}{m['year']} {m['right']}{m['strike']}"
    elif security_type in ["CASH", "CRYPTO"]:
        symbol = (
            f"{contract.localSymbol}".replace(".", "/") or f"{contract.symbol}/{contract.currency}"
        )
    elif security_type == "CFD":
        if m := RE_CFD_CASH.match(contract.localSymbol):
            symbol = (
                f"{contract.localSymbol}".replace(".", "/")
                or f"{contract.symbol}/{contract.currency}"
            )
        else:
            symbol = (contract.symbol).replace(" ", "-")
    elif security_type == "CMDTY":
        symbol = (contract.symbol).replace(" ", "-")
    else:
        symbol = None

    if symbol:
        return InstrumentId(Symbol(symbol), Venue(venue))

    raise ValueError(f"Unknown {contract=}")


def bag_contract_to_instrument_id(
    contract: IBContract,
    venue: str,
    contract_details_map: dict[int, IBContractDetails] | None = None,
) -> InstrumentId:
    """
    Create a spread instrument ID from a BAG contract.

    This is the reverse operation of _create_bag_contract_from_spread.
    It converts an IB BAG contract back to a Nautilus option spread instrument ID.

    Parameters
    ----------
    contract : IBContract
        The BAG contract with comboLegs representing the spread
    venue : str
        The venue for the instrument ID
    contract_details_map : dict[int, IBContractDetails] | None
        Map of contract IDs (conIds) to their contract details for leg resolution

    Returns
    -------
    InstrumentId
        A spread instrument ID created with InstrumentId.new_spread()

    """
    try:
        if not contract.comboLegs:
            raise ValueError("BAG contract has no combo legs")

        # Convert combo legs to instrument ID tuples
        leg_tuples = []

        for combo_leg in contract.comboLegs:
            # Get the contract details for this leg using conId
            if contract_details_map and combo_leg.conId in contract_details_map:
                leg_contract_details = contract_details_map[combo_leg.conId]
                leg_contract = leg_contract_details.contract

                # Create instrument ID from the leg contract
                leg_instrument_id = ib_contract_to_instrument_id_simplified_symbology(
                    leg_contract,
                    venue,
                )
            else:
                raise ValueError(
                    f"Cannot resolve leg instrument ID for conId {combo_leg.conId}. "
                    f"Contract details map not provided or incomplete.",
                )

            # Determine ratio (positive for BUY, negative for SELL)
            ratio = combo_leg.ratio if combo_leg.action == "BUY" else -combo_leg.ratio

            leg_tuples.append((leg_instrument_id, ratio))

        # Create the spread instrument ID
        return InstrumentId.new_spread(leg_tuples)

    except Exception as e:
        raise ValueError(f"Failed to create spread instrument ID from BAG contract {contract}: {e}")


def ib_contract_to_instrument_id_raw_symbology(
    contract: IBContract,
    venue: str,
) -> InstrumentId:
    if contract.secType == "CFD":
        symbol = f"{contract.localSymbol}={contract.secType}"
    elif contract.secType == "CMDTY":
        symbol = f"{contract.localSymbol}={contract.secType}"
    else:
        symbol = f"{contract.localSymbol}={contract.secType}"

    return InstrumentId.from_str(f"{symbol}.{venue}")


def instrument_id_to_ib_contract(
    instrument_id: InstrumentId,
    exchange: str,
    symbology_method: SymbologyMethod = SymbologyMethod.IB_SIMPLIFIED,
    contract_details_map: dict[InstrumentId, IBContractDetails] | None = None,
) -> IBContract:
    PyCondition.type(instrument_id, InstrumentId, "InstrumentId")

    if symbology_method == SymbologyMethod.IB_SIMPLIFIED:
        return instrument_id_to_ib_contract_simplified_symbology(
            instrument_id,
            exchange,
            contract_details_map,
        )
    elif symbology_method == SymbologyMethod.IB_RAW:
        return instrument_id_to_ib_contract_raw_symbology(instrument_id)
    else:
        raise NotImplementedError(f"{symbology_method} not implemented")


def instrument_id_to_ib_contract_simplified_symbology(  # noqa: C901 (too complex)
    instrument_id: InstrumentId,
    exchange: str,
    contract_details_map: dict[InstrumentId, IBContractDetails] | None = None,
) -> IBContract:
    if instrument_id.is_spread():
        return instrument_id_to_bag_contract(instrument_id, exchange, contract_details_map)
    elif exchange in VENUES_CASH and (m := RE_CASH.match(instrument_id.symbol.value)):
        return IBContract(
            secType="CASH",
            exchange=exchange,
            localSymbol=f"{m['symbol']}.{m['currency']}",
        )
    elif exchange in VENUES_CRYPTO and (m := RE_CRYPTO.match(instrument_id.symbol.value)):
        return IBContract(
            secType="CRYPTO",
            exchange=exchange,
            localSymbol=f"{m['symbol']}.{m['currency']}",
        )
    elif exchange in VENUES_OPT and (m := RE_OPT.match(instrument_id.symbol.value)):
        return IBContract(
            secType="OPT",
            exchange=exchange,
            localSymbol=f"{m['symbol'].ljust(6)}{m['expiry']}{m['right']}{m['strike']}{m['decimal']}",
        )
    elif exchange in VENUES_FUT:
        if m := RE_FUT_ORIGINAL.match(instrument_id.symbol.value):
            return IBContract(
                secType="FUT",
                exchange=exchange,
                localSymbol=f"{m['symbol']}{m['month']}{m['year']}",
            )
        elif m := RE_FUT_UNDERLYING.match(instrument_id.symbol.value):
            return IBContract(
                secType="CONTFUT",
                exchange=exchange,
                symbol=m["symbol"],
            )
        elif m := RE_FOP_ORIGINAL.match(instrument_id.symbol.value):
            return IBContract(
                secType="FOP",
                exchange=exchange,
                localSymbol=f"{m['symbol']}{m['month']}{m['year']} {m['right']}{m['strike']}",
            )
        else:
            raise ValueError(f"Cannot parse {instrument_id}, use 2-digit year for FUT and FOP")
    elif exchange in VENUES_CFD:
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
    elif exchange in VENUES_CMDTY:
        return IBContract(
            secType="CMDTY",
            exchange="SMART",
            symbol=f"{instrument_id.symbol.value}".replace("-", " "),
        )
    elif str(instrument_id.symbol).startswith("^"):
        return IBContract(
            secType="IND",
            exchange=exchange,
            localSymbol=instrument_id.symbol.value[1:],
        )

    # Default to Stock
    return IBContract(
        secType="STK",
        exchange="SMART",
        primaryExchange=exchange,
        localSymbol=f"{instrument_id.symbol.value}".replace("-", " "),
    )


def instrument_id_to_bag_contract(
    instrument_id: InstrumentId,
    exchange: str,
    contract_details_map: dict[InstrumentId, IBContractDetails] | None = None,
) -> IBContract:
    try:
        # Parse the spread ID back to individual legs
        leg_tuples = instrument_id.to_list()

        if not leg_tuples:
            raise ValueError("Spread instrument ID has no legs")

        # Create combo legs for the BAG contract
        combo_legs = []

        for leg_instrument_id, ratio in leg_tuples:
            # Get the contract details for this leg to extract conId
            if contract_details_map and leg_instrument_id in contract_details_map:
                contract_details = contract_details_map[leg_instrument_id]
                con_id = contract_details.contract.conId
                currency = contract_details.contract.currency
            else:
                # If we don't have contract details, we can't create a valid BAG contract
                raise ValueError(
                    f"Contract details not found for leg {leg_instrument_id}. "
                    f"Ensure all legs are loaded in the instrument provider before creating spread.",
                )

            # Determine action based on ratio (positive = BUY, negative = SELL)
            action = "BUY" if ratio > 0 else "SELL"
            abs_ratio = abs(ratio)

            # Create a combo leg with the actual conId
            combo_leg = ComboLeg(
                conId=con_id,
                ratio=abs_ratio,
                action=action,
                exchange=exchange,
            )
            combo_legs.append(combo_leg)

        # Create the BAG contract
        return IBContract(
            secType="BAG",
            exchange=exchange,
            currency=currency,
            comboLegs=combo_legs,
            comboLegsDescrip=f"Spread: {instrument_id.symbol.value}",
        )
    except Exception as e:
        raise ValueError(f"Failed to create BAG contract from spread {instrument_id}: {e}")


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
