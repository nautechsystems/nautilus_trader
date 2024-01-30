# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import time
from decimal import Decimal
from typing import Literal

import msgspec
import pandas as pd
from ibapi.contract import ContractDetails

# fmt: off
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import asset_class_from_str
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import OptionsContract
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


# fmt: on


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
    }
    return asset_class_from_str(mapping.get(sec_type, sec_type))


def contract_details_to_ib_contract_details(details: ContractDetails) -> IBContractDetails:
    details.contract = IBContract(**details.contract.__dict__)
    details = IBContractDetails(**details.__dict__)
    return details


def parse_instrument(
    contract_details: IBContractDetails,
) -> Instrument:
    security_type = contract_details.contract.secType
    if security_type == "STK":
        return parse_equity_contract(details=contract_details)
    elif security_type in ("FUT", "CONTFUT"):
        return parse_futures_contract(details=contract_details)
    elif security_type == "OPT":
        return parse_options_contract(details=contract_details)
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


def parse_futures_contract(
    details: IBContractDetails,
) -> FuturesContract:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    instrument_id = ib_contract_to_instrument_id(details.contract)
    expiration = pd.to_datetime(  # TODO: Check correctness
        details.contract.lastTradeDateOrContractMonth,
        format="%Y%m%d",
        utc=True,
    )
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


def parse_options_contract(
    details: IBContractDetails,
) -> OptionsContract:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    instrument_id = ib_contract_to_instrument_id(details.contract)
    asset_class = sec_type_to_asset_class(details.underSecType)
    option_kind = {
        "C": OptionKind.CALL,
        "P": OptionKind.PUT,
    }[details.contract.right]
    expiration = pd.to_datetime(  # TODO: Check correctness
        details.contract.lastTradeDateOrContractMonth,
        format="%Y%m%d",
        utc=True,
    )
    activation = expiration - pd.Timedelta(days=90)  # TODO: Make this more accurate

    return OptionsContract(
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
) -> CurrencyPair:
    price_precision: int = _tick_size_to_precision(details.minTick)
    size_precision: int = _tick_size_to_precision(details.minSize)
    timestamp = time.time_ns()
    instrument_id = ib_contract_to_instrument_id(details.contract)

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
) -> CryptoPerpetual:
    price_precision: int = _tick_size_to_precision(details.minTick)
    size_precision: int = _tick_size_to_precision(details.minSize)
    timestamp = time.time_ns()
    instrument_id = ib_contract_to_instrument_id(details.contract)

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


def ib_contract_to_instrument_id(contract: IBContract) -> InstrumentId:
    PyCondition.type(contract, IBContract, "IBContract")

    sec_type = contract.secType
    exchange = contract.exchange.replace(".", "/")

    if sec_type == "STK":
        symbol = f"{contract.localSymbol}={sec_type}"
        venue = contract.primaryExchange if exchange == "SMART" else exchange
    elif sec_type in ["CONTFUT"]:
        symbol = f"{contract.symbol}={sec_type}"
        venue = contract.exchange
    elif sec_type in ["CASH", "CRYPTO"]:
        symbol = f"{contract.localSymbol}={sec_type}" or f"{contract.symbol}.{contract.currency}"

        venue = contract.exchange
    elif sec_type in ["FUT", "FOP", "OPT"]:
        symbol = f"{contract.localSymbol}={sec_type}"
        venue = exchange
    else:
        symbol = None
        venue = None
    if symbol and venue:
        return InstrumentId(Symbol(symbol), Venue(venue))
    raise ValueError(f"Cannot parse {contract=} to InstrumentId")


def instrument_id_to_ib_contract(instrument_id: InstrumentId) -> IBContract:
    PyCondition.type(instrument_id, InstrumentId, "InstrumentId")
    parts = instrument_id.symbol.value.split("=")

    if len(parts) == 1:
        raise ValueError(f"{instrument_id} not in format symbol=secType")

    venue = str(instrument_id.venue.value).replace("/", ".")

    sec_type: Literal["CASH", "STK", "OPT", "FUT", "FOP", "CONTFUT", "CRYPTO", ""] = parts[1]

    if sec_type == "STK":
        return IBContract(
            secType="STK",
            exchange="SMART",
            primaryExchange=venue,
            localSymbol=parts[0],
        )
    if sec_type in ["CONTFUT"]:
        return IBContract(
            secType=sec_type,
            exchange=venue,
            symbol=parts[0],
        )
    elif sec_type in ["FUT", "FOP", "OPT", "CASH", "CRYPTO"]:
        return IBContract(
            secType=sec_type,
            exchange=venue,
            localSymbol=parts[0],
        )
    elif instrument_id.venue.value == "InteractiveBrokers":  # keep until a better approach
        # This will allow to make Instrument request using IBContract from within Strategy
        # and depending on the Strategy requirement
        return msgspec.json.decode(instrument_id.symbol.value, type=IBContract)

    # Default to Stock

    raise ValueError(f"Cannot parse {instrument_id} to IBContract, unknown security_type")
