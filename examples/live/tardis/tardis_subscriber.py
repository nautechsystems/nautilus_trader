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

from typing import Any

from nautilus_trader.adapters.tardis.config import TardisDataClientConfig
from nautilus_trader.adapters.tardis.constants import TARDIS
from nautilus_trader.adapters.tardis.constants import TARDIS_CLIENT_ID
from nautilus_trader.adapters.tardis.factories import TardisLiveDataClientFactory
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.strategy import Strategy


# *** THIS INTEGRATION IS STILL UNDER CONSTRUCTION. ***
# *** CONSIDER IT TO BE IN AN UNSTABLE BETA PHASE AND EXERCISE CAUTION. ***

# Run the following to start the tardis-machine server:
# docker run -p 8000:8000 -p 8001:8001 -e "TM_API_KEY=YOUR_API_KEY" -d tardisdev/tardis-machine

instrument_ids = [
    InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    InstrumentId.from_str("BTCUSD_PERP.BINANCE_DELIVERY"),
    # InstrumentId.from_str("USDTUSD.BINANCE_US"),
    # InstrumentId.from_str("BTCUSDT-SPOT.BYBIT"),
    # InstrumentId.from_str("BTCUSDT-LINEAR.BYBIT"),
    # InstrumentId.from_str("BTCUSDT.BINANCE"),
    # InstrumentId.from_str("XBTUSDT.BITMEX"),
    # InstrumentId.from_str("ETHUSDT.BITMEX"),
    # InstrumentId.from_str("BTC_USDT.GATE_IO"),
    # InstrumentId.from_str("BTC_USDT-PERP.GATE_IO"),
]

# See supported venues https://nautilustrader.io/docs/nightly/integrations/tardis#venues
venues = {i.venue.value for i in instrument_ids}
filters = {"venues": frozenset(venues)}
instrument_provider_config = InstrumentProviderConfig(load_all=True, filters=filters)

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=False,  # Not applicable
        inflight_check_interval_ms=0,  # Not applicable
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    cache=CacheConfig(
        # database=DatabaseConfig(),
        encoding="msgpack",
        timestamps_as_iso8601=True,
        buffer_interval_ms=100,
    ),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(),
    #     encoding="json",
    #     timestamps_as_iso8601=True,
    #     buffer_interval_ms=100,
    #     streams_prefix="quoters",
    #     use_instance_id=False,
    #     # types_filter=[QuoteTick],
    #     autotrim_mins=30,
    # ),
    # heartbeat_interval=1.0,
    data_clients={
        TARDIS: TardisDataClientConfig(
            api_key=None,  # 'TARDIS_API_KEY' env var
            instrument_provider=instrument_provider_config,
        ),
    },
    timeout_connection=20.0,
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
        self.client_id = TARDIS_CLIENT_ID

    def on_start(self) -> None:
        """
        Actions to be performed when the strategy is started.

        Here we specify the 'TARDIS' client_id for subscriptions.

        """
        for instrument_id in self.config.instrument_ids:
            # from nautilus_trader.model.enums import BookType
            # self.subscribe_order_book_at_interval(
            #     instrument_id=instrument_id,
            #     book_type=BookType.L2_MBP,
            #     depth=10,  # Optimal for Tardis snapshots
            #     interval_ms=1000,
            # )

            # self.subscribe_order_book_deltas(instrument_id=instrument_id, client_id=self.client_id)
            self.subscribe_quote_ticks(instrument_id, client_id=self.client_id)
            self.subscribe_trade_ticks(instrument_id, client_id=self.client_id)

            # from nautilus_trader.model.data import BarType
            # bar_type = BarType.from_str(f"{instrument_id}-1-SECOND-LAST-EXTERNAL")
            # self.subscribe_bars(bar_type, client_id=self.client_id)

            # self.subscribe_instrument_status(instrument_id)
            # self.request_quote_ticks(instrument_id)
            # self.request_trade_ticks(instrument_id)

            # from nautilus_trader.model.data import DataType
            # from nautilus_trader.model.data import InstrumentStatus
            #
            # status_data_type = DataType(
            #     type=InstrumentStatus,
            #     metadata={"instrument_id": instrument_id},
            # )
            # self.request_data(status_data_type)

            # from nautilus_trader.model.data import BarType
            # self.request_bars(BarType.from_str(f"{instrument_id}-1-SECOND-LAST-EXTERNAL"))

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        for instrument_id in self.config.instrument_ids:
            # self.unsubscribe_order_book_deltas(
            #     instrument_id=instrument_id,
            #     client_id=self.client_id,
            # )
            self.unsubscribe_quote_ticks(instrument_id, client_id=self.client_id)
            self.unsubscribe_trade_ticks(instrument_id, client_id=self.client_id)

    def on_historical_data(self, data: Any) -> None:
        self.log.info(repr(data), LogColor.CYAN)

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Actions to be performed when the strategy is running and receives order book
        deltas.

        Parameters
        ----------
        deltas : OrderBookDeltas
            The order book deltas received.

        """
        # self.log.info(repr(deltas), LogColor.CYAN)
        book = self.cache.order_book(deltas.instrument_id)
        self.log.info(f"\n{book.instrument_id}\n" + book.pprint(3), LogColor.CYAN)

    def on_order_book(self, order_book: OrderBook) -> None:
        """
        Actions to be performed when an order book update is received.
        """
        self.log.info(f"\n{order_book.instrument_id}\n" + order_book.pprint(10), LogColor.CYAN)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        """
        self.log.info(repr(tick), LogColor.CYAN)

    def on_trade_tick(self, tick: TradeTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        """
        self.log.info(repr(tick), LogColor.CYAN)

    def on_bar(self, bar: Bar) -> None:
        """
        Actions to be performed when the strategy is running and receives a bar.

        Parameters
        ----------
        bar : Bar
            The bar received.

        """
        self.log.info(repr(bar), LogColor.CYAN)


# Configure and initialize your strategy
strat_config = DataSubscriberConfig(instrument_ids=instrument_ids)
strategy = DataSubscriber(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(TARDIS, TardisLiveDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
