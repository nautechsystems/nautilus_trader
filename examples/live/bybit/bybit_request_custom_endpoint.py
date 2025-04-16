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

import os
from datetime import timedelta

from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.factories import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit.factories import BybitLiveExecClientFactory
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickerData
from nautilus_trader.common import Environment
from nautilus_trader.common.events import TimeEvent
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.data import Data
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import DataType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# *** THIS INTEGRATION IS STILL UNDER CONSTRUCTION. ***
# *** CONSIDER IT TO BE IN AN UNSTABLE BETA PHASE AND EXERCISE CAUTION. ***


class RequestDemoStrategyConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    interval: int


class RequestDemoStrategy(Strategy):
    """
    Strategy showcases how to request custom data from bybit adapter. BybitTickerData is
    specific to Bybit adapter and you can request it with `request_data` method.

    Also this strategy demonstrate:
    - how to request BybitTickerData
    - how to use clock to schedule this request periodically by time interval in seconds.

    """

    def __init__(self, config: RequestDemoStrategyConfig):
        super().__init__()

    def on_start(self):
        seconds_delta = timedelta(seconds=self.config.interval)
        self.clock.set_timer(
            name="fetch_ticker",
            interval=seconds_delta,
            callback=self.send_tickers_request,
        )

    def send_tickers_request(self, time_event: TimeEvent) -> None:
        data_type = DataType(
            BybitTickerData,
            metadata={"symbol": self.config.instrument_id.symbol},
        )
        self.request_data(data_type, ClientId("BYBIT"))

    def on_historical_data(self, data: Data) -> None:
        if isinstance(data, BybitTickerData):
            self.log.info(f"{data}")


api_key = os.getenv("BYBIT_TESTNET_API_KEY")
api_secret = os.getenv("BYBIT_TESTNET_API_SECRET")

config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    environment=Environment.LIVE,
    logging=LoggingConfig(log_level="INFO"),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1440,
    ),
    data_clients={
        "BYBIT": BybitDataClientConfig(
            api_key=api_key,
            api_secret=api_secret,
            product_types=[BybitProductType.LINEAR],
            instrument_provider=InstrumentProviderConfig(load_all=True),
            testnet=True,
        ),
    },
    exec_clients={
        "BYBIT": BybitExecClientConfig(
            api_key=api_key,
            api_secret=api_secret,
            product_types=[BybitProductType.LINEAR],
            instrument_provider=InstrumentProviderConfig(load_all=True),
            testnet=True,
            max_retries=3,
            retry_delay_initial_ms=1_000,
            retry_delay_max_ms=10_000,
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

node = TradingNode(config=config_node)

instrument_id = InstrumentId.from_str("ETHUSDT-LINEAR.BYBIT")
strategy_config = RequestDemoStrategyConfig(
    instrument_id=instrument_id,
    interval=10,
)
strategy_config = RequestDemoStrategy(config=strategy_config)

node.trader.add_strategy(strategy_config)
node.add_data_client_factory("BYBIT", BybitLiveDataClientFactory)
node.add_exec_client_factory("BYBIT", BybitLiveExecClientFactory)
node.build()

if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
