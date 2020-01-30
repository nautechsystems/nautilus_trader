# -------------------------------------------------------------------------------------------------
# <copyright file="ema_cross.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from collections import deque
from datetime import timedelta
from typing import Dict

from nautilus_trader.core.message import Event
from nautilus_trader.common.functions import fast_mean
from nautilus_trader.model.enums import OrderSide, OrderPurpose, TimeInForce, PriceType
from nautilus_trader.model.objects import Price, Tick, BarSpecification, BarType, Bar, Instrument
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.trade.strategy import TradingStrategy
from nautilus_trader.trade.sizing import FixedRiskSizer

from nautilus_indicators.average.ema import ExponentialMovingAverage
from nautilus_indicators.atr import AverageTrueRange


class EMACrossPy(TradingStrategy):
    """"
    A simple moving average cross example strategy. When the fast EMA crosses
    the slow EMA then a STOP_MARKET atomic order is placed for that direction
    with a trailing stop and profit target at 1R risk.
    """

    def __init__(self,
                 symbol: Symbol,
                 bar_spec: BarSpecification,
                 risk_bp: float=10.0,
                 fast_ema: int=10,
                 slow_ema: int=20,
                 atr_period: int=20,
                 sl_atr_multiple: float=2.0):
        """
        Initializes a new instance of the EMACrossPy class.

        :param symbol: The symbol for the strategy.
        :param bar_spec: The bar specification for the strategy.
        :param risk_bp: The risk per trade (basis points).
        :param fast_ema: The fast EMA period.
        :param slow_ema: The slow EMA period.
        :param atr_period: The ATR period.
        :param sl_atr_multiple: The ATR multiple for stop-loss prices.
        """
        # Order id tag must be unique at trader level
        super().__init__(order_id_tag=symbol.code, bar_capacity=40)

        # Custom strategy variables
        self.symbol = symbol
        self.bar_type = BarType(symbol, bar_spec)
        self.precision = 5          # dummy initial value for FX
        self.risk_bp = risk_bp
        self.entry_buffer = 0.0     # instrument.tick_size
        self.SL_buffer = 0.0        # instrument.tick_size * 10
        self.SL_atr_multiple = sl_atr_multiple

        self.instrument = None      # initialized in on_start()
        self.position_sizer = None  # initialized in on_start()

        # Track spreads
        self.spreads = deque(maxlen=100)

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(fast_ema)
        self.slow_ema = ExponentialMovingAverage(slow_ema)
        self.atr = AverageTrueRange(atr_period)

    def on_start(self):
        """
        This method is called when self.start() is called, and after internal start logic.
        """
        # Put custom code to be run on strategy start here (or pass)
        self.instrument = self.get_instrument(self.symbol)
        self.precision = self.instrument.price_precision
        self.entry_buffer = self.instrument.tick_size.as_double() * 3.0
        self.SL_buffer = self.instrument.tick_size * 10.0
        self.position_sizer = FixedRiskSizer(self.instrument)

        # Request historical data
        self.request_bars(self.bar_type)

        # Subscribe to live data
        self.subscribe_instrument(self.symbol)
        self.subscribe_bars(self.bar_type)
        self.subscribe_ticks(self.symbol)

        # Register the indicators for updating
        self.register_indicator(
            data_source=self.bar_type,
            indicator=self.fast_ema,
            update_method=self.fast_ema.update)
        self.register_indicator(
            data_source=self.bar_type,
            indicator=self.slow_ema,
            update_method=self.slow_ema.update)
        self.register_indicator(
            data_source=self.bar_type,
            indicator=self.atr,
            update_method=self.atr.update)

    def on_tick(self, tick: Tick):
        """
        This method is called whenever a Tick is received by the strategy, and
        after the Tick has been processed by the base class.
        The received Tick object is then passed into this method.

        :param tick: The received tick.
        """
        # self.log.info(f"Received Tick({tick})")  # For debugging
        self.spreads.append(tick.ask.as_double() - tick.bid.as_double())

    def on_bar(self, bar_type: BarType, bar: Bar):
        """
        This method is called whenever the strategy receives a Bar, and after the
        Bar has been processed by the base class.
        The received BarType and Bar objects are then passed into this method.

        :param bar_type: The received bar type.
        :param bar: The received bar.
        """
        self.log.info(f"Received {bar_type} Bar({bar})")  # For debugging

        # Check if indicators ready
        if not self.indicators_initialized():
            return  # Wait for indicators to warm up...

        # Check if tick data available
        if not self.has_ticks(self.symbol):
            return  # Wait for ticks...

        # Calculate average spread
        average_spread = fast_mean(self.spreads)

        # Check market liquidity
        if average_spread == 0.0:
            return  # Protect divide by zero
        else:
            liquidity_ratio = self.atr.value / average_spread
            if liquidity_ratio < 2.0:
                self.log.debug(f"Liquidity Ratio == {liquidity_ratio} (no liquidity).")
                return

        spread_buffer = max(average_spread, self.spreads[-1])
        sl_buffer = self.atr.value * self.SL_atr_multiple

        if self.count_orders_working() == 0 and self.is_flat():  # No active or pending positions
            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                self._enter_long(bar, sl_buffer, spread_buffer)
            # SELL LOGIC
            elif self.fast_ema.value < self.slow_ema.value:
                self._enter_short(bar, sl_buffer, spread_buffer)

        self._check_trailing_stops(bar, sl_buffer, spread_buffer)

    def _enter_long(self, bar: Bar, sl_buffer: float, spread_buffer: float):
        price_entry = Price(bar.high + self.entry_buffer + spread_buffer, self.precision)
        price_stop_loss = Price(bar.low - sl_buffer, self.precision)

        risk = price_entry.as_double() - price_stop_loss.as_double()
        price_take_profit = Price(price_entry + risk, self.precision)

        exchange_rate = self.get_exchange_rate_for_account(
            quote_currency=self.instrument.quote_currency,
            price_type=PriceType.ASK)

        position_size = self.position_sizer.calculate(
            equity=self.account().free_equity,
            risk_bp=self.risk_bp,
            entry=price_entry,
            stop_loss=price_stop_loss,
            exchange_rate=exchange_rate,
            commission_rate_bp=0.15,
            hard_limit=20000000,
            units=1,
            unit_batch_size=10000)
        if position_size > 0:
            atomic_order = self.order_factory.atomic_stop_market(
                symbol=self.symbol,
                order_side=OrderSide.BUY,
                quantity=position_size,
                price_entry=price_entry,
                price_stop_loss=price_stop_loss,
                price_take_profit=price_take_profit,
                time_in_force=TimeInForce.GTD,
                expire_time=bar.timestamp + timedelta(minutes=1))

            self.submit_atomic_order(atomic_order, self.position_id_generator.generate())
        else:
            self.log.info("Insufficient equity for BUY signal.")

    def _enter_short(self, bar: Bar, sl_buffer: float, spread_buffer: float):
        price_entry = Price(bar.low - self.entry_buffer, self.precision)
        price_stop_loss = Price(bar.high + sl_buffer + spread_buffer, self.precision)

        risk = price_stop_loss.as_double() - price_entry.as_double()
        price_take_profit = Price(price_entry - risk, self.precision)

        exchange_rate = self.get_exchange_rate_for_account(
            quote_currency=self.instrument.quote_currency,
            price_type=PriceType.ASK)

        position_size = self.position_sizer.calculate(
            equity=self.account().free_equity,
            risk_bp=self.risk_bp,
            entry=price_entry,
            stop_loss=price_stop_loss,
            exchange_rate=exchange_rate,
            commission_rate_bp=0.15,
            hard_limit=20000000,
            units=1,
            unit_batch_size=10000)

        if position_size > 0:  # Sufficient equity for a position
            atomic_order = self.order_factory.atomic_stop_market(
                symbol=self.symbol,
                order_side=OrderSide.SELL,
                quantity=position_size,
                price_entry=price_entry,
                price_stop_loss=price_stop_loss,
                price_take_profit=price_take_profit,
                time_in_force=TimeInForce.GTD,
                expire_time=bar.timestamp + timedelta(minutes=1))

            self.submit_atomic_order(atomic_order, self.position_id_generator.generate())
        else:
            self.log.info("Insufficient equity for SELL signal.")

    def _check_trailing_stops(self, bar: Bar, sl_buffer: float, spread_buffer: float):
        for working_order in self.orders_working().values():
            if working_order.purpose == OrderPurpose.STOP_LOSS:
                # SELL SIDE ORDERS
                if working_order.is_sell:
                    temp_price = Price(bar.low - sl_buffer, self.precision)
                    if temp_price.gt(working_order.price):
                        self.modify_order(working_order, working_order.quantity, temp_price)
                # BUY SIDE ORDERS
                elif working_order.is_buy:
                    temp_price = Price(bar.high + sl_buffer + spread_buffer, self.precision)
                    if temp_price.lt(working_order.price):
                        self.modify_order(working_order, working_order.quantity, temp_price)

    def on_instrument(self, instrument: Instrument):
        """
        This method is called whenever the strategy receives an Instrument update.

        :param instrument: The received instrument.
        """
        if self.instrument.symbol.equal(instrument.symbol):
            self.instrument = instrument

        self.log.info(f"Updated instrument {instrument}.")

    def on_event(self, event: Event):
        """
        This method is called whenever the strategy receives an Event object,
        and after the event has been processed by the TradingStrategy base class.
        These events could be AccountEvent, OrderEvent, PositionEvent, TimeEvent.

        :param event: The received event.
        """
        # Put custom code for event handling here (or pass)
        pass

    def on_stop(self):
        """
        This method is called when self.stop() is called and after internal
        stopping logic.
        """
        # Put custom code to be run on strategy stop here (or pass)
        pass

    def on_reset(self):
        """
        This method is called when self.reset() is called, and after internal
        reset logic such as clearing the internally held bars, ticks and resetting
        all indicators.
        """
        # Put custom code to be run on a strategy reset here (or pass)
        pass

    def on_save(self) -> Dict:
        # Put custom state to be saved here (or return empty dictionary)
        return {}

    def on_load(self, state: Dict):
        # Put custom state to be loaded here (or pass)
        pass

    def on_dispose(self):
        """
        This method is called when self.dispose() is called. Dispose of any
        resources that has been used by the strategy here.
        """
        # Put custom code to be run on a strategy disposal here (or pass)
        self.unsubscribe_instrument(self.symbol)
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_ticks(self.symbol)
