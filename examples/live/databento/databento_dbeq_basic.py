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

from nautilus_trader.adapters.databento.config import DatabentoDataClientConfig
from nautilus_trader.adapters.databento.factories import DatabentoLiveDataClientFactory
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.config.common import StrategyConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.strategy import Strategy


# *** THIS INTEGRATION IS STILL UNDER CONSTRUCTION. ***
# *** CONSIDER IT TO BE IN AN UNSTABLE BETA PHASE AND EXERCISE CAUTION. ***


# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="DEBUG"),  # For development
    exec_engine=LiveExecEngineConfig(
        reconciliation=False,  # Not applicable
        inflight_check_interval_ms=0,  # Not applicable
    ),
    # cache=CacheConfig(
    #     database=DatabaseConfig(),
    #     encoding="json",
    #     timestamps_as_iso8601=True,
    #     buffer_interval_ms=100,
    # ),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(),
    #     encoding="json",
    #     timestamps_as_iso8601=True,
    #     buffer_interval_ms=100,
    #     stream="quoters",
    #     use_instance_id=False,
    #     # types_filter=[QuoteTick],
    #     autotrim_mins=30,
    # ),
    # heartbeat_interval=1.0,
    # snapshot_orders=True,
    # snapshot_positions=True,
    # snapshot_positions_interval=5.0,
    data_clients={
        "DATABENTO": DatabentoDataClientConfig(
            api_key=None,  # 'BINANCE_API_KEY' env var
            http_gateway=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    timeout_connection=10.0,
    timeout_reconciliation=10.0,  # Not applicable
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=0.0,  # Not required as no order state
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)


class DataSubscriberConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``DataSubscriber`` instances.

    Parameters
    ----------
    instrument_ids : list[InstrumentId]
        The instrument IDs to subscribe to.

    """

    instrument_ids: list[InstrumentId]


class DataSubscriber(Strategy):
    """
    An example strategy which subscribes to live data.

    Parameters
    ----------
    config : DataSubscriberConfig
        The configuration for the instance.

    """

    def __init__(self, config: DataSubscriberConfig) -> None:
        super().__init__(config)

        # Configuration
        self.instrument_ids = config.instrument_ids
        self.databento_id = ClientId("DATABENTO")

    def on_start(self) -> None:
        """
        Actions to be performed when the strategy is started.

        Here we specify the 'DATABENTO' client for subscriptions.

        """
        for instrument_id in self.instrument_ids:
            self.subscribe_quote_ticks(instrument_id, client_id=self.databento_id)

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        # Databento does not yet support live data unsubscribing

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        """
        self.log.info(repr(tick), LogColor.CYAN)


# Configure your strategy
strat_config = DataSubscriberConfig(
    instrument_ids=[
        InstrumentId.from_str("AAPL.IEXG"),
    ],
)
# Instantiate your strategy
strategy = DataSubscriber(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user defined factories)
node.add_data_client_factory("DATABENTO", DatabentoLiveDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
