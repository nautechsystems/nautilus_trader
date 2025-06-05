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
import json
from decimal import Decimal

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.futures.types import BinanceFuturesMarkPriceUpdate
from nautilus_trader.adapters.sandbox.config import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox.factory import SandboxLiveExecClientFactory
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.data import Data
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.trading import Strategy
from nautilus_trader.trading.config import StrategyConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class TestStrategyConfig(StrategyConfig, frozen=True):
    futures_client_id: ClientId
    futures_instrument_id: InstrumentId
    spot_instrument_id: InstrumentId


class TestStrategy(Strategy):
    def __init__(self, config: TestStrategyConfig) -> None:
        super().__init__(config)

        self.futures_instrument: Instrument | None = None  # Initialized in on_start
        self.spot_instrument: Instrument | None = None  # Initialized in on_start
        self.futures_client_id = config.futures_client_id

    def on_start(self) -> None:
        self.futures_instrument = self.cache.instrument(self.config.futures_instrument_id)
        if self.futures_instrument is None:
            self.log.error(
                f"Could not find instrument for {self.config.futures_instrument_id}"
                f"\nPossible instruments: {self.cache.instrument_ids()}",
            )
            self.stop()
            return
        self.spot_instrument = self.cache.instrument(self.config.spot_instrument_id)
        if self.spot_instrument is None:
            self.log.error(
                f"Could not find futures instrument for {self.config.spot_instrument_id}"
                f"\nPossible instruments: {self.cache.instrument_ids()}",
            )
            self.stop()
            return

        account = self.portfolio.account(venue=self.futures_instrument.venue)
        balances = {str(currency): str(balance) for currency, balance in account.balances().items()}
        self.log.info(f"Futures balances\n{json.dumps(balances, indent=4)}", LogColor.GREEN)
        account = self.portfolio.account(venue=self.spot_instrument.venue)
        balances = {str(currency): str(balance) for currency, balance in account.balances().items()}
        self.log.info(f"Spot balances\n{json.dumps(balances, indent=4)}", LogColor.GREEN)

        # Subscribe to live data
        self.subscribe_quote_ticks(self.config.futures_instrument_id)
        self.subscribe_quote_ticks(self.config.spot_instrument_id)
        self.subscribe_data(
            data_type=DataType(
                BinanceFuturesMarkPriceUpdate,
                metadata={"instrument_id": self.futures_instrument.id},
            ),
            client_id=self.futures_client_id,
        )

    def on_data(self, data: Data) -> None:
        self.log.info(repr(data), LogColor.CYAN)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        self.log.info(repr(tick), LogColor.CYAN)

    def on_trade_tick(self, tick: TradeTick) -> None:
        self.log.info(repr(tick), LogColor.CYAN)

    def on_bar(self, bar: Bar) -> None:
        self.log.info(repr(bar), LogColor.CYAN)

    def on_stop(self) -> None:
        # Unsubscribe from data
        self.unsubscribe_quote_ticks(self.config.futures_instrument_id)
        self.unsubscribe_quote_ticks(self.config.spot_instrument_id)


async def main():
    """
    Show how to run a strategy in a sandbox for the Binance venue.
    """
    # Configure the trading node
    config_node = TradingNodeConfig(
        trader_id=TraderId("TESTER-001"),
        logging=LoggingConfig(
            log_level="INFO",
            log_colors=True,
            use_pyo3=True,
        ),
        exec_engine=LiveExecEngineConfig(
            reconciliation=True,
            reconciliation_lookback_mins=1440,
            filter_position_reports=True,
        ),
        cache=CacheConfig(
            timestamps_as_iso8601=True,
            flush_on_start=False,
        ),
        data_clients={
            "BINANCE_FUTURES": BinanceDataClientConfig(
                venue=Venue("BINANCE_FUTURES"),
                api_key=None,  # 'BINANCE_API_KEY' env var
                api_secret=None,  # 'BINANCE_API_SECRET' env var
                account_type=BinanceAccountType.USDT_FUTURE,
                base_url_http=None,  # Override with custom endpoint
                base_url_ws=None,  # Override with custom endpoint
                us=False,  # If client is for Binance US
                testnet=False,  # If client uses the testnet
                instrument_provider=InstrumentProviderConfig(load_all=True),
            ),
            "BINANCE_SPOT": BinanceDataClientConfig(
                venue=Venue("BINANCE_SPOT"),
                api_key=None,  # 'BINANCE_API_KEY' env var
                api_secret=None,  # 'BINANCE_API_SECRET' env var
                account_type=BinanceAccountType.SPOT,
                base_url_http=None,  # Override with custom endpoint
                base_url_ws=None,  # Override with custom endpoint
                us=False,  # If client is for Binance US
                testnet=False,  # If client uses the testnet
                instrument_provider=InstrumentProviderConfig(load_all=True),
            ),
        },
        exec_clients={
            "BINANCE_FUTURES": SandboxExecutionClientConfig(
                venue="BINANCE_FUTURES",
                account_type="MARGIN",
                starting_balances=["10_000 USDC", "0.005 BTC"],
                default_leverage=Decimal("5"),
            ),
            "BINANCE_SPOT": SandboxExecutionClientConfig(
                venue="BINANCE_SPOT",
                account_type="CASH",
                starting_balances=["1_000 USDC", "0.001 BTC"],
            ),
        },
        timeout_connection=30.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=10.0,
        timeout_post_stop=5.0,
    )

    # Instantiate the node with a configuration
    node = TradingNode(config=config_node)

    # Configure your strategy
    strat_config = TestStrategyConfig(
        futures_client_id=ClientId("BINANCE_FUTURES"),
        futures_instrument_id=InstrumentId.from_str("BTCUSDT-PERP.BINANCE_FUTURES"),
        spot_instrument_id=InstrumentId.from_str("BTCUSDC.BINANCE_SPOT"),
    )
    # Instantiate your strategy
    strategy = TestStrategy(config=strat_config)

    # Add your strategies and modules
    node.trader.add_strategy(strategy)

    # Register your client factories with the node (can take user-defined factories)
    node.add_data_client_factory("BINANCE_FUTURES", BinanceLiveDataClientFactory)
    node.add_data_client_factory("BINANCE_SPOT", BinanceLiveDataClientFactory)
    node.add_exec_client_factory("BINANCE_FUTURES", SandboxLiveExecClientFactory)
    node.add_exec_client_factory("BINANCE_SPOT", SandboxLiveExecClientFactory)
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
