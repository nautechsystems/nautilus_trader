#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_instrument_id
from nautilus_trader.adapters.polymarket.config import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket.config import PolymarketExecClientConfig
from nautilus_trader.adapters.polymarket.factories import PolymarketLiveDataClientFactory
from nautilus_trader.adapters.polymarket.factories import PolymarketLiveExecClientFactory
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.data import Data
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.trading.strategy import Strategy


# *** THIS INTEGRATION IS STILL UNDER CONSTRUCTION. ***
# *** CONSIDER IT TO BE IN AN UNSTABLE BETA PHASE AND EXERCISE CAUTION. ***

# For correct subscription operation, you must specify all instruments to be immediately
# subscribed for as part of the data client configuration

# US Election 2024 Winner market

# Trump
condition_id1 = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
token_id11 = 21742633143463906290569050155826241533067272736897614950488156847949938836455  #  (Yes)
token_id12 = 48331043336612883890938759509493159234755048973500640148014422747788308965732  #  (No)

# Kamala
condition_id2 = "0xc6485bb7ea46d7bb89beb9c91e7572ecfc72a6273789496f78bc5e989e4d1638"
token_id21 = 69236923620077691027083946871148646972011131466059644796654161903044970987404  #  (Yes)
token_id22 = 87584955359245246404952128082451897287778571240979823316620093987046202296181  #  (No)


instrument_ids = [
    get_polymarket_instrument_id(condition_id1, token_id11),
    get_polymarket_instrument_id(condition_id1, token_id12),
    get_polymarket_instrument_id(condition_id2, token_id21),
    get_polymarket_instrument_id(condition_id2, token_id22),
]

filters = {
    # "next_cursor": "MTE3MDA=",
    "is_active": True,
}

load_ids = [str(x) for x in instrument_ids]
instrument_provider_config = InstrumentProviderConfig(load_ids=frozenset(load_ids))
# instrument_provider_config = InstrumentProviderConfig(load_all=True, filters=filters)

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    cache=CacheConfig(
        # database=DatabaseConfig(),  # <-- Recommend Redis cache backing for Polymarket
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
        POLYMARKET: PolymarketDataClientConfig(
            private_key=None,  # 'POLYMARKET_PK' env var
            api_key=None,  # 'POLYMARKET_API_KEY' env var
            api_secret=None,  # 'POLYMARKET_API_SECRET' env var
            passphrase=None,  # 'POLYMARKET_PASSPHRASE' env var
            instrument_provider=instrument_provider_config,
            ws_connection_delay_secs=5,
            compute_effective_deltas=True,
        ),
    },
    exec_clients={
        POLYMARKET: PolymarketExecClientConfig(
            private_key=None,  # 'POLYMARKET_PK' env var
            api_key=None,  # 'POLYMARKET_API_KEY' env var
            api_secret=None,  # 'POLYMARKET_API_SECRET' env var
            passphrase=None,  # 'POLYMARKET_PASSPHRASE' env var
            instrument_provider=instrument_provider_config,
        ),
    },
    timeout_connection=60.0,
    timeout_reconciliation=20.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=10.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)


class TOBQuoterConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``TOBQuoter`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    trade_size : Decimal
        The position size per trade.
    dry_run : bool
        If the strategy should run without issuing order commands.
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.

    """

    instrument_id: InstrumentId
    trade_size: Decimal
    dry_run: bool = False


class TOBQuoter(Strategy):
    """
    A simple market maker which joins the current market best bid and ask.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : TOBQuoterConfig
        The configuration for the instance.

    """

    def __init__(self, config: TOBQuoterConfig) -> None:
        super().__init__(config)

        # Configuration
        self.instrument_id = config.instrument_id
        self.trade_size = Decimal(config.trade_size)
        self.dry_run = config.dry_run
        self.instrument: Instrument | None = None  # Initialized in on_start

        # Users order management variables
        self.buy_order: LimitOrder | None = None
        self.sell_order: LimitOrder | None = None

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        # Subscribe to live data
        self.subscribe_quote_ticks(self.instrument_id)
        self.subscribe_trade_ticks(self.instrument_id)

        self.subscribe_order_book_deltas(self.instrument_id)
        # self.subscribe_order_book_at_interval(
        #     self.instrument_id,
        #     depth=20,
        #     interval_ms=1000,
        # )  # For debugging

    def on_data(self, data: Data) -> None:
        """
        Actions to be performed when the strategy is running and receives data.

        Parameters
        ----------
        data : Data
            The data received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(data), LogColor.CYAN)

    def on_instrument(self, instrument: Instrument) -> None:
        """
        Actions to be performed when the strategy is running and receives an instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(instrument), LogColor.CYAN)

    def on_order_book(self, order_book: OrderBook) -> None:
        """
        Actions to be performed when the strategy is running and receives an order book.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(order_book), LogColor.CYAN)

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Actions to be performed when the strategy is running and receives order book
        deltas.

        Parameters
        ----------
        deltas : OrderBookDeltas
            The order book deltas received.

        """
        # For debugging (must add a subscription)
        # self.log.info(repr(deltas), LogColor.CYAN)

        book = self.cache.order_book(deltas.instrument_id)

        self.maintain_orders(book.best_bid_price(), book.best_ask_price())

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(tick), LogColor.CYAN)

        self.maintain_orders(tick.bid_price, tick.ask_price)

    def on_trade_tick(self, tick: TradeTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(tick), LogColor.CYAN)

    def maintain_orders(self, best_bid: Price, best_ask: Price) -> None:
        if self.dry_run:
            return

        if self.buy_order and (self.buy_order.is_emulated or self.buy_order.is_open):
            # TODO: Optionally cancel-replace
            # self.cancel_order(self.buy_order)
            pass

        if not self.buy_order or self.buy_order.is_closed:
            self.create_buy_order(best_bid)

        # Maintain sell orders
        if self.sell_order and (self.sell_order.is_emulated or self.sell_order.is_open):
            # TODO: Optionally cancel-replace
            # self.cancel_order(self.sell_order)
            pass

        if not self.sell_order or self.sell_order.is_closed:
            self.create_sell_order(best_ask)

    def create_buy_order(self, price: Price) -> None:
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
            price=price,
            # time_in_force=TimeInForce.GTD,
            # expire_time=self.clock.utc_now() + pd.Timedelta(minutes=10),
            post_only=False,  # Not supported on Polymarket
        )

        self.buy_order = order
        self.submit_order(order)

    def create_sell_order(self, price: Price) -> None:
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
            price=price,
            # time_in_force=TimeInForce.GTD,
            # expire_time=self.clock.utc_now() + pd.Timedelta(minutes=10),
            post_only=False,  # Not supported on Polymarket
        )

        self.sell_order = order
        self.submit_order(order)

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        if self.dry_run:
            return

        self.cancel_all_orders(self.instrument_id)
        self.close_all_positions(self.instrument_id, reduce_only=False)

    def on_reset(self) -> None:
        """
        Actions to be performed when the strategy is reset.
        """
        self.atr.reset()


instrument_id1 = instrument_ids[0]
# instrument_id2 = instrument_ids[1]
trade_size = Decimal("5")

# Configure your strategy
strat_config1 = TOBQuoterConfig(
    instrument_id=instrument_id1,
    external_order_claims=[instrument_id1],
    trade_size=trade_size,
    dry_run=False,
)
# strat_config2 = TOBQuoterConfig(
#     instrument_id=instrument_id2,
#     external_order_claims=[instrument_id2],
#     trade_size=trade_size,
# )

# Instantiate your strategy
strategy1 = TOBQuoter(config=strat_config1)
# strategy2 = TOBQuoter(config=strat_config2)

# Add your strategies and modules
node.trader.add_strategy(strategy1)
# node.trader.add_strategy(strategy2)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(POLYMARKET, PolymarketLiveDataClientFactory)
node.add_exec_client_factory(POLYMARKET, PolymarketLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
