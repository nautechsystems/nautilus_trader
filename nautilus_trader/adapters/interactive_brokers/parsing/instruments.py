# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
import time
from decimal import Decimal

from ib_insync import ContractDetails

from nautilus_trader.model.c_enums.asset_class import AssetClassParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.instruments.future import Future
from nautilus_trader.model.instruments.option import Option
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def _extract_isin(details: ContractDetails):
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
    return AssetClassParser.from_str_py(mapping.get(sec_type, sec_type))


def parse_instrument(
    contract_details: ContractDetails,
) -> Instrument:
    security_type = contract_details.contract.secType
    if security_type == "STK":
        return parse_equity_contract(details=contract_details)
    elif security_type == "FUT":
        return parse_future_contract(details=contract_details)
    elif security_type == "OPT":
        return parse_option_contract(details=contract_details)
    elif security_type == "CASH":
        return parse_forex_contract(details=contract_details)
    else:
        raise ValueError(f"Unknown {security_type=}")


def parse_equity_contract(details: ContractDetails) -> Equity:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    instrument_id = InstrumentId(
        symbol=Symbol(details.contract.localSymbol), venue=Venue(details.contract.primaryExchange)
    )
    return Equity(
        instrument_id=instrument_id,
        native_symbol=Symbol(details.contract.localSymbol),
        currency=Currency.from_str(details.contract.currency),
        price_precision=price_precision,
        price_increment=Price(details.minTick, price_precision),
        multiplier=Quantity.from_int(
            int(details.contract.multiplier or details.mdSizeMultiplier)
        ),  # is this right?
        lot_size=Quantity.from_int(1),
        isin=_extract_isin(details),
        ts_event=timestamp,
        ts_init=timestamp,
    )


def parse_future_contract(
    details: ContractDetails,
) -> Future:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    instrument_id = InstrumentId(
        symbol=Symbol(details.contract.localSymbol),
        venue=Venue(details.contract.primaryExchange or details.contract.exchange),
    )
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
            details.contract.lastTradeDateOrContractMonth, "%Y%m%d"
        ).date(),
        ts_event=timestamp,
        ts_init=timestamp,
    )


def parse_option_contract(
    details: ContractDetails,
) -> Option:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    instrument_id = InstrumentId(
        symbol=Symbol(details.contract.localSymbol.replace("  ", "")),
        venue=Venue(details.contract.primaryExchange or details.contract.exchange),
    )
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
            details.contract.lastTradeDateOrContractMonth, "%Y%m%d"
        ).date(),
        kind=kind,
        ts_event=timestamp,
        ts_init=timestamp,
    )


def parse_forex_contract(
    details: ContractDetails,
) -> CurrencyPair:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    instrument_id = InstrumentId(
        symbol=Symbol(f"{details.contract.symbol}/{details.contract.currency}"),
        venue=Venue(details.contract.primaryExchange or details.contract.exchange),
    )
    return CurrencyPair(
        instrument_id=instrument_id,
        native_symbol=Symbol(details.contract.localSymbol),
        base_currency=Currency.from_str(details.contract.currency),
        quote_currency=Currency.from_str(details.contract.symbol),
        price_precision=price_precision,
        size_precision=Quantity.from_int(1),
        price_increment=Price(details.minTick, price_precision),
        size_increment=Quantity(details.sizeMinTick or 1, 1),
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
    )
