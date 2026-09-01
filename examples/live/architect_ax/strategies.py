# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Example of Architect AX strategies.
"""

from __future__ import annotations

from decimal import Decimal
from typing import Any
from typing import Self

from nautilus_trader.common import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.indicators import BollingerBands
from nautilus_trader.indicators import RelativeStrengthIndex
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import TimeInForce
from nautilus_trader.trading import Strategy


class BBMeanReversionConfig(StrategyConfig):
    """
    Collect bbmean reversion config tests.
    """

    _CUSTOM_FIELDS = (
        "instrument_id",
        "bar_type",
        "trade_size",
        "bb_period",
        "bb_std",
        "rsi_period",
        "rsi_buy_threshold",
        "rsi_sell_threshold",
        "close_positions_on_stop",
    )

    def __new__(cls, *args: Any, **kwargs: Any) -> Self:
        """
        Create a new instance.
        """
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: InstrumentId,
        bar_type: BarType,
        trade_size: Decimal,
        bb_period: int = 20,
        bb_std: float = 2.0,
        rsi_period: int = 14,
        rsi_buy_threshold: float = 0.30,
        rsi_sell_threshold: float = 0.70,
        close_positions_on_stop: bool = True,
        **_kwargs: Any,
    ) -> None:
        """
        Initialize the helper.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size
        self.bb_period = bb_period
        self.bb_std = bb_std
        self.rsi_period = rsi_period
        self.rsi_buy_threshold = rsi_buy_threshold
        self.rsi_sell_threshold = rsi_sell_threshold
        self.close_positions_on_stop = close_positions_on_stop


class BBMeanReversion(Strategy):
    """
    Trade Bollinger Band mean reversion signals with RSI confirmation.
    """

    def __init__(self, config: BBMeanReversionConfig) -> None:
        """
        Initialize the helper.
        """
        if config.trade_size <= 0:
            raise ValueError("trade_size must be positive")

        super().__init__(config)
        self._instrument_id = config.instrument_id
        self._bar_type = config.bar_type
        self._trade_size = config.trade_size
        self._rsi_buy_threshold = config.rsi_buy_threshold
        self._rsi_sell_threshold = config.rsi_sell_threshold
        self._close_positions_on_stop = config.close_positions_on_stop
        self._instrument: Any | None = None
        self._trade_qty: Quantity | None = None
        self._bb = BollingerBands(config.bb_period, config.bb_std)
        self._rsi = RelativeStrengthIndex(config.rsi_period)

    def on_start(self) -> None:
        """
        On start.
        """
        self._instrument = self.cache.instrument(self._instrument_id)
        if self._instrument is None:
            log_msg = f"Could not find instrument for {self._instrument_id}"
            self.log.error(log_msg)
            self.stop()
            return

        self._trade_qty = Quantity.from_decimal_dp(
            self._trade_size,
            self._instrument.size_precision,
        )

        if self._trade_qty.as_decimal() <= 0:
            log_msg = f"Trade size {self._trade_size} rounds to zero for {self._instrument_id}"
            self.log.error(log_msg)
            self.stop()
            return

        self.register_indicator_for_bars(self._bar_type, self._bb)
        self.register_indicator_for_bars(self._bar_type, self._rsi)
        self.subscribe_bars(self._bar_type)

    def on_bar(self, bar: Bar) -> None:
        """
        On bar.
        """
        self.log.info(repr(bar), LogColor.CYAN)

        if not self.indicators_initialized():
            return
        if bar.open == bar.high == bar.low == bar.close:
            return

        close = bar.close.as_double()
        if not self._check_exit(close):
            self._check_entry(close)

    def on_stop(self) -> None:
        """
        On stop.
        """
        self.cancel_all_orders(self._instrument_id)
        if self._close_positions_on_stop:
            self.close_all_positions(self._instrument_id)
        self.unsubscribe_bars(self._bar_type)

    def on_reset(self) -> None:
        """
        On reset.
        """
        self._instrument = None
        self._trade_qty = None
        self._bb.reset()
        self._rsi.reset()

    def _check_exit(self, close: float) -> bool:
        if self.portfolio.is_net_long(self._instrument_id) and close >= self._bb.middle:
            self.close_all_positions(self._instrument_id)
            return True
        if self.portfolio.is_net_short(self._instrument_id) and close <= self._bb.middle:
            self.close_all_positions(self._instrument_id)
            return True
        return False

    def _check_entry(self, close: float) -> None:
        if close <= self._bb.lower and self._rsi.value < self._rsi_buy_threshold:
            if self.portfolio.is_net_short(self._instrument_id):
                self.close_all_positions(self._instrument_id)
            if not self.portfolio.is_net_long(self._instrument_id):
                self._submit_market_order(OrderSide.BUY)
        elif close >= self._bb.upper and self._rsi.value > self._rsi_sell_threshold:
            if self.portfolio.is_net_long(self._instrument_id):
                self.close_all_positions(self._instrument_id)
            if not self.portfolio.is_net_short(self._instrument_id):
                self._submit_market_order(OrderSide.SELL)

    def _submit_market_order(self, order_side: OrderSide) -> None:
        if self._trade_qty is None:
            return

        order = self.order_factory.market(
            instrument_id=self._instrument_id,
            order_side=order_side,
            quantity=self._trade_qty,
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)


class OrderBookImbalanceConfig(StrategyConfig):
    """
    Collect order book imbalance config tests.
    """

    _CUSTOM_FIELDS = (
        "instrument_id",
        "max_trade_size",
        "trigger_min_size",
        "trigger_imbalance_ratio",
        "min_seconds_between_triggers",
        "dry_run",
    )

    def __new__(cls, *args: Any, **kwargs: Any) -> Self:
        """
        Create a new instance.
        """
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: InstrumentId,
        max_trade_size: Decimal,
        trigger_min_size: Decimal = Decimal(100),
        trigger_imbalance_ratio: Decimal = Decimal("0.20"),
        min_seconds_between_triggers: float = 1.0,
        dry_run: bool = False,
        **_kwargs: Any,
    ) -> None:
        """
        Initialize the helper.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.max_trade_size = max_trade_size
        self.trigger_min_size = trigger_min_size
        self.trigger_imbalance_ratio = trigger_imbalance_ratio
        self.min_seconds_between_triggers = min_seconds_between_triggers
        self.dry_run = dry_run


class OrderBookImbalance(Strategy):
    """
    Send FOK limit orders when AX top-of-book sizes become imbalanced.
    """

    def __init__(self, config: OrderBookImbalanceConfig) -> None:
        """
        Initialize the helper.
        """
        if config.max_trade_size <= 0:
            raise ValueError("max_trade_size must be positive")
        if config.trigger_min_size <= 0:
            raise ValueError("trigger_min_size must be positive")
        if not Decimal(0) < config.trigger_imbalance_ratio < Decimal(1):
            raise ValueError("trigger_imbalance_ratio must be between 0 and 1")
        if config.min_seconds_between_triggers < 0:
            raise ValueError("min_seconds_between_triggers must be non-negative")

        super().__init__(config)
        self._instrument_id = config.instrument_id
        self._max_trade_size = config.max_trade_size
        self._trigger_min_size = config.trigger_min_size
        self._trigger_imbalance_ratio = config.trigger_imbalance_ratio
        self._trigger_interval_ns = int(config.min_seconds_between_triggers * 1_000_000_000)
        self._dry_run = config.dry_run
        self._instrument: Any | None = None
        self._last_trigger_ns: int | None = None

    def on_start(self) -> None:
        """
        On start.
        """
        self._instrument = self.cache.instrument(self._instrument_id)
        if self._instrument is None:
            log_msg = f"Could not find instrument for {self._instrument_id}"
            self.log.error(log_msg)
            self.stop()
            return

        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, quote: QuoteTick) -> None:
        """
        On quote.
        """
        bid_size = quote.bid_size.as_decimal()
        ask_size = quote.ask_size.as_decimal()
        if bid_size <= 0 or ask_size <= 0:
            return

        smaller = min(bid_size, ask_size)
        larger = max(bid_size, ask_size)
        ratio = smaller / larger
        if larger <= self._trigger_min_size or ratio >= self._trigger_imbalance_ratio:
            return

        now = self.clock.timestamp_ns()
        if (
            self._last_trigger_ns is not None
            and now - self._last_trigger_ns < self._trigger_interval_ns
        ):
            return
        if self.cache.orders_inflight(strategy_id=self.strategy_id):
            return

        if bid_size > ask_size:
            order_side = OrderSide.BUY
            price = quote.ask_price
            level_size = ask_size
        else:
            order_side = OrderSide.SELL
            price = quote.bid_price
            level_size = bid_size

        self._last_trigger_ns = now
        if self._dry_run or self._instrument is None:
            return

        quantity = Quantity.from_decimal_dp(
            min(level_size, self._max_trade_size),
            self._instrument.size_precision,
        )

        if quantity.as_decimal() <= 0:
            log_msg = f"Trade quantity rounds to zero for {self._instrument_id}"
            self.log.error(log_msg)
            return

        order = self.order_factory.limit(
            instrument_id=self._instrument_id,
            order_side=order_side,
            quantity=quantity,
            price=price,
            time_in_force=TimeInForce.FOK,
            post_only=False,
        )
        self.submit_order(order)

    def on_stop(self) -> None:
        """
        On stop.
        """
        self.cancel_all_orders(self._instrument_id)
        if not self._dry_run:
            self.close_all_positions(self._instrument_id)
        self.unsubscribe_quotes(self._instrument_id)

    def on_reset(self) -> None:
        """
        On reset.
        """
        self._instrument = None
        self._last_trigger_ns = None
