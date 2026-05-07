#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _common import add_strategy_from_config
from _common import build_ib_live_node
from _common import env_bool
from _common import env_int
from _common import futures_contract
from _common import ib_order_tags
from _common import instrument_provider_config
from _common import option_contract
from _common import resolve_ib_endpoint
from _common import schedule_node_stop

from nautilus_trader.core import nautilus_pyo3 as pyo3


ProviderConfigFactory = Callable[[], object]
BeforeBuildHook = Callable[[], None]
AfterBuildHook = Callable[[Any], None]


class OrderExample:
    def __init__(
        self,
        *,
        node_id: str,
        strategy_path: str,
        default_data_client_id: int,
        default_exec_client_id: int,
        provider_config: ProviderConfigFactory,
        before_build: BeforeBuildHook | None = None,
        after_build: AfterBuildHook | None = None,
        auto_stop_seconds: int = 20,
    ) -> None:
        self.node_id = node_id
        self.strategy_path = strategy_path
        self.default_data_client_id = default_data_client_id
        self.default_exec_client_id = default_exec_client_id
        self.provider_config = provider_config
        self.before_build = before_build
        self.after_build = after_build
        self.auto_stop_seconds = auto_stop_seconds


def futures_provider_config() -> object:
    return instrument_provider_config(
        load_contracts=[futures_contract()],
        symbol_to_mic_venue={"ES": "IB"},
    )


def spread_provider_config() -> object:
    ib = pyo3.interactive_brokers
    leg_contracts = [
        option_contract(local_symbol="ESM6 P6800", right=ib.IbOptionRight.PUT, strike=6800.0),
        option_contract(local_symbol="ESM6 P6775", right=ib.IbOptionRight.PUT, strike=6775.0),
    ]
    return instrument_provider_config(
        load_contracts=leg_contracts,
        symbol_to_mic_venue={"ES": "IB"},
    )


def databento_provider_config() -> object:
    return instrument_provider_config(
        load_ids=[
            os.getenv("IB_V2_DATABENTO_INSTRUMENT_ID", "YMM6.XCBT"),
        ],
    )


def set_databento_request_contracts_default() -> None:
    ib = pyo3.interactive_brokers
    os.environ.setdefault(
        "IB_V2_DATABENTO_REQUEST_CONTRACTS",
        json.dumps(
            [
                {
                    "secType": ib.IbSecurityType.STOCK.as_str(),
                    "symbol": "SPY",
                    "exchange": "SMART",
                    "primaryExchange": "CBOE",
                    "build_options_chain": True,
                    "min_expiry_days": 0,
                    "max_expiry_days": 3,
                },
            ],
            separators=(",", ":"),
        ),
    )


def print_bracket_details(_node: object) -> None:
    print(
        "Built v2 bracket-order node and registered BracketOrderStrategy for ESM6.IB.",
        flush=True,
    )
    print(
        "Set IB_V2_ENABLE_ORDER_SUBMISSION=1 and IB_V2_RUN_NODE=1 to submit orders.",
        flush=True,
    )


def print_market_details(_node: object) -> None:
    print(
        "Built v2 market-order node and registered MarketOrderStrategy for ESM6.IB.",
        flush=True,
    )
    print(
        "Set IB_V2_MARKET_SIDE, IB_V2_ENABLE_ORDER_SUBMISSION=1, and IB_V2_RUN_NODE=1 to submit.",
        flush=True,
    )


def print_oca_details(node: Any) -> None:
    ib = pyo3.interactive_brokers
    oca_group = os.getenv("IB_V2_OCA_GROUP", "<unique per run>")
    tag = ib_order_tags(ocaGroup=oca_group, ocaType=ib.IbOcaType.CANCEL_WITH_BLOCK.as_i32())
    print(f"Built v2 OCA node and registered OcaGroupStrategy: {node.trader_id}", flush=True)
    print(f"Strategy submits both child orders with tag when enabled: {tag}", flush=True)


def print_conditions_details(node: Any) -> None:
    ib = pyo3.interactive_brokers
    time_condition: dict[str, Any] = {
        "type": ib.IbConditionKind.TIME.as_str(),
        "time": (dt.datetime.now(dt.UTC) + dt.timedelta(minutes=5)).strftime("%Y%m%d-%H:%M:%S"),
        "isMore": True,
        "conjunction": ib.IbConditionConjunction.AND.as_str(),
    }
    conditions = [time_condition]
    if env_bool("IB_V2_ENABLE_PRICE_CONDITION"):
        conditions.append(
            {
                "type": ib.IbConditionKind.PRICE.as_str(),
                "conId": env_int("IB_V2_CONDITION_CONTRACT_ID", 0),
                "exchange": "CME",
                "isMore": True,
                "price": 6000.0,
                "triggerMethod": ib.IbTriggerMethod.DEFAULT.as_i32(),
                "conjunction": ib.IbConditionConjunction.AND.as_str(),
            },
        )

    tag = ib_order_tags(
        conditions=conditions,
        conditionsCancelOrder=env_bool("IB_V2_CONDITIONS_CANCEL_ORDER", False),
    )

    print(
        f"Built v2 conditional-order node and registered SimpleConditionsStrategy: {node.trader_id}",
        flush=True,
    )
    print(f"Strategy submits the conditional tag when enabled: {tag}", flush=True)


def print_spread_details(_node: object) -> None:
    print(
        "Built v2 spread node and registered SpreadOrderStrategy with 2 option legs.",
        flush=True,
    )
    print(
        "Set IB_V2_ENABLE_ORDER_SUBMISSION=1 and IB_V2_RUN_NODE=1 to submit the spread payload.",
        flush=True,
    )


def print_databento_details(node: Any) -> None:
    print(
        f"Built v2 IB node and registered DatabentoInstrumentIdStrategy: {node.trader_id}",
        flush=True,
    )


ORDER_EXAMPLES = {
    "bracket": OrderExample(
        node_id="IB-V2-BRACKET-001",
        strategy_path="ib_v2_order_strategies:BracketOrderStrategy",
        default_data_client_id=1401,
        default_exec_client_id=1402,
        provider_config=futures_provider_config,
        after_build=print_bracket_details,
    ),
    "market": OrderExample(
        node_id="IB-V2-MARKET-001",
        strategy_path="ib_v2_order_strategies:MarketOrderStrategy",
        default_data_client_id=1511,
        default_exec_client_id=1512,
        provider_config=futures_provider_config,
        after_build=print_market_details,
    ),
    "oca": OrderExample(
        node_id="IB-V2-OCA-001",
        strategy_path="ib_v2_order_strategies:OcaGroupStrategy",
        default_data_client_id=1411,
        default_exec_client_id=1412,
        provider_config=futures_provider_config,
        after_build=print_oca_details,
    ),
    "conditions": OrderExample(
        node_id="IB-V2-CONDITIONS-001",
        strategy_path="ib_v2_order_strategies:SimpleConditionsStrategy",
        default_data_client_id=1421,
        default_exec_client_id=1422,
        provider_config=futures_provider_config,
        after_build=print_conditions_details,
    ),
    "spread": OrderExample(
        node_id="IB-V2-SPREAD-001",
        strategy_path="ib_v2_order_strategies:SpreadOrderStrategy",
        default_data_client_id=111,
        default_exec_client_id=112,
        provider_config=spread_provider_config,
        after_build=print_spread_details,
    ),
    "databento": OrderExample(
        node_id="IB-V2-DB-ID-001",
        strategy_path="ib_v2_order_strategies:DatabentoInstrumentIdStrategy",
        default_data_client_id=2,
        default_exec_client_id=3,
        provider_config=databento_provider_config,
        before_build=set_databento_request_contracts_default,
        after_build=print_databento_details,
        auto_stop_seconds=30,
    ),
}


def run_order_example(strategy: str) -> None:
    example = ORDER_EXAMPLES[strategy]
    if example.before_build is not None:
        example.before_build()

    host, port = resolve_ib_endpoint()
    account_id = (
        os.getenv("TWS_ACCOUNT")
        if env_bool("IB_V2_ENABLE_EXECUTION") or env_bool("IB_V2_ENABLE_ORDER_SUBMISSION")
        else None
    )
    node = build_ib_live_node(
        name=example.node_id,
        trader_id=example.node_id,
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", example.default_data_client_id),
        exec_client_id=env_int("IB_V2_EXEC_CLIENT_ID", example.default_exec_client_id),
        account_id=account_id,
        provider_config=example.provider_config(),
    )
    add_strategy_from_config(node, example.strategy_path)

    if example.after_build is not None:
        example.after_build(node)

    if env_bool("IB_V2_RUN_NODE"):
        schedule_node_stop(
            node,
            env_int("IB_V2_AUTO_STOP_SECONDS", example.auto_stop_seconds),
        )
        node.run()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--strategy",
        choices=ORDER_EXAMPLES.keys(),
        default=os.getenv("IB_V2_ORDER_EXAMPLE", "market"),
    )
    args = parser.parse_args()
    run_order_example(args.strategy)


if __name__ == "__main__":
    main()
