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

from __future__ import annotations

from decimal import Decimal

from nautilus_trader.common import DataActor
from nautilus_trader.common import DataActorConfig
from nautilus_trader.core import UUID4
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import LimitOrder
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeTick
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


class SignalHarvestConfig(StrategyConfig):
    _CUSTOM_FIELDS = ("instrument_id", "bar_type", "trade_size")

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(self, instrument_id: str, bar_type: str, trade_size: str, **kwargs):
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size


class SignalHarvest(Strategy):
    def __init__(self, config: SignalHarvestConfig):
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

    def on_start(self):
        self._instrument = self.cache.instrument(self._instrument_id)
        self.subscribe_bars(self._bar_type)
        self.subscribe_trades(self._instrument_id)
        self.subscribe_mark_prices(self._instrument_id)
        self.subscribe_index_prices(self._instrument_id)
        self.subscribe_funding_rates(self._instrument_id)
        self.subscribe_instrument_status(self._instrument_id)
        self.subscribe_instrument_close(self._instrument_id)

    def on_bar(self, bar: Bar):
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

    def on_trade(self, trade: TradeTick):
        self._trade_count += 1

    def on_mark_price(self, mark_price: MarkPriceUpdate):
        self._mark_count += 1

    def on_index_price(self, index_price: IndexPriceUpdate):
        self._index_count += 1

    def on_funding_rate(self, funding_rate: FundingRateUpdate):
        self._funding_count += 1

    def on_instrument_status(self, status: InstrumentStatus):
        self._status_count += 1

    def on_instrument_close(self, close: InstrumentClose):
        self._close_count += 1

    def on_reset(self):
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

    def on_stop(self):
        self.cancel_all_orders(self._instrument_id)
        self.close_all_positions(self._instrument_id)

    def _update_bar_state(self, bar: Bar):
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

    def _submit_market(self, side: OrderSide):
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
                contingency_type=ContingencyType.NO_CONTINGENCY,
            ),
        )

    def _submit_limit(self, side: OrderSide, price: Price):
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
    _CUSTOM_FIELDS = ("instrument_id", "trade_size")

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(self, instrument_id: str, trade_size: str, **kwargs):
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = trade_size


class BookChurn(Strategy):
    def __init__(self, config: BookChurnConfig):
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

    def on_start(self):
        self.subscribe_book_deltas(self._instrument_id, BookType.L2_MBP, depth=10)
        self.subscribe_book_at_interval(self._instrument_id, BookType.L2_MBP, interval_ms=1_000)

    def on_book_deltas(self, deltas: OrderBookDeltas):
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

    def on_book(self, book):
        self._book_count += 1

    def on_reset(self):
        self._book_count = 0
        self._delta_count = 0
        self._order_count = 0
        self._entry_sent = False
        self._resting_sent = False
        self._cancel_sent = False
        self._exit_sent = False

    def on_stop(self):
        self.cancel_all_orders(self._instrument_id)
        self.close_all_positions(self._instrument_id)

    def _submit_market(self, side: OrderSide):
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
                contingency_type=ContingencyType.NO_CONTINGENCY,
            ),
        )

    def _submit_limit(self, side: OrderSide, price: Price):
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
    _CUSTOM_FIELDS = ("instrument_id", "trade_size", "exec_algorithm_id")

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(self, instrument_id: str, trade_size: str, exec_algorithm_id: str, **kwargs):
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = trade_size
        self.exec_algorithm_id = exec_algorithm_id


class RoutedOrderProbe(Strategy):
    def __init__(self, config: RoutedOrderProbeConfig):
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._qty = Quantity.from_str(config.trade_size)
        self._exec_algorithm_id = ExecAlgorithmId(config.exec_algorithm_id)
        self._sent = False

    def on_start(self):
        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, quote: QuoteTick):
        if not self._sent:
            self._sent = True
            self.submit_order(
                MarketOrder(
                    trader_id=self.trader_id,
                    strategy_id=self.strategy_id,
                    instrument_id=self._instrument_id,
                    client_order_id=ClientOrderId(f"{self.strategy_id}-1"),
                    order_side=OrderSide.BUY,
                    quantity=self._qty,
                    init_id=UUID4(),
                    ts_init=self.clock.timestamp_ns(),
                    time_in_force=TimeInForce.GTC,
                    reduce_only=False,
                    quote_quantity=False,
                    contingency_type=ContingencyType.NO_CONTINGENCY,
                    exec_algorithm_id=self._exec_algorithm_id,
                ),
            )

    def on_reset(self):
        self._sent = False


class RoutedOrderExecAlgorithmConfig(DataActorConfig):
    _CUSTOM_FIELDS = ("exec_algorithm_id", "signal_name")

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        exec_algorithm_id: str,
        signal_name: str = "routed-order",
        actor_id=None,
        log_events: bool = True,
        log_commands: bool = True,
        **kwargs,
    ):
        self.actor_id = actor_id
        self.exec_algorithm_id = exec_algorithm_id
        self.log_events = log_events
        self.log_commands = log_commands
        self.signal_name = signal_name


class RoutedOrderExecAlgorithm(DataActor):
    received_client_order_ids = []
    received_exec_algorithm_ids = []
    signal_values = []

    def __init__(self, config: RoutedOrderExecAlgorithmConfig):
        super().__init__(config)
        self._signal_name = config.signal_name

    @classmethod
    def reset_observations(cls):
        cls.received_client_order_ids = []
        cls.received_exec_algorithm_ids = []
        cls.signal_values = []

    def on_start(self):
        type(self).reset_observations()

    def on_order(self, order):
        client_order_id = str(order.client_order_id)

        type(self).received_client_order_ids.append(client_order_id)
        type(self).received_exec_algorithm_ids.append(order.exec_algorithm_id)
        type(self).signal_values.append(client_order_id)
        self.publish_signal(self._signal_name, client_order_id)


class MarketDataAuditActorConfig(DataActorConfig):
    _CUSTOM_FIELDS = ("instrument_id",)

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        actor_id=None,
        log_events: bool = True,
        log_commands: bool = True,
        **kwargs,
    ):
        self.actor_id = actor_id
        self.log_events = log_events
        self.log_commands = log_commands
        self.instrument_id = instrument_id


class MarketDataAuditActor(DataActor):
    quote_count = 0
    book_count = 0
    depth_count = 0
    last_bid = None
    last_book_bid = None
    last_book_ask = None

    def __init__(self, config: MarketDataAuditActorConfig):
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)

    @classmethod
    def reset_observations(cls):
        cls.quote_count = 0
        cls.book_count = 0
        cls.depth_count = 0
        cls.last_bid = None
        cls.last_book_bid = None
        cls.last_book_ask = None

    def on_start(self):
        type(self).reset_observations()
        self.subscribe_quotes(self._instrument_id)
        self.subscribe_book_at_interval(
            self._instrument_id,
            BookType.L2_MBP,
            interval_ms=1,
            depth=10,
        )

    def on_quote(self, quote: QuoteTick):
        type(self).quote_count += 1
        type(self).last_bid = quote.bid_price

    def on_book_deltas(self, deltas: OrderBookDeltas):
        type(self).depth_count += 1

    def on_book(self, book):
        type(self).book_count += 1
        type(self).last_book_bid = book.best_bid_price()
        type(self).last_book_ask = book.best_ask_price()

    def on_reset(self):
        type(self).reset_observations()


class StreamingWhipsawConfig(StrategyConfig):
    _CUSTOM_FIELDS = ("instrument_id", "trade_size")

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(self, instrument_id: str, trade_size: str, **kwargs):
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = trade_size


class StreamingWhipsaw(Strategy):
    def __init__(self, config: StreamingWhipsawConfig):
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._qty = Quantity.from_str(config.trade_size)
        self._quote_count = 0
        self._order_count = 0

    def on_start(self):
        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, quote: QuoteTick):
        self._quote_count += 1
        if self._quote_count in (1, 7):
            self._submit_market(OrderSide.BUY)
        elif self._quote_count in (5, 10):
            self._submit_market(OrderSide.SELL)

    def on_reset(self):
        self._quote_count = 0
        self._order_count = 0

    def on_stop(self):
        self.close_all_positions(self._instrument_id)

    def _submit_market(self, side: OrderSide):
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
                contingency_type=ContingencyType.NO_CONTINGENCY,
            ),
        )
