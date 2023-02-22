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

import datetime
import re
import time
from decimal import Decimal

import msgspec

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import asset_class_from_str
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.crypto_perpetual import CryptoPerpetual
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.instruments.future import Future
from nautilus_trader.model.instruments.option import Option
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


venues_cash = ["IDEALPRO"]
venues_crypto = ["PAXOS"]
venues_opt = ["SMART"]
venues_stk = [
    "ARCA",  # US
    "NASDAQ",  # US
    "NYSE",  # US
    "AMEX",  # US
    "ASX",  # AU
    "MEXI",  # MX
    "ALPHA",  # CA
    "LSE",  # UK
    "TSEJ",  # JP
]
venues_fut = [
    "CBOT",  # US
    "CME",  # US
    "COMEX",  # US
    "KCBT",  # US
    "MGE",  # US
    "NYMEX",  # US
    "NYBOT",  # US
    "SNFE",  # AU
]

re_cash = re.compile(r"^(?P<symbol>[A-Z]{3})\/(?P<currency>[A-Z]{3})$")
re_opt = re.compile(
    r"^(?P<symbol>^[A-Z]{1,6})(?P<expiry>\d{6})(?P<right>[CP])(?P<strike>\d{5})(?P<decimal>\d{3})$",
)
re_ind = re.compile(r"^(?P<symbol>\w{1,3})$")
re_fut = re.compile(r"^(?P<symbol>\w{1,3})(?P<month>[FGHJKMNQUVXZ])(?P<year>\d)$")
re_fop = re.compile(
    r"^(?P<symbol>\w{1,3})(?P<month>[FGHJKMNQUVXZ])(?P<year>\d)(?P<right>[CP])(?P<strike>.{4,5})$",
)
re_crypto = re.compile(r"^(?P<symbol>[A-Z]*)\/(?P<currency>[A-Z]{3})$")


def _extract_isin(details: IBContractDetails):
    for tag_value in details.secIdList:
        if tag_value.tag == "ISIN":
            return tag_value.value
    raise ValueError("No ISIN found")


def _tick_size_to_precision(tick_size: float) -> int:
    tick_size_str = f"{tick_size:f}"
    return len(tick_size_str.partition(".")[2].rstrip("0"))


def sec_type_to_asset_class(sec_type: str):
    mapping = {
        "STK": "EQUITY",
        "IND": "INDEX",
        "CASH": "FX",
        "BOND": "BOND",
    }
    return asset_class_from_str(mapping.get(sec_type, sec_type))


def parse_instrument(
    contract_details: IBContractDetails,
) -> Instrument:
    security_type = contract_details.contract.secType
    if security_type == "STK":
        return parse_equity_contract(details=contract_details)
    elif security_type == "FUT" or security_type == "CONTFUT":
        return parse_future_contract(details=contract_details)
    elif security_type == "OPT":
        return parse_option_contract(details=contract_details)
    elif security_type == "CASH":
        print(contract_details)
        return parse_forex_contract(details=contract_details)
    elif security_type == "CRYPTO":
        return parse_crypto_contract(details=contract_details)
    else:
        raise ValueError(f"Unknown {security_type=}")


def contract_details_to_dict(details: IBContractDetails) -> dict:
    dict_details = details.dict().copy()
    dict_details["contract"] = details.contract.dict().copy()
    return dict_details


def parse_equity_contract(details: IBContractDetails) -> Equity:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    instrument_id = ib_contract_to_instrument_id(details.contract)
    return Equity(
        instrument_id=instrument_id,
        native_symbol=Symbol(details.contract.localSymbol),
        currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        price_increment=Price(details.minTick, price_precision),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        isin=_extract_isin(details),
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def parse_future_contract(
    details: IBContractDetails,
) -> Future:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    instrument_id = ib_contract_to_instrument_id(details.contract)
    return Future(
        instrument_id=instrument_id,
        native_symbol=Symbol(details.contract.localSymbol),
        asset_class=sec_type_to_asset_class(details.underSecType),
        currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        price_increment=Price(details.minTick, price_precision),
        multiplier=Quantity.from_int(int(details.contract.multiplier)),
        lot_size=Quantity.from_int(1),
        underlying=details.underSymbol,
        expiry_date=datetime.datetime.strptime(
            details.contract.lastTradeDateOrContractMonth,
            "%Y%m%d",
        ).date(),
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def parse_option_contract(
    details: IBContractDetails,
) -> Option:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    instrument_id = ib_contract_to_instrument_id(details.contract)
    asset_class = {
        "STK": AssetClass.EQUITY,
    }[details.underSecType]
    kind = {
        "C": OptionKind.CALL,
        "P": OptionKind.PUT,
    }[details.contract.right]
    return Option(
        instrument_id=instrument_id,
        native_symbol=Symbol(details.contract.localSymbol),
        asset_class=asset_class,
        currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        price_increment=Price(details.minTick, price_precision),
        multiplier=Quantity.from_int(int(details.contract.multiplier)),
        lot_size=Quantity.from_int(1),
        underlying=details.underSymbol,
        strike_price=Price.from_str(str(details.contract.strike)),
        expiry_date=datetime.datetime.strptime(
            details.contract.lastTradeDateOrContractMonth,
            "%Y%m%d",
        ).date(),
        kind=kind,
        ts_event=timestamp,
        ts_init=timestamp,
        info=contract_details_to_dict(details),
    )


def parse_forex_contract(
    details: IBContractDetails,
) -> CurrencyPair:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    instrument_id = ib_contract_to_instrument_id(details.contract)
    return CurrencyPair(
        instrument_id=instrument_id,
        native_symbol=Symbol(details.contract.localSymbol),
        base_currency=Currency.from_str(details.contract.symbol),
        quote_currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        size_precision=Quantity.from_int(1),
        price_increment=Price(details.minTick, price_precision),
        size_increment=Quantity(getattr(details, "sizeMinTick", None) or 1, 1),
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
) -> CryptoPerpetual:
    price_precision: int = _tick_size_to_precision(details.minTick)
    size_precision = details.minSize.as_tuple().exponent * -1
    timestamp = time.time_ns()
    instrument_id = ib_contract_to_instrument_id(details.contract)
    return CryptoPerpetual(
        instrument_id=instrument_id,
        native_symbol=Symbol(details.contract.localSymbol),
        base_currency=Currency.from_str(details.contract.symbol),
        quote_currency=Currency.from_str(details.contract.currency),
        settlement_currency=Currency.from_str(details.contract.currency),
        is_inverse=False,  # No inverse instruments trade on InteractiveBrokers?
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


def ib_contract_to_instrument_id(contract: IBContract) -> InstrumentId:
    PyCondition.type(contract, IBContract, "IBContract")

    security_type = contract.secType
    if security_type == "STK":
        symbol = contract.localSymbol.replace(" ", "-")
        venue = contract.primaryExchange if contract.exchange == "SMART" else contract.exchange
    elif security_type in ["OPT", "FUT", "FOP", "CONTFUT"]:
        symbol = contract.localSymbol.replace(" ", "") or contract.symbol.replace(" ", "")
        venue = contract.exchange
    elif security_type in ["CASH", "CRYPTO"]:
        symbol = (
            f"{contract.localSymbol}".replace(".", "/") or f"{contract.symbol}/{contract.currency}"
        )
        venue = contract.exchange
    else:
        symbol = None
        venue = None
    if symbol and venue:
        return InstrumentId(Symbol(symbol), Venue(venue))
    raise ValueError(f"Unknown {contract=}")


def instrument_id_to_ib_contract(instrument_id: InstrumentId) -> IBContract:
    PyCondition.type(instrument_id, InstrumentId, "InstrumentId")

    if instrument_id.venue.value in venues_cash and (
        m := re_cash.match(instrument_id.symbol.value)
    ):
        return IBContract(
            secType="CASH",
            exchange=instrument_id.venue.value,
            localSymbol=f"{m['symbol']}.{m['currency']}",
        )
    elif instrument_id.venue.value in venues_crypto and (
        m := re_crypto.match(instrument_id.symbol.value)
    ):
        return IBContract(
            secType="CRYPTO",
            exchange=instrument_id.venue.value,
            localSymbol=f"{m['symbol']}.{m['currency']}",
        )
    elif instrument_id.venue.value in venues_opt and (
        m := re_opt.match(instrument_id.symbol.value)
    ):
        return IBContract(
            secType="OPT",
            exchange=instrument_id.venue.value,
            localSymbol=f"{m['symbol'].ljust(6)}{m['expiry']}{m['right']}{m['strike']}{m['decimal']}",
        )
    elif instrument_id.venue.value in venues_fut:
        if m := re_fut.match(instrument_id.symbol.value):
            return IBContract(
                secType="FUT",
                exchange=instrument_id.venue.value,
                localSymbol=f"{m['symbol']}{m['month']}{m['year']}",
            )
        elif m := re_ind.match(instrument_id.symbol.value):
            return IBContract(
                secType="CONTFUT",
                exchange=instrument_id.venue.value,
                symbol=m["symbol"],
            )
        elif m := re_fop.match(instrument_id.symbol.value):
            return IBContract(
                secType="FOP",
                exchange=instrument_id.venue.value,
                localSymbol=f"{m['symbol']}{m['month']}{m['year']} {m['right']}{m['strike']}",
            )
    elif instrument_id.venue.value in venues_stk:
        return IBContract(
            secType="STK",
            exchange="SMART",
            primaryExchange=instrument_id.venue.value,
            localSymbol=f"{instrument_id.symbol.value}".replace("-", " "),
        )
    elif instrument_id.venue.value == "InteractiveBrokers":  # keep until a better approach
        # This will allow to make Instrument request using IBContract from within Strategy
        # and depending on the Strategy requirement
        return msgspec.json.decode(instrument_id.symbol.value, type=IBContract)
    raise ValueError(
        f"Ambiguous {instrument_id}, use `IBContract` instead. "
        f"If you believe this is correct InstrumentId, submit Issue to support similar ids.",
    )
