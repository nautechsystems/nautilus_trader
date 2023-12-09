#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.factories import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit.factories import BybitLiveExecClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.common.clock import TimeEvent
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import InstrumentId

from nautilus_trader.core.uuid import UUID4

from nautilus_trader.core.message import Request
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.live.node import TradingNode
from nautilus_trader.trading import Strategy
"""
    This strategy demonstrates how to request data from a custom bybit endpoint.
    Some adapter endpoints are not connected to data engine, so they cannot be queried.
    We can use the message bus to request data from these endpoints, as they are registered
    in appropriate clients.
    Also this strategy demonstrate:
     - how to use the message bus to request data from a custom endpoint.
     - how to use clock to schedule a this request periodically.
"""

# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# *** THIS INTEGRATION IS STILL UNDER CONSTRUCTION. ***
# *** CONSIDER IT TO BE IN AN UNSTABLE BETA PHASE AND EXERCISE CAUTION. ***


class RequestDemoStrategyConfig(StrategyConfig, frozen=True):
    instrument_id: str
    interval: int


class RequestDemoStrategy(Strategy):
    def __init__(self, config: RequestDemoStrategyConfig):
        super().__init__()
        self.interval = config.interval
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.type_dict_uuid = dict()
        self.linear_request_ticker_uuid = UUID4()

    def start(self):
        seconds_delta = timedelta(seconds=self.interval)
        self.clock.set_timer(name='fetch_ticker', interval=seconds_delta, callback=self.send_tickers_request)

    def send_tickers_request(self, time_event: TimeEvent):
        request = Request(
            request_id=self.linear_request_ticker_uuid,
            ts_init=self.clock.timestamp_ns(),
            callback=self.on_data,
            metadata=dict(symbol=self.instrument_id.symbol)
        )
        self.msgbus.request(endpoint="bybit.data.tickers", request=request)

    def on_data(self, data: DataResponse):
        ## check generic data response by uuid
        if data.correlation_id == self.linear_request_ticker_uuid:
            tickers = data.data
            for ticker in tickers:
                self.log.info(f"{ticker}")


bybit_api_key = os.getenv("BYBIT_API_KEY", None)
bybit_api_secret = os.getenv("BYBIT_API_SECRET", None)

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
            api_key=bybit_api_key,
            api_secret=bybit_api_secret,
            instrument_types=[BybitInstrumentType.LINEAR],
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        "BYBIT": BybitExecClientConfig(
            api_key=bybit_api_key,
            api_secret=bybit_api_secret,
            instrument_types=[BybitInstrumentType.LINEAR],
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

node = TradingNode(config=config_node)

instrument_id = "ETHUSDT-LINEAR.BYBIT"
strategy_config = RequestDemoStrategyConfig(
    instrument_id=instrument_id,
    interval=10
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
