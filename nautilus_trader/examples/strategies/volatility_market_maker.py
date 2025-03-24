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

from decimal import Decimal

import pandas as pd

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.indicators.atr import AverageTrueRange
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class VolatilityMarketMakerConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``VolatilityMarketMaker`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    bar_type : BarType
        The bar type for the strategy.
    atr_period : PositiveInt
        The period for the ATR indicator.
    atr_multiple : PositiveFloat
        The ATR multiple for bracketing limit orders.
    trade_size : Decimal
        The position size per trade.
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    emulation_trigger : str, default 'NO_TRIGGER'
        The emulation trigger for submitting emulated orders.
        If ``None`` then orders will not be emulated.
    client_id : ClientId, optional
        The custom client ID for data and execution.
        For example if you have multiple clients for Binance you might use 'BINANCE-SPOT'.

    """

    instrument_id: InstrumentId
    bar_type: BarType
    atr_period: PositiveInt
    atr_multiple: PositiveFloat
    trade_size: Decimal
    emulation_trigger: str = "NO_TRIGGER"
    client_id: ClientId | None = None


class VolatilityMarketMaker(Strategy):
    """
    A very dumb market maker which brackets the current market based on volatility
    measured by an ATR indicator.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : VolatilityMarketMakerConfig
        The configuration for the instance.

    """

    def __init__(self, config: VolatilityMarketMakerConfig) -> None:
        super().__init__(config)

        self.instrument: Instrument | None = None  # Initialized in on_start
        self.client_id = config.client_id

        # Create the indicators for the strategy
        self.atr = AverageTrueRange(config.atr_period)

        # Users order management variables
        self.buy_order: LimitOrder | None = None
        self.sell_order: LimitOrder | None = None

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        # Register the indicators for updating
        self.register_indicator_for_bars(self.config.bar_type, self.atr)

        # Get historical data
        self.request_bars(
            self.config.bar_type,
            client_id=self.client_id,
            start=self.clock.utc_now() - pd.Timedelta(days=1),
        )

        # Subscribe to live data
        self.subscribe_bars(self.config.bar_type, client_id=self.client_id)
        self.subscribe_quote_ticks(self.config.instrument_id, client_id=self.client_id)
        self.subscribe_trade_ticks(self.config.instrument_id, client_id=self.client_id)
        # self.subscribe_order_book_deltas(self.config.instrument_id, client_id=self.client_id)  # For debugging
        # self.subscribe_order_book_at_interval(
        #     self.config.instrument_id,
        #     depth=20,
        #     interval_ms=1000,
        #     client_id=self.client_id,
        # )  # For debugging

        # self.subscribe_data(
        #     data_type=DataType(
        #         BinanceTicker,
        #         metadata={"instrument_id": self.instrument.id},
        #     ),
        #     client_id=self.client_id,
        # )

        # self.subscribe_data(
        #     data_type=DataType(
        #         BinanceFuturesMarkPriceUpdate,
        #         metadata={"instrument_id": self.instrument.id},
        #     ),
        #     client_id=ClientId("BINANCE"),
        # )

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
        # self.log.info(repr(instrument), LogColor.CYAN)

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
        self.log.info(repr(deltas), LogColor.CYAN)

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

        # own_book = self.cache.own_order_book(tick.instrument_id)
        # if not own_book:
        #     return
        # self.log.info("\n" + repr(own_book), LogColor.MAGENTA)
        # self.log.info("\n" + own_book.pprint(), LogColor.MAGENTA)

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

    def on_bar(self, bar: Bar) -> None:
        """
        Actions to be performed when the strategy is running and receives a bar.

        Parameters
        ----------
        bar : Bar
            The bar received.

        """
        self.log.info(repr(bar), LogColor.CYAN)

        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(
                f"Waiting for indicators to warm up [{self.cache.bar_count(self.config.bar_type)}]",
                color=LogColor.BLUE,
            )
            return  # Wait for indicators to warm up...

        last: QuoteTick = self.cache.quote_tick(self.config.instrument_id)
        if last is None:
            self.log.info("No quotes yet")
            return

        # Maintain buy orders
        if self.buy_order and (self.buy_order.is_emulated or self.buy_order.is_open):
            # price: Decimal = last.bid_price - (self.atr.value * self.config.atr_multiple)
            # self.modify_order(
            #     order=self.buy_order,
            #     price=self.instrument.make_price(price),
            # )
            # return
            self.cancel_order(self.buy_order)
        self.create_buy_order(last)

        # Maintain sell orders
        if self.sell_order and (self.sell_order.is_emulated or self.sell_order.is_open):
            # price = last.ask_price + (self.atr.value * self.config.atr_multiple)
            # self.modify_order(
            #     order=self.sell_order,
            #     price=self.instrument.make_price(price),
            # )
            # return
            self.cancel_order(self.sell_order)
        self.create_sell_order(last)

    def create_buy_order(self, last: QuoteTick) -> None:
        """
        Market maker simple buy limit method (example).
        """
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        price: Decimal = last.bid_price - (self.atr.value * self.config.atr_multiple)
        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.config.trade_size),
            price=self.instrument.make_price(price),
            time_in_force=TimeInForce.GTD,
            expire_time=self.clock.utc_now() + pd.Timedelta(minutes=10),
            post_only=True,  # default value is True
            # display_qty=self.instrument.make_qty(self.config.trade_size / 2),  # iceberg
            emulation_trigger=TriggerType[self.config.emulation_trigger],
        )

        self.buy_order = order
        self.submit_order(order, client_id=self.client_id)
        # order_list = self.order_factory.create_list([order])
        # self.submit_order_list(order_list)

    def create_sell_order(self, last: QuoteTick) -> None:
        """
        Market maker simple sell limit method (example).
        """
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        price: Decimal = last.ask_price + (self.atr.value * self.config.atr_multiple)
        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.config.trade_size),
            price=self.instrument.make_price(price),
            time_in_force=TimeInForce.GTD,
            expire_time=self.clock.utc_now() + pd.Timedelta(minutes=10),
            post_only=True,  # default value is True
            # display_qty=self.instrument.make_qty(self.config.trade_size / 2),  # iceberg
            emulation_trigger=TriggerType[self.config.emulation_trigger],
        )

        self.sell_order = order
        self.submit_order(order, client_id=self.client_id)
        # order_list = self.order_factory.create_list([order])
        # self.submit_order_list(order_list, client_id=self.client_id)

    def on_event(self, event: Event) -> None:
        """
        Actions to be performed when the strategy is running and receives an event.

        Parameters
        ----------
        event : Event
            The event received.

        """
        last: QuoteTick = self.cache.quote_tick(self.config.instrument_id)
        if last is None:
            self.log.info("No quotes yet")
            return

        # If order filled then replace order at ATR multiple distance from the market
        if isinstance(event, OrderFilled):
            if self.buy_order and event.order_side == OrderSide.BUY:
                if self.buy_order.is_closed:
                    self.create_buy_order(last)
            elif (
                self.sell_order and event.order_side == OrderSide.SELL and self.sell_order.is_closed
            ):
                self.create_sell_order(last)

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.config.instrument_id, client_id=self.client_id)

        # # Uncomment to test individual cancels
        # for order in self.cache.orders_open(instrument_id=self.config.instrument_id):
        #     self.cancel_order(order)

        # # Uncomment to test batch cancel
        # open_orders = self.cache.orders_open(instrument_id=self.config.instrument_id)
        # if open_orders:
        #     self.cancel_orders(open_orders, client_id=self.client_id)

        self.close_all_positions(self.config.instrument_id, client_id=self.client_id)

        # Unsubscribe from data
        self.unsubscribe_bars(self.config.bar_type, client_id=self.client_id)
        self.unsubscribe_quote_ticks(self.config.instrument_id, client_id=self.client_id)
        self.unsubscribe_trade_ticks(self.config.instrument_id, client_id=self.client_id)
        # self.unsubscribe_order_book_deltas(self.config.instrument_id, client_id=self.client_id)  # For debugging
        # self.unsubscribe_order_book_at_interval(self.config.instrument_id, client_id=self.client_id)  # For debugging

    def on_reset(self) -> None:
        """
        Actions to be performed when the strategy is reset.
        """
        # Reset indicators here
        self.atr.reset()

    def on_save(self) -> dict[str, bytes]:
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        """
        return {}

    def on_load(self, state: dict[str, bytes]) -> None:
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict[str, bytes]
            The strategy state dictionary.

        """

    def on_dispose(self) -> None:
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.

        """
