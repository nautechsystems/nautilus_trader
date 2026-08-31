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
Test backtest surface behavior.
"""

from __future__ import annotations

from decimal import Decimal
from typing import ClassVar

from nautilus_trader.common import DataActor
from nautilus_trader.common import DataActorConfig
from nautilus_trader.common import GreeksCalculator
from nautilus_trader.core import UUID4
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import LimitOrder
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeTick
from nautilus_trader.trading import ExecutionAlgorithm
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


class SignalHarvestConfig(StrategyConfig):
    """
    Collect signal harvest config tests.
    """

    _CUSTOM_FIELDS = ("instrument_id", "bar_type", "trade_size")

    def __new__(cls, *args: object, **kwargs: object) -> object:
        """
        Create a new instance.
        """
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        bar_type: str,
        trade_size: str,
        **_kwargs: object,
    ) -> None:
        """
        Initialize the helper.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size


class SignalHarvest(Strategy):
    """
    Collect signal harvest tests.
    """

    def __init__(self, config: SignalHarvestConfig) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._bar_type = BarType.from_str(config.bar_type)
        self._qty = Quantity.from_str(config.trade_size)
        self._instrument = None
        self._bar_count = 0
        self._trade_count = 0
        self._mark_count = 0
        self._index_count = 0
        self._funding_count = 0
        self._status_count = 0
        self._close_count = 0
        self._fast = Decimal(0)
        self._slow = Decimal(0)
        self._gains = Decimal(0)
        self._losses = Decimal(0)
        self._prev_close: Decimal | None = None
        self._entered = False
        self._limit_sent = False
        self._cancel_sent = False
        self._exit_sent = False
        self._order_count = 0

    def on_start(self) -> None:
        """
        On start.
        """
        self._instrument = self.cache.instrument(self._instrument_id)
        self.subscribe_bars(self._bar_type)
        self.subscribe_trades(self._instrument_id)
        self.subscribe_mark_prices(self._instrument_id)
        self.subscribe_index_prices(self._instrument_id)
        self.subscribe_funding_rates(self._instrument_id)
        self.subscribe_instrument_status(self._instrument_id)
        self.subscribe_instrument_close(self._instrument_id)

    def on_bar(self, bar: Bar) -> None:
        """
        On bar.
        """
        self._bar_count += 1
        self._update_bar_state(bar)

        if self._bar_count >= 6 and self._all_auxiliary_data_seen() and not self._entered:
            self._submit_market(OrderSide.BUY)
            self._entered = True
        elif self._bar_count >= 8 and self._entered and not self._limit_sent:
            self._submit_limit(OrderSide.SELL, self._resting_exit_price(bar.close))
            self._limit_sent = True
        elif self._bar_count >= 10 and self._limit_sent and not self._cancel_sent:
            self.cancel_all_orders(self._instrument_id)
            self._cancel_sent = True
        elif self._bar_count >= 12 and self._entered and not self._exit_sent:
            self._submit_market(OrderSide.SELL)
            self._exit_sent = True

    def on_trade(self, _trade: TradeTick) -> None:
        """
        On trade.
        """
        self._trade_count += 1

    def on_mark_price(self, _mark_price: MarkPriceUpdate) -> None:
        """
        On mark price.
        """
        self._mark_count += 1

    def on_index_price(self, _index_price: IndexPriceUpdate) -> None:
        """
        On index price.
        """
        self._index_count += 1

    def on_funding_rate(self, _funding_rate: FundingRateUpdate) -> None:
        """
        On funding rate.
        """
        self._funding_count += 1

    def on_instrument_status(self, _status: InstrumentStatus) -> None:
        """
        On instrument status.
        """
        self._status_count += 1

    def on_instrument_close(self, _close: InstrumentClose) -> None:
        """
        On instrument close.
        """
        self._close_count += 1

    def on_reset(self) -> None:
        """
        On reset.
        """
        self._bar_count = 0
        self._trade_count = 0
        self._mark_count = 0
        self._index_count = 0
        self._funding_count = 0
        self._status_count = 0
        self._close_count = 0
        self._fast = Decimal(0)
        self._slow = Decimal(0)
        self._gains = Decimal(0)
        self._losses = Decimal(0)
        self._prev_close = None
        self._entered = False
        self._limit_sent = False
        self._cancel_sent = False
        self._exit_sent = False
        self._order_count = 0

    def on_stop(self) -> None:
        """
        On stop.
        """
        self.cancel_all_orders(self._instrument_id)
        self.close_all_positions(self._instrument_id)

    def _update_bar_state(self, bar: Bar) -> None:
        close = bar.close.as_decimal()
        if self._bar_count == 1:
            self._fast = close
            self._slow = close
            self._prev_close = close
            return

        prev_close = self._prev_close if self._prev_close is not None else close
        diff = close - prev_close
        self._gains = (self._gains * Decimal("0.8")) + max(diff, Decimal(0))
        self._losses = (self._losses * Decimal("0.8")) + max(-diff, Decimal(0))
        self._fast = (close * Decimal("0.35")) + (self._fast * Decimal("0.65"))
        self._slow = (close * Decimal("0.12")) + (self._slow * Decimal("0.88"))
        self._prev_close = close

    def _all_auxiliary_data_seen(self) -> bool:
        return all(
            count > 0
            for count in (
                self._trade_count,
                self._mark_count,
                self._index_count,
                self._funding_count,
                self._status_count,
                self._close_count,
            )
        )

    def _resting_exit_price(self, close: Price) -> Price:
        if self._instrument is None:
            return close
        return self._instrument.make_price(
            close.as_decimal() + (self._instrument.price_increment.as_decimal() * Decimal(25)),
        )

    def _submit_market(self, side: OrderSide) -> None:
        self._order_count += 1
        self.submit_order(
            MarketOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=self._instrument_id,
                client_order_id=ClientOrderId(f"{self.strategy_id}-{self._order_count}"),
                order_side=side,
                quantity=self._qty,
                init_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                time_in_force=TimeInForce.GTC,
                reduce_only=False,
                quote_quantity=False,
                contingency_type=None,
            ),
        )

    def _submit_limit(self, side: OrderSide, price: Price) -> None:
        self._order_count += 1
        self.submit_order(
            LimitOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=self._instrument_id,
                client_order_id=ClientOrderId(f"{self.strategy_id}-{self._order_count}"),
                order_side=side,
                quantity=self._qty,
                price=price,
                time_in_force=TimeInForce.GTC,
                post_only=False,
                reduce_only=False,
                quote_quantity=False,
                init_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )


class BookChurnConfig(StrategyConfig):
    """
    Collect book churn config tests.
    """

    _CUSTOM_FIELDS = ("instrument_id", "trade_size")

    def __new__(cls, *args: object, **kwargs: object) -> object:
        """
        Create a new instance.
        """
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(self, instrument_id: str, trade_size: str, **_kwargs: object) -> None:
        """
        Initialize the helper.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = trade_size


class BookChurn(Strategy):
    """
    Collect book churn tests.
    """

    def __init__(self, config: BookChurnConfig) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._qty = Quantity.from_str(config.trade_size)
        self._book_count = 0
        self._delta_count = 0
        self._order_count = 0
        self._entry_sent = False
        self._resting_sent = False
        self._cancel_sent = False
        self._exit_sent = False

    def on_start(self) -> None:
        """
        On start.
        """
        self.subscribe_book_deltas(self._instrument_id, BookType.L2_MBP, depth=10)
        self.subscribe_book_at_interval(self._instrument_id, BookType.L2_MBP, interval_ms=1_000)

    def on_book_deltas(self, _deltas: OrderBookDeltas) -> None:
        """
        On book deltas.
        """
        self._delta_count += 1
        if self._delta_count >= 1 and not self._entry_sent:
            self._submit_market(OrderSide.BUY)
            self._entry_sent = True
        elif self._delta_count >= 2 and not self._resting_sent:
            self._submit_limit(OrderSide.SELL, Price.from_str("2010.00"))
            self._resting_sent = True
        elif self._delta_count >= 3 and not self._cancel_sent:
            self.cancel_all_orders(self._instrument_id)
            self._cancel_sent = True
        elif self._delta_count >= 4 and not self._exit_sent:
            self._submit_market(OrderSide.SELL)
            self._exit_sent = True

    def on_book(self, _book: OrderBook) -> None:
        """
        On book.
        """
        self._book_count += 1

    def on_reset(self) -> None:
        """
        On reset.
        """
        self._book_count = 0
        self._delta_count = 0
        self._order_count = 0
        self._entry_sent = False
        self._resting_sent = False
        self._cancel_sent = False
        self._exit_sent = False

    def on_stop(self) -> None:
        """
        On stop.
        """
        self.cancel_all_orders(self._instrument_id)
        self.close_all_positions(self._instrument_id)

    def _submit_market(self, side: OrderSide) -> None:
        self._order_count += 1
        self.submit_order(
            MarketOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=self._instrument_id,
                client_order_id=ClientOrderId(f"{self.strategy_id}-{self._order_count}"),
                order_side=side,
                quantity=self._qty,
                init_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                time_in_force=TimeInForce.GTC,
                reduce_only=False,
                quote_quantity=False,
                contingency_type=None,
            ),
        )

    def _submit_limit(self, side: OrderSide, price: Price) -> None:
        self._order_count += 1
        self.submit_order(
            LimitOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=self._instrument_id,
                client_order_id=ClientOrderId(f"{self.strategy_id}-{self._order_count}"),
                order_side=side,
                quantity=self._qty,
                price=price,
                time_in_force=TimeInForce.GTC,
                post_only=False,
                reduce_only=False,
                quote_quantity=False,
                init_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
            ),
        )


class RoutedOrderProbeConfig(StrategyConfig):
    """
    Collect routed order probe config tests.
    """

    _CUSTOM_FIELDS = ("instrument_id", "trade_size", "exec_algorithm_id")

    def __new__(cls, *args: object, **kwargs: object) -> object:
        """
        Create a new instance.
        """
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        trade_size: str,
        exec_algorithm_id: str,
        **_kwargs: object,
    ) -> None:
        """
        Initialize the helper.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = trade_size
        self.exec_algorithm_id = exec_algorithm_id


class RoutedOrderProbe(Strategy):
    """
    Collect routed order probe tests.
    """

    def __init__(self, config: RoutedOrderProbeConfig) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._qty = Quantity.from_str(config.trade_size)
        self._exec_algorithm_id = ExecAlgorithmId(config.exec_algorithm_id)
        self._sent = False

    def on_start(self) -> None:
        """
        On start.
        """
        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, _quote: QuoteTick) -> None:
        """
        On quote.
        """
        if not self._sent:
            self._sent = True
            client_order_id = ClientOrderId(f"{self.strategy_id}-1")
            self.submit_order(
                MarketOrder(
                    trader_id=self.trader_id,
                    strategy_id=self.strategy_id,
                    instrument_id=self._instrument_id,
                    client_order_id=client_order_id,
                    order_side=OrderSide.BUY,
                    quantity=self._qty,
                    init_id=UUID4(),
                    ts_init=self.clock.timestamp_ns(),
                    time_in_force=TimeInForce.GTC,
                    reduce_only=False,
                    quote_quantity=False,
                    contingency_type=None,
                    exec_algorithm_id=self._exec_algorithm_id,
                    exec_spawn_id=client_order_id,
                ),
            )

    def on_reset(self) -> None:
        """
        On reset.
        """
        self._sent = False


class RoutedOrderExecutionAlgorithmConfig(DataActorConfig):
    """
    Collect routed order execution algorithm config tests.
    """

    _CUSTOM_FIELDS = ("exec_algorithm_id", "signal_name")

    def __new__(cls, *args: object, **kwargs: object) -> object:
        """
        Create a new instance.
        """
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        exec_algorithm_id: str,
        signal_name: str = "routed-order",
        actor_id: object = None,
        log_events: bool = True,
        log_commands: bool = True,
        **_kwargs: object,
    ) -> None:
        """
        Initialize the helper.
        """
        self.actor_id = actor_id
        self.exec_algorithm_id = exec_algorithm_id
        self.log_events = log_events
        self.log_commands = log_commands
        self.signal_name = signal_name


class RoutedOrderDataActorExecutionAlgorithm(DataActor):
    """
    Collect routed order data actor execution algorithm tests.
    """

    received_client_order_ids: ClassVar[list[object]] = []
    received_exec_algorithm_ids: ClassVar[list[object]] = []
    signal_values: ClassVar[list[object]] = []

    def __init__(self, config: RoutedOrderExecutionAlgorithmConfig) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)
        self._signal_name = config.signal_name

    @classmethod
    def reset_observations(cls) -> None:
        """
        Reset observations.
        """
        cls.received_client_order_ids = []
        cls.received_exec_algorithm_ids = []
        cls.signal_values = []

    def on_start(self) -> None:
        """
        On start.
        """
        type(self).reset_observations()

    def on_order(self, order: object) -> None:
        """
        On order.
        """
        client_order_id = str(order.client_order_id)

        type(self).received_client_order_ids.append(client_order_id)
        type(self).received_exec_algorithm_ids.append(order.exec_algorithm_id)
        type(self).signal_values.append(client_order_id)
        self.publish_signal(self._signal_name, client_order_id)


class RoutedOrderExecutionAlgorithm(ExecutionAlgorithm):
    """
    Collect routed order execution algorithm tests.
    """

    cache_instrument_ids: ClassVar[list[object]] = []
    greeks_types: ClassVar[list[object]] = []
    portfolio_initialized: ClassVar[list[object]] = []
    received_client_order_ids: ClassVar[list[object]] = []
    received_exec_algorithm_ids: ClassVar[list[object]] = []
    running_states: ClassVar[list[object]] = []
    signal_counts_after_unsubscribe: ClassVar[list[object]] = []
    signal_values: ClassVar[list[object]] = []

    def __init__(self, config: RoutedOrderExecutionAlgorithmConfig) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)
        self._signal_name = config.signal_name

    @classmethod
    def reset_observations(cls) -> None:
        """
        Reset observations.
        """
        cls.cache_instrument_ids = []
        cls.greeks_types = []
        cls.portfolio_initialized = []
        cls.received_client_order_ids = []
        cls.received_exec_algorithm_ids = []
        cls.running_states = []
        cls.signal_counts_after_unsubscribe = []
        cls.signal_values = []

    def on_start(self) -> None:
        """
        On start.
        """
        type(self).reset_observations()
        self.subscribe_signal(self._signal_name)

    def on_stop(self) -> None:
        """
        On stop.
        """
        self.unsubscribe_signal(self._signal_name)
        self.publish_signal(self._signal_name, "after-unsubscribe")
        type(self).signal_counts_after_unsubscribe.append(len(type(self).signal_values))

    def on_order(self, order: object) -> None:
        """
        On order.
        """
        client_order_id = str(order.client_order_id)
        instrument = self.cache.instrument(order.instrument_id)

        type(self).cache_instrument_ids.append(str(instrument.id))
        type(self).greeks_types.append(type(GreeksCalculator(self.cache, self.clock)).__name__)
        type(self).portfolio_initialized.append(self.portfolio.is_initialized())
        type(self).received_client_order_ids.append(client_order_id)
        type(self).received_exec_algorithm_ids.append(order.exec_algorithm_id)
        type(self).running_states.append(self.is_running())
        self.publish_signal(self._signal_name, client_order_id)

    def on_signal(self, signal: object) -> None:
        """
        On signal.
        """
        type(self).signal_values.append(signal.value)


class DoubleSpawnExecutionAlgorithm(ExecutionAlgorithm):
    """
    Collect double spawn execution algorithm tests.
    """

    cached_primary_quantities: ClassVar[list[object]] = []
    spawned_exec_algorithm_ids: ClassVar[list[object]] = []

    def __init__(self, config: RoutedOrderExecutionAlgorithmConfig) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)

    @classmethod
    def reset_observations(cls) -> None:
        """
        Reset observations.
        """
        cls.cached_primary_quantities = []
        cls.spawned_exec_algorithm_ids = []

    def on_start(self) -> None:
        """
        On start.
        """
        type(self).reset_observations()

    def on_order(self, order: object) -> None:
        """
        On order.
        """
        first = self.spawn_market(order, Quantity.from_str("0.03000"))
        second = self.spawn_market(order, Quantity.from_str("0.02000"))
        cached_primary = self.cache.order(order.client_order_id)

        type(self).cached_primary_quantities.append(cached_primary.quantity.as_decimal())
        type(self).spawned_exec_algorithm_ids.extend(
            [first.exec_algorithm_id, second.exec_algorithm_id],
        )


class OversizedSpawnExecutionAlgorithm(ExecutionAlgorithm):
    """
    Collect oversized spawn execution algorithm tests.
    """

    error_messages: ClassVar[list[object]] = []

    def __init__(self, config: RoutedOrderExecutionAlgorithmConfig) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)

    @classmethod
    def reset_observations(cls) -> None:
        """
        Reset observations.
        """
        cls.error_messages = []

    def on_start(self) -> None:
        """
        On start.
        """
        type(self).reset_observations()

    def on_order(self, order: object) -> None:
        """
        On order.
        """
        try:
            self.spawn_market(order, Quantity.from_str("0.11000"))
        except ValueError as e:
            type(self).error_messages.append(str(e))
        else:
            type(self).error_messages.append("no error")


class MarketDataAuditActorConfig(DataActorConfig):
    """
    Collect market data audit actor config tests.
    """

    _CUSTOM_FIELDS = ("instrument_id",)

    def __new__(cls, *args: object, **kwargs: object) -> object:
        """
        Create a new instance.
        """
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        actor_id: object = None,
        log_events: bool = True,
        log_commands: bool = True,
        **_kwargs: object,
    ) -> None:
        """
        Initialize the helper.
        """
        self.actor_id = actor_id
        self.log_events = log_events
        self.log_commands = log_commands
        self.instrument_id = instrument_id


class MarketDataAuditActor(DataActor):
    """
    Collect market data audit actor tests.
    """

    quote_count = 0
    book_count = 0
    depth_count = 0
    last_bid = None
    last_book_bid = None
    last_book_ask = None

    def __init__(self, config: MarketDataAuditActorConfig) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)

    @classmethod
    def reset_observations(cls) -> None:
        """
        Reset observations.
        """
        cls.quote_count = 0
        cls.book_count = 0
        cls.depth_count = 0
        cls.last_bid = None
        cls.last_book_bid = None
        cls.last_book_ask = None

    def on_start(self) -> None:
        """
        On start.
        """
        type(self).reset_observations()
        self.subscribe_quotes(self._instrument_id)
        self.subscribe_book_at_interval(
            self._instrument_id,
            BookType.L2_MBP,
            interval_ms=1,
            depth=10,
        )

    def on_quote(self, quote: QuoteTick) -> None:
        """
        On quote.
        """
        type(self).quote_count += 1
        type(self).last_bid = quote.bid_price

    def on_book_deltas(self, _deltas: OrderBookDeltas) -> None:
        """
        On book deltas.
        """
        type(self).depth_count += 1

    def on_book(self, book: OrderBook) -> None:
        """
        On book.
        """
        type(self).book_count += 1
        type(self).last_book_bid = book.best_bid_price()
        type(self).last_book_ask = book.best_ask_price()

    def on_reset(self) -> None:
        """
        On reset.
        """
        type(self).reset_observations()


class QuoteCountActorConfig(DataActorConfig):
    """
    Configure quote count actor tests.
    """

    _CUSTOM_FIELDS = ("instrument_id",)

    def __new__(cls, *args: object, **kwargs: object) -> object:
        """
        Create a new instance.
        """
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(self, instrument_id: str, **_kwargs: object) -> None:
        """
        Initialize the config.
        """
        super().__init__()
        self.instrument_id = instrument_id


class QuoteCountActor(DataActor):
    """
    Count quotes received through actor registration tests.
    """

    quote_count: ClassVar[int] = 0
    last_bid: ClassVar[object] = None

    def __init__(self, config: QuoteCountActorConfig) -> None:
        """
        Initialize the actor.
        """
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)

    @classmethod
    def reset_observations(cls) -> None:
        """
        Reset observations.
        """
        cls.quote_count = 0
        cls.last_bid = None

    def on_start(self) -> None:
        """
        Subscribe to quotes.
        """
        type(self).reset_observations()
        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, quote: QuoteTick) -> None:
        """
        Record a quote.
        """
        type(self).quote_count += 1
        type(self).last_bid = quote.bid_price

    def on_reset(self) -> None:
        """
        Reset observations.
        """
        type(self).reset_observations()


class StreamingWhipsawConfig(StrategyConfig):
    """
    Collect streaming whipsaw config tests.
    """

    _CUSTOM_FIELDS = ("instrument_id", "trade_size")

    def __new__(cls, *args: object, **kwargs: object) -> object:
        """
        Create a new instance.
        """
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(self, instrument_id: str, trade_size: str, **_kwargs: object) -> None:
        """
        Initialize the helper.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = trade_size


class StreamingWhipsaw(Strategy):
    """
    Collect streaming whipsaw tests.
    """

    def __init__(self, config: StreamingWhipsawConfig) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._qty = Quantity.from_str(config.trade_size)
        self._quote_count = 0
        self._order_count = 0

    def on_start(self) -> None:
        """
        On start.
        """
        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, _quote: QuoteTick) -> None:
        """
        On quote.
        """
        self._quote_count += 1
        if self._quote_count in (1, 7):
            self._submit_market(OrderSide.BUY)
        elif self._quote_count in (5, 10):
            self._submit_market(OrderSide.SELL)

    def on_reset(self) -> None:
        """
        On reset.
        """
        self._quote_count = 0
        self._order_count = 0

    def on_stop(self) -> None:
        """
        On stop.
        """
        self.close_all_positions(self._instrument_id)

    def _submit_market(self, side: OrderSide) -> None:
        self._order_count += 1
        self.submit_order(
            MarketOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=self._instrument_id,
                client_order_id=ClientOrderId(f"{self.strategy_id}-{self._order_count}"),
                order_side=side,
                quantity=self._qty,
                init_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                time_in_force=TimeInForce.GTC,
                reduce_only=False,
                quote_quantity=False,
                contingency_type=None,
            ),
        )
