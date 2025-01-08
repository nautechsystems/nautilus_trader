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

import time
import datetime
import numpy as np
import pandas as pd
from decimal import Decimal
from typing import List, Dict
from collections import deque
import os

from nautilus_trader.config import NonNegativeFloat
from nautilus_trader.config import PositiveInt
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.message import Event
from nautilus_trader.core.rust.common import LogColor
from nautilus_trader.indicators.atr import AverageTrueRange
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import book_type_from_str
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.position import Position
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.accounting.accounts.base import Account


class HighFrequencyGridTradingConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``HighFrequencyGridTrading`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    max_trade_size : Decimal
        The max position size per trade (volume on the level can be less).
    min_seconds_between_triggers : NonNegativeFloat, default 1.0
        The minimum time between triggers.
    book_type : str, default 'L2_MBP'
        The order book type for the strategy.
    use_quote_ticks : bool, default False
        If quote ticks should be used.
    subscribe_ticker : bool, default False
        If tickers should be subscribed to.
    use_trade_ticks : bool, default False
        If trade ticks should be used.
    grid_num : PositiveInt, default 20
        网格数.
    max_position : PositiveFloat, default xx
        算法的最大持仓.
    grid_interval : PositiveInt, default 10
        网格的间隔.
    half_spread : PositiveInt, default 20
        半价差.
    """

    instrument_id: InstrumentId
    max_trade_size: Decimal
    min_seconds_between_triggers: NonNegativeFloat = 0.2
    book_type: str = "L2_MBP"
    use_quote_ticks: bool = True
    subscribe_ticker: bool = True
    use_trade_ticks: bool = True

    grid_num: PositiveInt = int(os.getenv('GRID_NUM', 10))
    max_position: PositiveFloat = float(os.getenv('MAX_POSITION', 1.0))
    grid_interval: PositiveInt = int(os.getenv('GRID_INTERVAL', 28))
    half_spread: PositiveInt = int(os.getenv('HALF_SPREAD', 36))
    skew: PositiveFloat = float(os.getenv('SKEW', 1.2))

    looking_depth: PositiveFloat = float(os.getenv('LOOKING_DEPTH', 0.01))
    adjusted_factor: PositiveFloat = float(os.getenv('ADJUSTED_FACTOR', 1.2))


class HighFrequencyGridTrading(Strategy):
    """
    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : HighFrequencyGridTradingConfig
        The configuration for the instance.

    """

    def __init__(self, config: HighFrequencyGridTradingConfig) -> None:
        super().__init__(config)

        # Configuration
        self.instrument_id = config.instrument_id
        self.max_trade_size = config.max_trade_size
        self.min_seconds_between_triggers = config.min_seconds_between_triggers
        self._last_trigger_timestamp: datetime.datetime | None = None
        self._last_check_timestamp: datetime.datetime | None = None
        self.instrument: Instrument | None = None
        self.book_type: BookType = book_type_from_str(self.config.book_type)
          
        self.init_balance: float = None
        self.order_side: OrderSide = None
        self.qty: float = None
        self.avg_px: float = None

        self.last_px: float = None
        self.last_order_side: OrderSide = None
        
        self.prev_mid: float = None

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        BINANCE = Venue("BINANCE")
        self.init_balance = float(self.portfolio.account(BINANCE).balance_free(Currency.from_str('USDT')))
        self.instrument = self.cache.instrument(self.instrument_id)
        self.tick_size: Price = self.instrument.price_increment

        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        if self.config.use_trade_ticks:
            self.subscribe_trade_ticks(instrument_id=self.instrument_id)

        if self.config.use_quote_ticks:
            self.subscribe_quote_ticks(self.instrument.id)
        
        self.subscribe_order_book_deltas(self.instrument.id, self.book_type)        
        
        self._last_trigger_timestamp = self.clock.utc_now()
        self._last_check_timestamp = self.clock.utc_now()
    
    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Actions to be performed when order book deltas are received.
        """
        pass

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when a delta is received.
        """
        seconds_since_last_trigger = (
            self.clock.utc_now() - self._last_trigger_timestamp
        ).total_seconds()
        if seconds_since_last_trigger < self.min_seconds_between_triggers:
            self.log.debug("Time since last order < min_seconds_between_triggers - skipping")
            return 
        self.check_trigger()

    def on_trade_tick(self, tick: TradeTick) -> None:
        pass

    def on_order_book(self, order_book: OrderBook) -> None:
        """
        Actions to be performed when an order book update is received.
        """
        pass
    
    def check_trigger(self) -> None:
        """
        Check for trigger conditions.
        """
        self._last_trigger_timestamp = self.clock.utc_now()
        
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        # Fetch book from the cache being maintained by the `DataEngine`
        book = self.cache.order_book(self.instrument_id)
        if not book:
            self.log.error("No book being maintained")
            return

        if not book.spread():
            return

        best_bid_size: Quantity | None = book.best_bid_size()
        best_ask_size: Quantity | None = book.best_ask_size()
        if (best_bid_size is None or best_bid_size <= 0) or (best_ask_size is None or best_ask_size <= 0):
            self.log.warning("No market yet")
            return

        best_bid_price: Price | None = book.best_bid_price()
        best_ask_price: Price | None = book.best_ask_price()
        if (best_bid_price is None or best_bid_price <= 0) or (best_ask_price is None or best_ask_price <= 0):
            self.log.warning("No market yet")
            return
        
        if (best_ask_price - best_bid_price) / self.tick_size > 2:
            self.cancel_all_orders(self.instrument.id)
            return 

        net_position = self.portfolio.net_position(self.instrument_id)
        mid_price = (best_bid_price + best_ask_price) / Decimal('2.0')
        skew_position = np.power(self.config.skew, float(net_position) / self.config.max_position)
        reservation_price = mid_price - self.tick_size * Decimal(skew_position)
        
        if reservation_price == self.prev_mid:
            return 

        looking_depth = int(np.floor(float(mid_price) * self.config.looking_depth / self.tick_size))
        grid_interval = max(self.config.grid_interval, int(np.floor(looking_depth/self.config.grid_num)))

        grid_interval *= self.tick_size
        bid_half_spread = self.tick_size * self.config.half_spread
        ask_half_spread = self.tick_size * self.config.half_spread

        pow_coef = 0.0
        if self.portfolio.is_net_short(self.instrument_id):
            pow_coef = np.minimum(0, float(net_position) + self.config.max_position / 2)
        elif self.portfolio.is_net_long(self.instrument_id):
            pow_coef = np.maximum(0, float(net_position) - self.config.max_position / 2)

        bid_half_spread *= Decimal(np.power(self.config.adjusted_factor, pow_coef / self.config.max_position))
        ask_half_spread *= Decimal(np.power(1/self.config.adjusted_factor, pow_coef / self.config.max_position))

        # Since our price is skewed, it may cross the spread. To ensure market making and avoid crossing the spread,
        # limit the price to the best bid and best ask.
        bid_price = np.minimum(reservation_price - bid_half_spread, best_bid_price)
        ask_price = np.maximum(reservation_price + ask_half_spread, best_ask_price)

        # Aligns the prices to the grid.
        bid_price = np.floor(bid_price / grid_interval) * grid_interval
        ask_price = np.ceil(ask_price / grid_interval) * grid_interval

        #if (bid_price == self.last_px and self.last_order_side == OrderSide.BUY) or \
        #        (ask_price == self.last_px and self.last_order_side == OrderSide.SELL):
        #    return 

        if len(self.cache.orders_inflight(strategy_id=self.id)) > 0:
            self.log.info("Already have orders in flight - skipping.")
            return
        
        fib_coef = [1.,1.,2.,3.,5.,8.,13.,21.,34.,55.]
        
        half_spread = self.tick_size * self.config.half_spread
        new_bid_orders = dict()
        if net_position < self.config.max_position:
            for coef, i in zip(fib_coef, range(self.config.grid_num)):
                bid_price_tick = round(bid_price / self.tick_size)
                
                if net_position != 0 and self.last_px is not None and self.last_order_side is not None:
                    if self.last_order_side == OrderSide.BUY and \
                            bid_price > self.last_px - 3*half_spread and \
                            bid_price < self.last_px + 3*half_spread:
                        bid_price -= Decimal(coef) * grid_interval
                        continue

                # order price in tick is used as order id.
                new_bid_orders[np.uint64(bid_price_tick)] = bid_price
                
                if net_position > self.config.max_position/2:
                    bid_price -= Decimal(coef) * grid_interval * Decimal(np.power(self.config.adjusted_factor, pow_coef / self.config.max_position))
                else:
                    bid_price -= Decimal(coef) * grid_interval

        new_ask_orders = dict()
        if -net_position < self.config.max_position:
            for coef, i in zip(fib_coef, range(self.config.grid_num)):
                ask_price_tick = round(ask_price / self.tick_size)
                
                if net_position != 0 and self.last_px is not None and self.last_order_side is not None:
                    if self.last_order_side == OrderSide.SELL and \
                            ask_price > self.last_px - 3*half_spread and \
                            ask_price < self.last_px + 3*half_spread:
                        ask_price += Decimal(coef) * grid_interval
                        continue

                # order price in tick is used as order id.
                new_ask_orders[np.uint64(ask_price_tick)] = ask_price
                
                if net_position < -self.config.max_position/2:
                    ask_price += Decimal(coef) * grid_interval * Decimal(np.power(1/self.config.adjusted_factor, pow_coef / self.config.max_position))
                else:
                    ask_price += Decimal(coef) * grid_interval
        
        open_orders = self.cache.orders_open(instrument_id=self.instrument_id)
        for order in open_orders:
            if order.status != OrderStatus.CANCELED and order.is_open:
                price_tick = np.uint64(round(order.price / self.tick_size))
                if order.side == OrderSide.BUY and price_tick not in new_bid_orders:
                    self.cancel_order(order)
                elif order.side == OrderSide.SELL and price_tick not in new_ask_orders:
                    self.cancel_order(order)
        
        orders = self.cache.orders_open(instrument_id=self.instrument_id)
        for order_id, order_price in new_bid_orders.items():
            # Posts a new buy order if there is no working order at the price on the new grid.
            open_order_price_ticks = []
            for open_order in orders:
                if open_order.side == OrderSide.BUY:
                    bid_price_tick = round(open_order.price / self.tick_size)
                    open_order_price_ticks.append(np.uint64(bid_price_tick))

            if order_id not in open_order_price_ticks:
                self.buy(order_price, self.max_trade_size)
                

        for order_id, order_price in new_ask_orders.items():
            # Posts a new sell order if there is no working order at the price on the new grid.
            open_order_price_ticks = []
            for open_order in orders:
                if open_order.side == OrderSide.SELL:
                    ask_price_tick = round(open_order.price / self.tick_size)
                    open_order_price_ticks.append(np.uint64(ask_price_tick))

            if order_id not in open_order_price_ticks:
                self.sell(order_price, self.max_trade_size)

        self.prev_mid = reservation_price
            

    def buy(self, bid_price, quantity) -> None:
        order = self.order_factory.limit(
            instrument_id=self.instrument.id,
            price=self.instrument.make_price(bid_price),
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(quantity),
            #time_in_force=TimeInForce.GTD,
            #expire_time=self.clock.utc_now() + pd.Timedelta(seconds=10),
            #post_only=True,  # default value is True
            #reduce_only=False
        )
        #self.log.info(f"Hitting! {order=}", color=LogColor.BLUE)
        self.submit_order(order)
    
    def sell(self, ask_price, quantity)-> None:
        order = self.order_factory.limit(
            instrument_id=self.instrument.id,
            price=self.instrument.make_price(ask_price),
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(quantity),
            #time_in_force=TimeInForce.GTD,
            #expire_time=self.clock.utc_now() + pd.Timedelta(seconds=10),
            #post_only=True,  # default value is True
            #reduce_only=False
        )
            
        #self.log.info(f"Hitting! {order=}", color=LogColor.BLUE)
        self.submit_order(order)

    def on_event(self, event: Event) -> None:
        if isinstance(event, PositionOpened):
            self.qty = event.signed_qty
            self.order_side = event.side
            self.avg_px = event.avg_px_open

        if isinstance(event, OrderFilled):
            self.last_px = event.last_px
            self.last_order_side = event.order_side
        
    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        if self.instrument is None:
            return

        self.cancel_all_orders(self.instrument.id)
        self.close_all_positions(self.instrument.id)
