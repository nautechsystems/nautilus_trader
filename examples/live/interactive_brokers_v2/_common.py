#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import datetime as dt
import json
import os
import signal
import socket
import subprocess
from collections.abc import Sequence
from typing import Any

from nautilus_trader.core import nautilus_pyo3 as pyo3


IB = "IB"
DEFAULT_HOST = "127.0.0.1"
DEFAULT_TWS_PORT = 7497
FUTURES_MONTH_CODES = {
    1: "F",
    2: "G",
    3: "H",
    4: "J",
    5: "K",
    6: "M",
    7: "N",
    8: "Q",
    9: "U",
    10: "V",
    11: "X",
    12: "Z",
}
QUARTERLY_CONTRACT_MONTHS = (3, 6, 9, 12)


def env_bool(name: str, default: bool = False) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.lower() in {"1", "true", "yes", "y"}


def env_int(name: str, default: int) -> int:
    return int(os.getenv(name, str(default)))


def resolve_ib_endpoint() -> tuple[str, int]:
    host = os.getenv("IB_V2_HOST") or os.getenv("IB_PYO3_HOST") or DEFAULT_HOST
    port = int(os.getenv("IB_V2_PORT") or os.getenv("IB_PYO3_PORT") or DEFAULT_TWS_PORT)
    return host, port


def is_ib_endpoint_reachable(host: str, port: int, timeout: float = 2.0) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def schedule_node_stop(node: object, delay_seconds: int) -> None:
    if delay_seconds <= 0:
        return

    subprocess.Popen(  # noqa: S603
        ["/bin/sh", "-c", f"sleep {delay_seconds}; kill -{signal.SIGINT} {os.getpid()}"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def ib_account_id(raw_account_id: str) -> pyo3.AccountId:
    if "-" in raw_account_id:
        return pyo3.AccountId.from_str(raw_account_id)
    return pyo3.AccountId.from_str(f"{IB}-{raw_account_id}")


def contract_month_code(year: int, month: int) -> str:
    return f"{FUTURES_MONTH_CODES[month]}{year % 10}"


def third_friday(year: int, month: int) -> dt.date:
    first_day = dt.date(year, month, 1)
    first_friday_offset = (4 - first_day.weekday()) % 7
    return first_day + dt.timedelta(days=first_friday_offset + 14)


def active_quarterly_contract(
    *,
    symbol: str,
    venue: str,
    today: dt.date | None = None,
    min_days_to_expiry: int = 45,
) -> tuple[str, str, str]:
    today = today or dt.date.today()
    target_expiry = today + dt.timedelta(days=min_days_to_expiry)
    year = today.year

    while True:
        for month in QUARTERLY_CONTRACT_MONTHS:
            expiry = third_friday(year, month)
            if expiry >= target_expiry:
                local_symbol = f"{symbol}{contract_month_code(year, month)}"
                return local_symbol, f"{local_symbol}.{venue}", expiry.strftime("%Y%m%d")
        year += 1


def active_monthly_contract(
    *,
    symbol: str,
    venue: str,
    today: dt.date | None = None,
    min_days_to_contract_month: int = 45,
) -> tuple[str, str, str]:
    today = today or dt.date.today()
    target_month = today + dt.timedelta(days=min_days_to_contract_month)
    year = today.year
    month = today.month

    while dt.date(year, month, 1) < target_month:
        month += 1
        if month > 12:
            month = 1
            year += 1

    local_symbol = f"{symbol}{contract_month_code(year, month)}"
    return local_symbol, f"{local_symbol}.{venue}", f"{year}{month:02d}"


def default_es_future() -> tuple[str, str, str]:
    return active_quarterly_contract(symbol="ES", venue="XCME")


def default_ym_future() -> tuple[str, str, str]:
    return active_quarterly_contract(symbol="YM", venue="XCBT")


def default_cl_future() -> tuple[str, str, str]:
    return active_monthly_contract(symbol="CL", venue="XNYM")


def default_es_future_instrument_id() -> str:
    return default_es_future()[1]


def default_ym_future_instrument_id() -> str:
    return default_ym_future()[1]


def default_cl_future_instrument_id() -> str:
    return default_cl_future()[1]


def format_option_strike(strike: float) -> str:
    strike_value = float(strike)
    return str(int(strike_value)) if strike_value.is_integer() else str(strike_value)


def default_es_put_option_local_symbol(strike: float = 6800.0) -> str:
    local_symbol, _, _ = default_es_future()
    return f"{local_symbol} P{format_option_strike(strike)}"


def default_es_put_option_instrument_id(strike: float = 6800.0) -> str:
    return f"{default_es_put_option_local_symbol(strike)}.XCME"


def default_es_put_spread_instrument_id(
    long_strike: float = 6800.0,
    short_strike: float = 6750.0,
) -> str:
    # IB generic spread IDs encode signed leg ratios in the instrument ID
    leg_ratios = [
        (default_es_put_option_local_symbol(long_strike), 1),
        (default_es_put_option_local_symbol(short_strike), -1),
    ]
    leg_ratios.sort(key=lambda value: value[0])

    symbol_parts = []

    for symbol, ratio in leg_ratios:
        if ratio > 0:
            symbol_parts.append(f"({ratio}){symbol}")
        else:
            symbol_parts.append(f"(({abs(ratio)})){symbol}")

    return f"{'_'.join(symbol_parts)}.XCME"


def default_stock_contracts() -> list[dict[str, str]]:
    ib = pyo3.interactive_brokers
    return [
        {
            "secType": ib.IbSecurityType.STOCK.as_str(),
            "symbol": "AAPL",
            "exchange": "SMART",
            "primaryExchange": "NASDAQ",
            "currency": "USD",
        },
        {
            "secType": ib.IbSecurityType.STOCK.as_str(),
            "symbol": "MSFT",
            "exchange": "SMART",
            "primaryExchange": "NASDAQ",
            "currency": "USD",
        },
        {
            "secType": ib.IbSecurityType.STOCK.as_str(),
            "symbol": "TSLA",
            "exchange": "SMART",
            "primaryExchange": "NASDAQ",
            "currency": "USD",
        },
    ]


def futures_contract(
    *,
    symbol: str = "ES",
    exchange: str = "CME",
    local_symbol: str | None = None,
    expiry: str | None = None,
) -> dict[str, object]:
    ib = pyo3.interactive_brokers
    default_local_symbol, _, default_expiry = default_es_future()
    return {
        "secType": ib.IbSecurityType.FUTURE.as_str(),
        "symbol": symbol,
        "exchange": exchange,
        "localSymbol": local_symbol or default_local_symbol,
        "lastTradeDateOrContractMonth": expiry or default_expiry,
        "currency": "USD",
    }


def option_contract(
    *,
    symbol: str = "ES",
    exchange: str = "CME",
    local_symbol: str | None = None,
    expiry: str | None = None,
    right: Any | None = None,
    strike: float | None = None,
) -> dict[str, object]:
    ib = pyo3.interactive_brokers
    right = right or ib.IbOptionRight.PUT
    right_value = right.as_str() if hasattr(right, "as_str") else str(right)
    default_local_symbol = default_es_put_option_local_symbol(strike or 6800.0)
    _, _, default_expiry = default_es_future()
    contract: dict[str, object] = {
        "secType": ib.IbSecurityType.FUTURES_OPTION.as_str(),
        "symbol": symbol,
        "exchange": exchange,
        "localSymbol": local_symbol or default_local_symbol,
        "lastTradeDateOrContractMonth": expiry or default_expiry,
        "right": right_value,
        "currency": "USD",
    }

    if strike is not None:
        contract["strike"] = strike

    return contract


def ib_order_tags(**values: object) -> str:
    return "IBOrderTags:" + json.dumps(values, separators=(",", ":"), sort_keys=True)


def add_strategy_from_config(node: object, strategy_path: str) -> None:
    node.add_strategy_from_config(  # type: ignore[attr-defined]
        pyo3.ImportableStrategyConfig(  # type: ignore[attr-defined]
            strategy_path=strategy_path,
            config_path="",
            config={},
        ),
    )


def instrument_ids(values: Sequence[str]) -> list[pyo3.InstrumentId]:
    return [pyo3.InstrumentId.from_str(value) for value in values]


def instrument_provider_config(
    load_ids: Sequence[str] | None = None,
    load_contracts: Sequence[dict[str, object]] | None = None,
    symbol_to_mic_venue: dict[str, str] | None = None,
) -> object:
    ib = pyo3.interactive_brokers
    return ib.InteractiveBrokersInstrumentProviderConfig(
        symbology_method=ib.SymbologyMethod.SIMPLIFIED,
        load_ids=set(instrument_ids(load_ids or ())),
        load_contracts=list(load_contracts or ()),
        build_options_chain=False,
        build_futures_chain=False,
        symbol_to_mic_venue=symbol_to_mic_venue or {},
    )


def build_ib_live_node(
    *,
    name: str,
    trader_id: str,
    host: str,
    port: int,
    data_client_id: int,
    exec_client_id: int | None = None,
    account_id: str | None = None,
    provider_config: object | None = None,
) -> object:
    ib = pyo3.interactive_brokers
    trader = pyo3.TraderId.from_str(trader_id)
    provider_config = provider_config or instrument_provider_config()

    builder = pyo3.live.LiveNode.builder(name, trader, pyo3.Environment.LIVE)  # type: ignore[attr-defined]
    builder = builder.with_timeout_connection(env_int("IB_V2_NODE_CONNECTION_TIMEOUT", 15))
    builder = builder.with_timeout_reconciliation(5)
    builder = builder.with_timeout_portfolio(5)
    builder = builder.with_timeout_disconnection_secs(5)
    builder = builder.with_delay_post_stop_secs(2)
    builder = builder.with_reconciliation(env_bool("IB_V2_RECONCILIATION", False))
    builder = builder.add_data_client(
        None,
        ib.InteractiveBrokersDataClientFactory(),
        ib.InteractiveBrokersDataClientConfig(
            host=host,
            port=port,
            client_id=data_client_id,
            connection_timeout=env_int("IB_V2_CONNECTION_TIMEOUT", 10),
            request_timeout=env_int("IB_V2_REQUEST_TIMEOUT", 30),
            market_data_type=ib.MarketDataType.DELAYED_FROZEN,
            instrument_provider=provider_config,
        ),
    )

    if account_id is not None:
        builder = builder.add_exec_client(
            None,
            ib.InteractiveBrokersExecutionClientFactory(trader, ib_account_id(account_id)),
            ib.InteractiveBrokersExecClientConfig(
                host=host,
                port=port,
                client_id=exec_client_id or data_client_id,
                account_id=account_id,
                connection_timeout=env_int("IB_V2_CONNECTION_TIMEOUT", 10),
                request_timeout=env_int("IB_V2_REQUEST_TIMEOUT", 30),
                fetch_all_open_orders=False,
                instrument_provider=provider_config,
            ),
        )

    return builder.build()
