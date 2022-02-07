import datetime
import time

from ib_insync import ContractDetails

from nautilus_trader.model.c_enums.asset_class import AssetClassParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments.base import Instrument
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
    instrument_id: InstrumentId,
    contract_details: ContractDetails,
) -> Instrument:
    security_type = contract_details.contract.secType
    if security_type == "STK":
        return parse_equity_contract(instrument_id=instrument_id, details=contract_details)
    elif security_type == "FUT":
        return parse_future_contract(instrument_id=instrument_id, details=contract_details)
    else:
        raise ValueError(f"Unknown {security_type=}")


def parse_equity_contract(
    instrument_id: InstrumentId,
    details: ContractDetails,
) -> Equity:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    equity = Equity(
        instrument_id=instrument_id,
        local_symbol=Symbol(details.contract.localSymbol),
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
    return equity


def parse_future_contract(
    instrument_id: InstrumentId,
    details: ContractDetails,
) -> Future:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    future = Future(
        instrument_id=instrument_id,
        local_symbol=Symbol(details.contract.localSymbol),
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

    return future


def parse_option_contract(
    instrument_id: InstrumentId,
    asset_class: AssetClass,
    details: ContractDetails,
) -> Option:
    price_precision: int = _tick_size_to_precision(details.minTick)
    timestamp = time.time_ns()
    future = Option(
        instrument_id=instrument_id,
        local_symbol=Symbol(details.contract.localSymbol),
        asset_class=asset_class,
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

    return future
