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
Strategies used by acceptance tests.

Each strategy is importable via ImportableStrategyConfig. They aim to exercise specific
engine behaviours — multi-cycle PnL accounting, cascading order submission, timer
firing, etc.

"""

from __future__ import annotations

from datetime import UTC
from datetime import datetime
from decimal import Decimal

from nautilus_trader.core import UUID4
from nautilus_trader.indicators import MovingAverageConvergenceDivergence
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import LimitOrder
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StopMarketOrder
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeTick
from nautilus_trader.model import TrailingOffsetType
from nautilus_trader.model import TriggerType
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


def _market_order(
    strategy: Strategy,
    instrument_id: InstrumentId,
    side: OrderSide,
    qty: Quantity,
) -> MarketOrder:
    return MarketOrder(
        trader_id=strategy.trader_id,
        strategy_id=strategy.strategy_id,
        instrument_id=instrument_id,
        client_order_id=ClientOrderId(f"{strategy.strategy_id}-{UUID4()}"),
        order_side=side,
        quantity=qty,
        init_id=UUID4(),
        ts_init=strategy.clock.timestamp_ns(),
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )


def _stop_market_order(
    strategy: Strategy,
    instrument_id: InstrumentId,
    side: OrderSide,
    qty: Quantity,
    trigger_price: Price,
) -> StopMarketOrder:
    return StopMarketOrder(
        trader_id=strategy.trader_id,
        strategy_id=strategy.strategy_id,
        instrument_id=instrument_id,
        client_order_id=ClientOrderId(f"{strategy.strategy_id}-{UUID4()}"),
        order_side=side,
        quantity=qty,
        trigger_price=trigger_price,
        trigger_type=TriggerType.LAST_PRICE,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=strategy.clock.timestamp_ns(),
    )


def _limit_order(
    strategy: Strategy,
    instrument_id: InstrumentId,
    side: OrderSide,
    qty: Quantity,
    price: Price,
) -> LimitOrder:
    return LimitOrder(
        trader_id=strategy.trader_id,
        strategy_id=strategy.strategy_id,
        instrument_id=instrument_id,
        client_order_id=ClientOrderId(f"{strategy.strategy_id}-{UUID4()}"),
        order_side=side,
        quantity=qty,
        price=price,
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=strategy.clock.timestamp_ns(),
    )


# `StrategyConfig` is a pyo3 `@final` type whose `__new__` validates kwargs
# against the base schema and `__init__` falls through to `object.__init__`.
# Each subclass below therefore overrides `__new__` to strip its own custom
# fields and forward the remaining kwargs (`order_id_tag`, `log_events`, etc.)
# to the base; `__init__` only stores the custom fields.


class BarEntryExitConfig(StrategyConfig):
    _CUSTOM_FIELDS = ("instrument_id", "bar_type", "trade_size", "entry_bar", "exit_bar")

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        bar_type: str,
        trade_size: str,
        entry_bar: int = 0,
        exit_bar: int = 10,
        **kwargs,
    ):
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size
        self.entry_bar = entry_bar
        self.exit_bar = exit_bar


class BarEntryExit(Strategy):
    """
    Submit a single buy market order on entry_bar and a single sell on exit_bar.
    """

    def __init__(self, config: BarEntryExitConfig):
        super().__init__(config)
        from nautilus_trader.model import BarType

        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._bar_type = BarType.from_str(config.bar_type)
        self._qty = Quantity.from_str(config.trade_size)
        self._entry_bar = config.entry_bar
        self._exit_bar = config.exit_bar
        self._bar_count = 0

    def on_start(self):
        self.subscribe_bars(self._bar_type)

    def on_bar(self, bar: Bar):
        if self._bar_count == self._entry_bar:
            self.submit_order(
                _market_order(self, self._instrument_id, OrderSide.BUY, self._qty),
            )
        elif self._bar_count == self._exit_bar:
            self.submit_order(
                _market_order(self, self._instrument_id, OrderSide.SELL, self._qty),
            )
        self._bar_count += 1

    def on_stop(self):
        pass


class TickScheduledConfig(StrategyConfig):
    """
    Submit a market order at each (tick_index, side, quantity) entry in `actions`.
    """

    _CUSTOM_FIELDS = ("instrument_id", "actions")

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        actions: list,
        **kwargs,
    ):
        super().__init__()
        self.instrument_id = instrument_id
        self.actions = actions


class TickScheduled(Strategy):
    def __init__(self, config: TickScheduledConfig):
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._actions: dict[int, list[tuple[OrderSide, Quantity]]] = {}

        for entry in config.actions:
            idx = int(entry[0])
            side = OrderSide.BUY if str(entry[1]).upper() == "BUY" else OrderSide.SELL
            qty = Quantity.from_str(str(entry[2]))
            self._actions.setdefault(idx, []).append((side, qty))

        self._tick_count = 0

    def on_start(self):
        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, tick: QuoteTick):
        self._tick_count += 1
        for side, qty in self._actions.get(self._tick_count, []):
            self.submit_order(_market_order(self, self._instrument_id, side, qty))

    def on_stop(self):
        pass


class MultiInstrumentTickScheduledConfig(StrategyConfig):
    """
    Submit market orders from an instrument keyed action schedule.
    """

    _CUSTOM_FIELDS = ("instrument_actions",)

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_actions: dict,
        **kwargs,
    ):
        super().__init__()
        self.instrument_actions = instrument_actions


class MultiInstrumentTickScheduled(Strategy):
    def __init__(self, config: MultiInstrumentTickScheduledConfig):
        super().__init__(config)
        self._actions: dict[InstrumentId, dict[int, list[tuple[OrderSide, Quantity]]]] = {}
        self._tick_counts: dict[InstrumentId, int] = {}

        for raw_instrument_id, actions in config.instrument_actions.items():
            instrument_id = InstrumentId.from_str(str(raw_instrument_id))
            instrument_actions: dict[int, list[tuple[OrderSide, Quantity]]] = {}
            for entry in actions:
                idx = int(entry[0])
                side = OrderSide.BUY if str(entry[1]).upper() == "BUY" else OrderSide.SELL
                qty = Quantity.from_str(str(entry[2]))
                instrument_actions.setdefault(idx, []).append((side, qty))

            self._actions[instrument_id] = instrument_actions
            self._tick_counts[instrument_id] = 0

    def on_start(self):
        for instrument_id in self._actions:
            self.subscribe_quotes(instrument_id)

    def on_quote(self, tick: QuoteTick):
        instrument_id = tick.instrument_id
        self._tick_counts[instrument_id] += 1
        for side, qty in self._actions[instrument_id].get(self._tick_counts[instrument_id], []):
            self.submit_order(_market_order(self, instrument_id, side, qty))

    def on_stop(self):
        pass


class EMACrossStopEntryConfig(StrategyConfig):
    _CUSTOM_FIELDS = (
        "instrument_id",
        "bar_type",
        "trade_size",
        "fast_ema_period",
        "slow_ema_period",
        "atr_period",
        "trailing_atr_multiple",
        "trailing_offset_type",
        "trigger_type",
        "emulation_trigger",
    )

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        bar_type: str,
        trade_size: str,
        fast_ema_period: int = 10,
        slow_ema_period: int = 20,
        atr_period: int = 20,
        trailing_atr_multiple: float = 3.0,
        trailing_offset_type: str = "PRICE",
        trigger_type: str = "LAST_PRICE",
        emulation_trigger: str = "NO_TRIGGER",
        **kwargs,
    ):
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size
        self.fast_ema_period = fast_ema_period
        self.slow_ema_period = slow_ema_period
        self.atr_period = atr_period
        self.trailing_atr_multiple = trailing_atr_multiple
        self.trailing_offset_type = trailing_offset_type
        self.trigger_type = trigger_type
        self.emulation_trigger = emulation_trigger


class EMACrossTrailingStopConfig(StrategyConfig):
    _CUSTOM_FIELDS = (
        "instrument_id",
        "bar_type",
        "trade_size",
        "fast_ema_period",
        "slow_ema_period",
        "atr_period",
        "trailing_atr_multiple",
        "trailing_offset_type",
        "trigger_type",
        "emulation_trigger",
    )

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        bar_type: str,
        trade_size: str,
        fast_ema_period: int = 10,
        slow_ema_period: int = 20,
        atr_period: int = 20,
        trailing_atr_multiple: float = 2.0,
        trailing_offset_type: str = "PRICE",
        trigger_type: str = "BID_ASK",
        emulation_trigger: str = "BID_ASK",
        **kwargs,
    ):
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size
        self.fast_ema_period = fast_ema_period
        self.slow_ema_period = slow_ema_period
        self.atr_period = atr_period
        self.trailing_atr_multiple = trailing_atr_multiple
        self.trailing_offset_type = trailing_offset_type
        self.trigger_type = trigger_type
        self.emulation_trigger = emulation_trigger


class _EMACrossTrailingWorkflow(Strategy):
    def __init__(self, config):
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._bar_type = BarType.from_str(config.bar_type)
        self._trade_size = Decimal(str(config.trade_size))
        self._fast_period = config.fast_ema_period
        self._slow_period = config.slow_ema_period
        self._atr_period = config.atr_period
        self._fast_alpha = Decimal(2) / Decimal(self._fast_period + 1)
        self._slow_alpha = Decimal(2) / Decimal(self._slow_period + 1)
        self._trailing_atr_multiple = Decimal(str(config.trailing_atr_multiple))
        self._trailing_offset_type = TrailingOffsetType.from_str(config.trailing_offset_type)
        self._trigger_type = TriggerType.from_str(config.trigger_type)
        self._emulation_trigger = TriggerType.from_str(config.emulation_trigger)

        self._instrument = None
        self._tick_size = Decimal(0)
        self._bar_count = 0
        self._fast_ema = Decimal(0)
        self._slow_ema = Decimal(0)
        self._atr = Decimal(0)
        self._prev_close = Decimal(0)
        self.entry = None
        self.trailing_stop = None

    def on_start(self):
        self._instrument = self.cache.instrument(self._instrument_id)
        if self._instrument is None:
            self.log.error(f"Could not find instrument for {self._instrument_id}")
            self.stop()
            return

        self._tick_size = self._instrument.price_increment.as_decimal()
        self.subscribe_bars(self._bar_type)
        self.subscribe_quotes(self._instrument_id)
        self.subscribe_trades(self._instrument_id)

    def on_bar(self, bar: Bar):
        self._update_indicators(bar)
        if not self._indicators_ready():
            return
        if not self.portfolio.is_flat(self._instrument_id):
            return
        if self.entry is not None and not self.entry.is_closed():
            return

        if self._fast_ema >= self._slow_ema:
            self._submit_entry(OrderSide.BUY, bar)
        else:
            self._submit_entry(OrderSide.SELL, bar)

    def on_order_filled(self, event: OrderFilled):
        if self.entry and event.client_order_id == self.entry.client_order_id:
            if event.order_side == OrderSide.BUY:
                self._submit_trailing_stop(OrderSide.SELL, event.last_px, event.position_id)
            else:
                self._submit_trailing_stop(OrderSide.BUY, event.last_px, event.position_id)
        elif self.trailing_stop and event.client_order_id == self.trailing_stop.client_order_id:
            self.entry = None
            self.trailing_stop = None

    def on_order_rejected(self, event):
        self._clear_terminal_entry(event.client_order_id)

    def on_order_canceled(self, event):
        self._clear_terminal_entry(event.client_order_id)

    def on_order_expired(self, event):
        self._clear_terminal_entry(event.client_order_id)

    def on_stop(self):
        self.cancel_all_orders(self._instrument_id)
        self.close_all_positions(self._instrument_id)

    def on_reset(self):
        self._bar_count = 0
        self._fast_ema = Decimal(0)
        self._slow_ema = Decimal(0)
        self._atr = Decimal(0)
        self._prev_close = Decimal(0)
        self.entry = None
        self.trailing_stop = None

    def _submit_entry(self, side: OrderSide, bar: Bar):
        raise NotImplementedError

    def _submit_trailing_stop(self, side: OrderSide, activation_price: Price, position_id):
        if self._instrument is None:
            self.log.error("No instrument loaded")
            return

        offset = self._atr * self._trailing_atr_multiple
        trailing_offset = Decimal(f"{offset:.{self._instrument.price_precision}f}")
        order = self.order_factory.trailing_stop_market(
            instrument_id=self._instrument_id,
            order_side=side,
            quantity=self._instrument.make_qty(self._trade_size),
            trailing_offset=trailing_offset,
            trailing_offset_type=self._trailing_offset_type,
            activation_price=activation_price,
            trigger_type=self._trigger_type,
            reduce_only=True,
            emulation_trigger=self._emulation_trigger,
            tags=["ema-cross-trailing-stop"],
        )

        self.trailing_stop = order
        self.submit_order(order, position_id=position_id)

    def _update_indicators(self, bar: Bar):
        high = bar.high.as_decimal()
        low = bar.low.as_decimal()
        close = bar.close.as_decimal()
        true_range = high - low
        if self._bar_count > 0:
            true_range = max(true_range, abs(high - self._prev_close), abs(low - self._prev_close))

        self._bar_count += 1
        if self._bar_count == 1:
            self._fast_ema = close
            self._slow_ema = close
            self._atr = true_range
        else:
            self._fast_ema = (
                self._fast_alpha * close + (Decimal(1) - self._fast_alpha) * self._fast_ema
            )
            self._slow_ema = (
                self._slow_alpha * close + (Decimal(1) - self._slow_alpha) * self._slow_ema
            )
            self._atr = ((self._atr * Decimal(self._atr_period - 1)) + true_range) / Decimal(
                self._atr_period,
            )
        self._prev_close = close

    def _indicators_ready(self) -> bool:
        return self._bar_count >= max(self._slow_period, self._atr_period)

    def _clear_terminal_entry(self, client_order_id: ClientOrderId):
        if self.entry and client_order_id == self.entry.client_order_id:
            self.entry = None


class EMACrossStopEntry(_EMACrossTrailingWorkflow):
    def _submit_entry(self, side: OrderSide, bar: Bar):
        if self._instrument is None:
            self.log.error("No instrument loaded")
            return

        if side == OrderSide.BUY:
            trigger_price = bar.high.as_decimal() + (self._tick_size * 2)
        else:
            trigger_price = bar.low.as_decimal() - (self._tick_size * 2)

        order = self.order_factory.market_if_touched(
            instrument_id=self._instrument_id,
            order_side=side,
            quantity=self._instrument.make_qty(self._trade_size),
            trigger_price=self._instrument.make_price(trigger_price),
            trigger_type=self._trigger_type,
            time_in_force=TimeInForce.IOC,
            emulation_trigger=self._emulation_trigger,
        )

        self.entry = order
        self.submit_order(order)


class EMACrossTrailingStop(_EMACrossTrailingWorkflow):
    def _submit_entry(self, side: OrderSide, bar: Bar):
        if self._instrument is None:
            self.log.error("No instrument loaded")
            return

        order = self.order_factory.market(
            instrument_id=self._instrument_id,
            order_side=side,
            quantity=self._instrument.make_qty(self._trade_size),
        )

        self.entry = order
        self.submit_order(order)


class CascadingStopConfig(StrategyConfig):
    _CUSTOM_FIELDS = ("instrument_id", "trade_size", "stop_price")

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        trade_size: str,
        stop_price: str,
        **kwargs,
    ):
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = trade_size
        self.stop_price = stop_price


class CascadingStop(Strategy):
    def __init__(self, config: CascadingStopConfig):
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._qty = Quantity.from_str(config.trade_size)
        self._stop_price = Price.from_str(config.stop_price)
        self._tick_count = 0
        self._entry_filled = False

    def on_start(self):
        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, tick: QuoteTick):
        self._tick_count += 1
        if self._tick_count == 1:
            self.submit_order(
                _market_order(self, self._instrument_id, OrderSide.BUY, self._qty),
            )

    def on_order_filled(self, event: OrderFilled):
        if not self._entry_filled:
            self._entry_filled = True
            self.submit_order(
                _stop_market_order(
                    self,
                    self._instrument_id,
                    OrderSide.SELL,
                    self._qty,
                    self._stop_price,
                ),
            )

    def on_stop(self):
        pass


class MultiCascadeConfig(StrategyConfig):
    _CUSTOM_FIELDS = ("instrument_id", "trade_size", "stop_price", "limit_price")

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        trade_size: str,
        stop_price: str,
        limit_price: str,
        **kwargs,
    ):
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = trade_size
        self.stop_price = stop_price
        self.limit_price = limit_price


class MultiCascade(Strategy):
    def __init__(self, config: MultiCascadeConfig):
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._qty = Quantity.from_str(config.trade_size)
        self._stop_price = Price.from_str(config.stop_price)
        self._limit_price = Price.from_str(config.limit_price)
        self._tick_count = 0
        self._entry_filled = False
        self._stop_accepted = False
        self._stop_client_order_id: ClientOrderId | None = None

    def on_start(self):
        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, tick: QuoteTick):
        self._tick_count += 1
        if self._tick_count == 1:
            self.submit_order(
                _market_order(self, self._instrument_id, OrderSide.BUY, self._qty),
            )

    def on_order_filled(self, event: OrderFilled):
        if not self._entry_filled:
            self._entry_filled = True
            stop_order = _stop_market_order(
                self,
                self._instrument_id,
                OrderSide.SELL,
                self._qty,
                self._stop_price,
            )
            self._stop_client_order_id = stop_order.client_order_id
            self.submit_order(stop_order)

    def on_order_accepted(self, event):
        if (
            self._stop_client_order_id is not None
            and event.client_order_id == self._stop_client_order_id
            and not self._stop_accepted
        ):
            self._stop_accepted = True
            self.submit_order(
                _limit_order(
                    self,
                    self._instrument_id,
                    OrderSide.SELL,
                    self._qty,
                    self._limit_price,
                ),
            )

    def on_stop(self):
        pass


class DualTimerConfig(StrategyConfig):
    _CUSTOM_FIELDS = ("instrument_id", "trade_size", "alert_iso")

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        trade_size: str,
        alert_iso: str,
        **kwargs,
    ):
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = trade_size
        self.alert_iso = alert_iso  # ISO 8601 (e.g. "2020-01-01T00:00:30+00:00")


class DualTimer(Strategy):
    def __init__(self, config: DualTimerConfig):
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._qty = Quantity.from_str(config.trade_size)
        self._alert_iso = config.alert_iso
        self.fired_a = False
        self.fired_b = False

    def on_start(self):
        self.subscribe_quotes(self._instrument_id)

        iso = self._alert_iso
        if iso.endswith("Z"):
            iso = iso[:-1] + "+00:00"
        elif "+" not in iso and "T" in iso:
            iso = iso + "+00:00"
        alert_time = datetime.fromisoformat(iso).astimezone(UTC)

        self.clock.set_time_alert("timer_a", alert_time, self._on_timer_a)
        self.clock.set_time_alert("timer_b", alert_time, self._on_timer_b)

    def _on_timer_a(self, event):
        self.fired_a = True
        self.submit_order(
            _market_order(self, self._instrument_id, OrderSide.BUY, self._qty),
        )

    def _on_timer_b(self, event):
        self.fired_b = True
        self.submit_order(
            _market_order(self, self._instrument_id, OrderSide.SELL, self._qty),
        )

    def on_stop(self):
        pass


class MACDStrategyConfig(StrategyConfig):
    _CUSTOM_FIELDS = (
        "instrument_id",
        "trade_size",
        "fast_period",
        "slow_period",
        "entry_threshold",
    )

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        trade_size: str,
        fast_period: int = 12,
        slow_period: int = 26,
        entry_threshold: float = 0.00010,
        **kwargs,
    ):
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = trade_size
        self.fast_period = fast_period
        self.slow_period = slow_period
        self.entry_threshold = entry_threshold


class MACDTradeTickStrategy(Strategy):
    """
    A simplified port of the v1 MACD blackbox strategy.

    Drives entries and exits off MACD on trade ticks. Tracks a position-side flag
    locally rather than relying on portfolio state, since acceptance focuses on event-
    sequencing rather than PnL.

    """

    def __init__(self, config: MACDStrategyConfig):
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._qty = Quantity.from_str(config.trade_size)
        self._entry_threshold = config.entry_threshold
        self.macd = MovingAverageConvergenceDivergence(
            fast_period=config.fast_period,
            slow_period=config.slow_period,
            price_type=PriceType.MID,
        )
        self._position_side: OrderSide | None = None  # None=flat, BUY=long, SELL=short

    def on_start(self):
        self.subscribe_trades(self._instrument_id)

    def on_trade(self, tick: TradeTick):
        self.macd.handle_trade_tick(tick)
        if not self.macd.initialized:
            return

        value = self.macd.value

        if value > self._entry_threshold and self._position_side != OrderSide.BUY:
            if self._position_side == OrderSide.SELL:
                self.submit_order(
                    _market_order(self, self._instrument_id, OrderSide.BUY, self._qty),
                )
            self.submit_order(
                _market_order(self, self._instrument_id, OrderSide.BUY, self._qty),
            )
            self._position_side = OrderSide.BUY
        elif value < -self._entry_threshold and self._position_side != OrderSide.SELL:
            if self._position_side == OrderSide.BUY:
                self.submit_order(
                    _market_order(self, self._instrument_id, OrderSide.SELL, self._qty),
                )
            self.submit_order(
                _market_order(self, self._instrument_id, OrderSide.SELL, self._qty),
            )
            self._position_side = OrderSide.SELL

    def on_stop(self):
        pass
