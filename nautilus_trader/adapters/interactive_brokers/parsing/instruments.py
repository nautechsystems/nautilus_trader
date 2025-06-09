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

    return asset_class_from_str(mapping.get(sec_type, sec_type))


def contract_details_to_ib_contract_details(details: ContractDetails) -> IBContractDetails:
    details.contract = IBContract(**details.contract.__dict__)
    details = IBContractDetails(**details.__dict__)

    return details


def parse_instrument(
    contract_details: IBContractDetails,
    venue: str,
    symbology_method: SymbologyMethod = SymbologyMethod.IB_SIMPLIFIED,
) -> Instrument:
    security_type = contract_details.contract.secType
    instrument_id = ib_contract_to_instrument_id(
        contract_details.contract,
        venue,
        symbology_method,
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
) -> InstrumentId:
    PyCondition.type(contract, IBContract, "IBContract")

    if symbology_method == SymbologyMethod.IB_SIMPLIFIED:
        return ib_contract_to_instrument_id_simplified_symbology(contract, venue)
    elif symbology_method == SymbologyMethod.IB_RAW:
        return ib_contract_to_instrument_id_raw_symbology(contract, venue)
    else:
        raise NotImplementedError(f"{symbology_method} not implemented")


def ib_contract_to_instrument_id_simplified_symbology(  # noqa: C901 (too complex)
    contract: IBContract,
    venue: str,
) -> InstrumentId:
    security_type = contract.secType

    if security_type == "STK":
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
) -> IBContract:
    PyCondition.type(instrument_id, InstrumentId, "InstrumentId")

    if symbology_method == SymbologyMethod.IB_SIMPLIFIED:
        return instrument_id_to_ib_contract_simplified_symbology(instrument_id, exchange)
    elif symbology_method == SymbologyMethod.IB_RAW:
        return instrument_id_to_ib_contract_raw_symbology(instrument_id)
    else:
        raise NotImplementedError(f"{symbology_method} not implemented")


def instrument_id_to_ib_contract_simplified_symbology(  # noqa: C901 (too complex)
    instrument_id: InstrumentId,
    exchange: str,
) -> IBContract:
    if exchange in VENUES_CASH and (m := RE_CASH.match(instrument_id.symbol.value)):
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
