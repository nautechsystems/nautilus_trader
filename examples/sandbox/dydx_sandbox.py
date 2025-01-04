#!/usr/bin/env python3
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

import asyncio
from decimal import Decimal

from nautilus_trader.adapters.dydx.config import DYDXDataClientConfig
from nautilus_trader.adapters.dydx.factories import DYDXLiveDataClientFactory
from nautilus_trader.adapters.sandbox.config import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox.factory import SandboxLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.volatility_market_maker import VolatilityMarketMaker
from nautilus_trader.examples.strategies.volatility_market_maker import VolatilityMarketMakerConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


async def main():
    """
    Show how to run a strategy in a sandbox for the dYdX venue.
    """
    instrument_provider_config = InstrumentProviderConfig(load_all=True)

    # Set up the execution clients (required per venue)
    venues = ["DYDX"]

    exec_clients = {}
    for venue in venues:
        exec_clients[venue] = SandboxExecutionClientConfig(
            venue=venue,
            starting_balances=["10_000 USDC", "10 ETH"],
            instrument_provider=instrument_provider_config,
            account_type="MARGIN",
            oms_type="NETTING",
        )

    # Configure the trading node
    config_node = TradingNodeConfig(
        trader_id=TraderId("TESTER-001"),
        logging=LoggingConfig(
            log_level="INFO",
            # log_level_file="DEBUG",
            # log_file_format="json",
            log_colors=True,
            use_pyo3=True,
        ),
        data_clients={
            "DYDX": DYDXDataClientConfig(
                wallet_address=None,  # 'DYDX_WALLET_ADDRESS' env var
                instrument_provider=instrument_provider_config,
                is_testnet=False,  # If client uses the testnet
            ),
        },
        exec_clients=exec_clients,
        timeout_connection=30.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=10.0,
        timeout_post_stop=5.0,
    )

    # Instantiate the node with a configuration
    node = TradingNode(config=config_node)

    # Configure your strategy
    strat_config = VolatilityMarketMakerConfig(
        instrument_id=InstrumentId.from_str("ETH-USD-PERP.DYDX"),
        external_order_claims=[InstrumentId.from_str("ETH-USD-PERP.DYDX")],
        bar_type=BarType.from_str("ETH-USD-PERP.DYDX-1-MINUTE-LAST-EXTERNAL"),
        atr_period=20,
        atr_multiple=6.0,
        trade_size=Decimal("0.010"),
        # manage_gtd_expiry=True,
    )
    # Instantiate your strategy
    strategy = VolatilityMarketMaker(config=strat_config)

    # Add your strategies and modules
    node.trader.add_strategy(strategy)

    # Register client factories with the node
    for data_client in config_node.data_clients:
        node.add_data_client_factory(data_client, DYDXLiveDataClientFactory)

    for exec_client in config_node.exec_clients:
        node.add_exec_client_factory(exec_client, SandboxLiveExecClientFactory)

    node.build()

    try:
        await node.run_async()
    finally:
        await node.stop_async()
        await asyncio.sleep(1)
        node.dispose()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    asyncio.run(main())
