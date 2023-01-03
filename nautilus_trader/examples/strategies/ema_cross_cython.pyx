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

from decimal import Decimal
from typing import Optional

from nautilus_trader.config import StrategyConfig

from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.message cimport Event
from nautilus_trader.indicators.average.ema cimport ExponentialMovingAverage
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.trading.strategy cimport Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Notes for strategies written in Cython
# --------------------------------------
# The `except *` statement in void method signatures is to allow C and Python
# raised exceptions to bubble up (otherwise they are ignored)


class EMACrossConfig(StrategyConfig):
    """
    Configuration for ``EMACross`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    bar_type : BarType
        The bar type for the strategy.
    trade_size : str
        The position size per trade (interpreted as Decimal).
    fast_ema_period : int, default 10
        The fast EMA period.
    slow_ema_period : int, default 20
        The slow EMA period.
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OmsType
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).
    """

    instrument_id: str
    bar_type: str
    trade_size: Decimal
    fast_ema_period: int = 10
    slow_ema_period: int = 20


cdef class EMACross(Strategy):
    """
    A simple moving average cross example strategy.

    When the fast EMA crosses the slow EMA then enter a position at the market
    in that direction.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : EMACrossConfig
        The configuration for the instance.
    """
    # Backing fields are necessary
    cdef InstrumentId instrument_id
    cdef BarType bar_type
    cdef object trade_size
    cdef ExponentialMovingAverage fast_ema
    cdef ExponentialMovingAverage slow_ema

    def __init__(self, config not None: EMACrossConfig):
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str_c(config.instrument_id)
        self.bar_type = BarType.from_str_c(config.bar_type)
        self.trade_size = Decimal(config.trade_size)

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(config.fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(config.slow_ema_period)

        self.instrument: Optional[Instrument] = None  # Initialized in on_start

    cpdef void on_start(self) except *:
        """Actions to be performed on strategy start."""
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        # Register the indicators for updating
        self.register_indicator_for_bars(self.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.bar_type, self.slow_ema)

        # Get historical data
        self.request_bars(self.bar_type)

        # Subscribe to live data
        self.subscribe_bars(self.bar_type)

    cpdef void on_instrument(self, Instrument instrument) except *:
        """
        Actions to be performed when the strategy is running and receives an
        instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument received.

        """
        pass

    cpdef void on_order_book(self, OrderBook order_book) except *:
        """
        Actions to be performed when the strategy is running and receives an order book.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        """
        # self.log.info(f"Received {order_book}")  # For debugging (must add a subscription)
        pass

    cpdef void on_quote_tick(self, QuoteTick tick) except *:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        """
        # self.log.info(f"Received {tick}")  # For debugging (must add a subscription)
        pass

    cpdef void on_trade_tick(self, TradeTick tick) except *:
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        """
        # self.log.info(f"Received {tick}")  # For debugging (must add a subscription)
        pass

    cpdef void on_bar(self, Bar bar) except *:
        """
        Actions to be performed when the strategy is running and receives a bar.

        Parameters
        ----------
        bar : Bar
            The bar received.

        """
        self.log.info(f"Received Bar({bar})")

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(
                f"Waiting for indicators to warm up "
                f"[{self.cache.bar_count(self.bar_type)}]...",
                color=LogColor.BLUE,
            )
            return  # Wait for indicators to warm up...

        # BUY LOGIC
        if self.fast_ema.value >= self.slow_ema.value:
            if self.portfolio.is_flat(self.instrument_id):
                self.buy()
            elif self.portfolio.is_net_short(self.instrument_id):
                self.close_all_positions(self.instrument_id)
                self.buy()
        # SELL LOGIC
        elif self.fast_ema.value < self.slow_ema.value:
            if self.portfolio.is_flat(self.instrument_id):
                self.sell()
            elif self.portfolio.is_net_long(self.instrument_id):
                self.close_all_positions(self.instrument_id)
                self.sell()

    cpdef void buy(self) except *:
        """
        Users simple buy method (example).
        """
        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        cdef MarketOrder order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
        )

        self.submit_order(order)

    cpdef void sell(self) except *:
        """
        Users simple sell method (example).
        """
        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        cdef MarketOrder order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
        )

        self.submit_order(order)

    cpdef void on_data(self, Data data) except *:
        """
        Actions to be performed when the strategy is running and receives generic data.

        Parameters
        ----------
        data : Data
            The data received.

        """
        pass

    cpdef void on_event(self, Event event) except *:
        """
        Actions to be performed when the strategy is running and receives an event.

        Parameters
        ----------
        event : Event
            The event received.

        """
        pass

    cpdef void on_stop(self) except *:
        """
        Actions to be performed when the strategy is stopped.

        """
        self.cancel_all_orders(self.instrument_id)
        self.close_all_positions(self.instrument_id)

    cpdef void on_reset(self) except *:
        """
        Actions to be performed when the strategy is reset.
        """
        # Reset indicators here
        self.fast_ema.reset()
        self.slow_ema.reset()

    cpdef dict on_save(self):
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        """
        return {}

    cpdef void on_load(self, dict state) except *:
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict[str, bytes]
            The strategy state dictionary.

        """
        pass

    cpdef void on_dispose(self) except *:
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.

        """
        self.unsubscribe_bars(self.bar_type)
