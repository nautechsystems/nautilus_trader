#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Hyperliquid outcome market paper trading example.

This example runs with:
- Hyperliquid live outcome market data client
- Sandbox execution client (simulated fills)
- Python strategy (`ExecTester`) for paper-trading validation

"""

from __future__ import annotations

import asyncio
import os
from decimal import Decimal

from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidProductType
from nautilus_trader.adapters.hyperliquid.factories import get_cached_hyperliquid_http_client
from nautilus_trader.adapters.hyperliquid.paper import select_outcome_instrument_id
from nautilus_trader.adapters.sandbox.config import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox.factory import SandboxLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import HyperliquidEnvironment
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


async def discover_outcome_instrument_id(
    *,
    environment: HyperliquidEnvironment,
    preferred: str | None,
) -> InstrumentId:
    """
    Load currently listed Hyperliquid outcome instruments and choose one.
    """
    http_client = get_cached_hyperliquid_http_client(
        timeout_secs=15,
        environment=environment,
    )
    instruments = await http_client.load_instrument_definitions(
        include_spot=False,
        include_perps=False,
        include_perps_hip3=False,
        include_outcomes=True,
    )
    instrument_ids = [InstrumentId.from_str(inst.id.value) for inst in instruments]
    return select_outcome_instrument_id(instrument_ids, preferred=preferred)


def main() -> None:
    testnet = os.getenv("HYPERLIQUID_OUTCOME_TESTNET", "1").strip() == "1"
    environment = HyperliquidEnvironment.TESTNET if testnet else HyperliquidEnvironment.MAINNET
    preferred_instrument = os.getenv("HYPERLIQUID_OUTCOME_INSTRUMENT_ID")

    instrument_id = asyncio.run(
        discover_outcome_instrument_id(
            environment=environment,
            preferred=preferred_instrument,
        ),
    )

    config_node = TradingNodeConfig(
        trader_id=TraderId("PAPER-001"),
        logging=LoggingConfig(
            log_level="INFO",
            log_colors=True,
            use_pyo3=True,
        ),
        data_clients={
            HYPERLIQUID: HyperliquidDataClientConfig(
                environment=environment,
                instrument_provider=InstrumentProviderConfig(load_all=True),
                product_types=(HyperliquidProductType.OUTCOME,),
                update_outcome_instruments_on_expiry=True,
            ),
        },
        exec_clients={
            "SANDBOX": SandboxExecutionClientConfig(
                venue=HYPERLIQUID,
                base_currency="USDH",
                starting_balances=["10_000 USDH"],
                account_type="MARGIN",
                oms_type="NETTING",
            ),
        },
        timeout_connection=30.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=10.0,
        timeout_post_stop=5.0,
    )

    node = TradingNode(config=config_node)

    strategy = ExecTester(
        config=ExecTesterConfig(
            instrument_id=instrument_id,
            external_order_claims=[instrument_id],
            client_id=ClientId("SANDBOX"),
            order_qty=Decimal(100),
            open_position_on_start_qty=Decimal(100),
            open_position_time_in_force=TimeInForce.GTC,
            enable_limit_buys=True,
            enable_limit_sells=False,
            use_post_only=True,
            log_data=True,
        ),
    )
    node.trader.add_strategy(strategy)

    node.add_data_client_factory(HYPERLIQUID, HyperliquidLiveDataClientFactory)
    node.add_exec_client_factory("SANDBOX", SandboxLiveExecClientFactory)
    node.build()

    try:
        node.run(raise_exception=True)
    except KeyboardInterrupt:
        print("Keyboard interrupt received, shutting down...")
    finally:
        node.stop()
        node.dispose()


if __name__ == "__main__":
    main()
