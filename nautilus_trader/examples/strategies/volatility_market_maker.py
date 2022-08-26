# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Dict, Optional, Union

from nautilus_trader.common.logging import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.indicators.atr import AverageTrueRange
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class VolatilityMarketMakerConfig(StrategyConfig):
    """
    Configuration for ``VolatilityMarketMaker`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    bar_type : BarType
        The bar type for the strategy.
    atr_period : int
        The period for the ATR indicator.
    atr_multiple : float
        The ATR multiple for bracketing limit orders.
    trade_size : Decimal
        The position size per trade.
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OMSType
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).
    """

    instrument_id: str
    bar_type: str
    atr_period: int
    atr_multiple: float
    trade_size: Decimal


class VolatilityMarketMaker(Strategy):
    """
    A very dumb market maker which brackets the current market based on
    volatility measured by an ATR indicator.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : VolatilityMarketMakerConfig
        The configuration for the instance.
    """

    def __init__(self, config: VolatilityMarketMakerConfig):
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.bar_type = BarType.from_str(config.bar_type)
        self.trade_size = Decimal(config.trade_size)
        self.atr_multiple = config.atr_multiple

        # Create the indicators for the strategy
        self.atr = AverageTrueRange(config.atr_period)

        self.instrument: Optional[Instrument] = None  # Initialized in on_start

        # Users order management variables
        self.buy_order: Union[LimitOrder, None] = None
        self.sell_order: Union[LimitOrder, None] = None

    def on_start(self):
        """Actions to be performed on strategy start."""
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        # Register the indicators for updating
        self.register_indicator_for_bars(self.bar_type, self.atr)

        # Get historical data
        self.request_bars(self.bar_type)

        # Subscribe to live data
        self.subscribe_bars(self.bar_type)
        self.subscribe_quote_ticks(self.instrument_id)
        # self.subscribe_trade_ticks(self.instrument_id)
        # self.subscribe_ticker(self.instrument_id)  # For debugging
        # self.subscribe_order_book_deltas(self.instrument_id, depth=100)  # For debugging
        # self.subscribe_order_book_snapshots(self.instrument_id, depth=20, interval_ms=1000)  # For debugging
        # self.subscribe_data(
        #     data_type=DataType(
        #         BinanceFuturesMarkPriceUpdate, metadata={"instrument_id": self.instrument.id}
        #     ),
        #     client_id=ClientId("BINANCE"),
        # )

    def on_data(self, data: Data):
        """
        Actions to be performed when the strategy is running and receives generic
        data.

        Parameters
        ----------
        data : Data
            The data received.

        """
        # For debugging (must add a subscription)
        # self.log.info(repr(data), LogColor.CYAN)
        pass

    def on_instrument(self, instrument: Instrument):
        """
        Actions to be performed when the strategy is running and receives an
        instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument received.

        """
        # For debugging (must add a subscription)
        # self.log.info(repr(instrument), LogColor.CYAN)
        pass

    def on_order_book(self, order_book: OrderBook):
        """
        Actions to be performed when the strategy is running and receives an order book.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        """
        # For debugging (must add a subscription)
        # self.log.info(repr(order_book), LogColor.CYAN)
        pass

    def on_order_book_delta(self, delta: OrderBookDelta):
        """
        Actions to be performed when the strategy is running and receives an order book delta.

        Parameters
        ----------
        delta : OrderBookDelta
            The order book delta received.

        """
        # For debugging (must add a subscription)
        # self.log.info(repr(delta), LogColor.CYAN)
        pass

    def on_ticker(self, ticker: Ticker):
        """
        Actions to be performed when the strategy is running and receives a ticker.

        Parameters
        ----------
        ticker : Ticker
            The ticker received.

        """
        # For debugging (must add a subscription)
        # self.log.info(repr(ticker), LogColor.CYAN)
        pass

    def on_quote_tick(self, tick: QuoteTick):
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        """
        # For debugging (must add a subscription)
        # self.log.info(repr(tick), LogColor.CYAN)
        pass

    def on_trade_tick(self, tick: TradeTick):
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        """
        # For debugging (must add a subscription)
        # self.log.info(repr(tick), LogColor.CYAN)
        pass

    def on_bar(self, bar: Bar):
        """
        Actions to be performed when the strategy is running and receives a bar.

        Parameters
        ----------
        bar : Bar
            The bar received.

        """
        self.log.info(repr(bar), LogColor.CYAN)

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(
                f"Waiting for indicators to warm up " f"[{self.cache.bar_count(self.bar_type)}]...",
                color=LogColor.BLUE,
            )
            return  # Wait for indicators to warm up...

        last: QuoteTick = self.cache.quote_tick(self.instrument_id)
        if last is None:
            self.log.info("No quotes yet...")
            return

        # Maintain buy orders
        if self.buy_order and self.buy_order.is_open:
            self.cancel_order(self.buy_order)
        self.create_buy_order(last)

        # Maintain sell orders
        if self.sell_order and self.sell_order.is_open:
            self.cancel_order(self.sell_order)
        self.create_sell_order(last)

    def create_buy_order(self, last: QuoteTick):
        """
        Market maker simple buy limit method (example).
        """
        price: Decimal = last.bid - (self.atr.value * self.atr_multiple)
        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
            price=self.instrument.make_price(price),
            time_in_force=TimeInForce.GTC,
            post_only=True,  # default value is True
            # display_qty=self.instrument.make_qty(self.trade_size / 2),  # iceberg
        )

        self.buy_order = order
        self.submit_order(order)

    def create_sell_order(self, last: QuoteTick):
        """
        Market maker simple sell limit method (example).
        """
        price: Decimal = last.ask + (self.atr.value * self.atr_multiple)
        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
            price=self.instrument.make_price(price),
            time_in_force=TimeInForce.GTC,
            post_only=True,  # default value is True
            # display_qty=self.instrument.make_qty(self.trade_size / 2),  # iceberg
        )

        self.sell_order = order
        self.submit_order(order)

    def on_event(self, event: Event):
        """
        Actions to be performed when the strategy is running and receives an event.

        Parameters
        ----------
        event : Event
            The event received.

        """
        last: QuoteTick = self.cache.quote_tick(self.instrument_id)
        if last is None:
            self.log.info("No quotes yet...")
            return

        # If order filled then replace order at atr multiple distance from the market
        if isinstance(event, OrderFilled):
            if event.order_side == OrderSide.BUY:
                if self.buy_order.is_closed:
                    self.create_buy_order(last)
            elif event.order_side == OrderSide.SELL:
                if self.sell_order.is_closed:
                    self.create_sell_order(last)

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.instrument_id)
        self.close_all_positions(self.instrument_id)

        # Unsubscribe from data
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_quote_ticks(self.instrument_id)

    def on_reset(self):
        """
        Actions to be performed when the strategy is reset.
        """
        # Reset indicators here
        self.atr.reset()

    def on_save(self) -> Dict[str, bytes]:
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        """
        return {}

    def on_load(self, state: Dict[str, bytes]):
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict[str, bytes]
            The strategy state dictionary.

        """
        pass

    def on_dispose(self):
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.

        """
        pass
