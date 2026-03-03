from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
import json
import math
from types import MappingProxyType
from typing import Any
from typing import TypeAlias


TOPIC_STATE = "maker_poc.state"
TOPIC_EVENT = "maker_poc.event"
TOPIC_TRADE = "maker_poc.trade"
TOPIC_ALERT = "maker_poc.alert"
TOPIC_MARKET_BBO = "maker_poc.market_bbo"
TOPIC_FV = "maker_poc.fv"
TOPIC_BALANCES = "maker_poc.balances"

STRATEGY_ID = "bybit_binance_plumeusdt_makerv3"

JSONPrimitive: TypeAlias = None | bool | int | float | str
JSONValue: TypeAlias = JSONPrimitive | list["JSONValue"] | dict[str, "JSONValue"]


@dataclass(frozen=True, slots=True)
class InstrumentContract:
    instrument_id: str
    chainsaw_exchange: str
    chainsaw_symbol: str


INSTRUMENT_CONTRACTS: tuple[InstrumentContract, ...] = (
    InstrumentContract(
        instrument_id="PLUMEUSDT-LINEAR.BYBIT",
        chainsaw_exchange="bybit_linear",
        chainsaw_symbol="plume/usdt",
    ),
    InstrumentContract(
        instrument_id="PLUMEUSDT.BINANCE",
        chainsaw_exchange="binance_spot",
        chainsaw_symbol="plume/usdt",
    ),
)

INSTRUMENT_CONTRACTS_BY_ID: Mapping[str, InstrumentContract] = MappingProxyType(
    {item.instrument_id: item for item in INSTRUMENT_CONTRACTS},
)


def json_dumps_compact(obj: Any) -> str:
    normalized = _to_json_value(obj)
    return json.dumps(
        normalized,
        allow_nan=False,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    )


def get_instrument_contract(instrument_id: str) -> InstrumentContract:
    try:
        return INSTRUMENT_CONTRACTS_BY_ID[instrument_id]
    except KeyError as exc:
        msg = f"Unknown instrument_id '{instrument_id}'."
        raise KeyError(msg) from exc


def make_last_key_component(chainsaw_exchange: str, chainsaw_symbol: str) -> str:
    base, quote = _split_symbol(chainsaw_symbol)
    return f"last:{chainsaw_exchange}:{base}_{quote}"


def make_last_key_component_for_instrument(instrument_id: str) -> str:
    contract = get_instrument_contract(instrument_id)
    return make_last_key_component(contract.chainsaw_exchange, contract.chainsaw_symbol)


def make_fv_coin(chainsaw_symbol: str) -> str:
    base, quote = _split_symbol(chainsaw_symbol)
    return f"{base.lower()}/{quote.lower()}"


def make_fv_coin_for_instrument(instrument_id: str) -> str:
    contract = get_instrument_contract(instrument_id)
    return make_fv_coin(contract.chainsaw_symbol)


def _split_symbol(chainsaw_symbol: str) -> tuple[str, str]:
    parts = chainsaw_symbol.split("/", maxsplit=1)
    if len(parts) != 2 or not parts[0] or not parts[1]:
        msg = (
            "Invalid chainsaw_symbol format "
            f"'{chainsaw_symbol}', expected '<base>/<quote>'."
        )
        raise ValueError(msg)
    return parts[0].upper(), parts[1].upper()


def _to_json_value(value: Any, *, path: str = "$") -> JSONValue:
    if value is None or isinstance(value, str | bool | int):
        return value

    if isinstance(value, float):
        if not math.isfinite(value):
            msg = f"Non-finite float at {path}: {value!r}"
            raise ValueError(msg)
        return value

    if isinstance(value, list | tuple):
        return [_to_json_value(item, path=f"{path}[{index}]") for index, item in enumerate(value)]

    if isinstance(value, Mapping):
        normalized: dict[str, JSONValue] = {}
        for key, item in value.items():
            if not isinstance(key, str):
                msg = f"Unsupported dict key type at {path}: {type(key).__name__}"
                raise TypeError(msg)
            normalized[key] = _to_json_value(item, path=f"{path}.{key}")
        return normalized

    msg = f"Unsupported value type at {path}: {type(value).__name__}"
    raise TypeError(msg)


__all__ = [
    "INSTRUMENT_CONTRACTS",
    "INSTRUMENT_CONTRACTS_BY_ID",
    "STRATEGY_ID",
    "TOPIC_ALERT",
    "TOPIC_BALANCES",
    "TOPIC_EVENT",
    "TOPIC_FV",
    "TOPIC_MARKET_BBO",
    "TOPIC_STATE",
    "TOPIC_TRADE",
    "InstrumentContract",
    "get_instrument_contract",
    "json_dumps_compact",
    "make_fv_coin",
    "make_fv_coin_for_instrument",
    "make_last_key_component",
    "make_last_key_component_for_instrument",
]
