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
import logging

from nautilus_trader.adapters.binance import BinanceAccountType
from nautilus_trader.adapters.binance import BinanceLiveExecClientFactory
from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

async def main():
    # Configure the trading node
    config_node = TradingNodeConfig(
        trader_id=TraderId("TESTER-001"),
        logging=LoggingConfig(log_level="INFO"),
        exec_clients={
            "BINANCE": {
                "factory": BinanceLiveExecClientFactory,
                "config": {
                    "account_type": BinanceAccountType.PORTFOLIO_MARGIN,
                    "api_key": None,  # 'BINANCE_API_KEY' env var
                    "api_secret": None,  # 'BINANCE_API_SECRET' env var
                    "testnet": False,  # If you want to test with testnet
                    "us": False,  # If you are using binance.us
                },
            },
        },
        data_clients={
            "BINANCE": {
                "factory": BinanceLiveDataClientFactory,
                "config": {
                    "account_type": BinanceAccountType.PORTFOLIO_MARGIN,
                    "api_key": None,  # 'BINANCE_API_KEY' env var
                    "api_secret": None,  # 'BINANCE_API_SECRET' env var
                    "testnet": False,
                    "us": False,
                },
            },
        },
        strategies=[
            {
                "strategy": EMACross,
                "config": EMACrossConfig(
                    instrument_id="BTCUSDT-PERP.BINANCE",
                    bar_type="BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL",
                    fast_ema_period=10,
                    slow_ema_period=20,
                    trade_size="0.01",
                ),
            },
        ],
        timeout_connection=20.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=10.0,
    )

    # Build the trading node
    node = TradingNode(config=config_node)

    try:
        # Start the trading node
        await node.start_async()

        # Wait until interrupted
        await asyncio.sleep(300)  # Run for 5 minutes

    except KeyboardInterrupt:
        pass
    finally:
        # Stop the trading node
        await node.stop_async()


if __name__ == "__main__":
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    )
    asyncio.run(main())
