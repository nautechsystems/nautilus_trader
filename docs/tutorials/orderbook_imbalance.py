from __future__ import annotations

from decimal import Decimal

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model import (
    BookType,
    InstrumentId,
    OrderBookDeltas,
    OrderSide,
    Quantity,
    TimeInForce,
)
from nautilus_trader.trading import Strategy


class OrderBookImbalanceConfig(StrategyConfig):
    _CUSTOM_FIELDS = (
        "instrument_id",
        "max_trade_size",
        "trigger_min_size",
        "trigger_imbalance_ratio",
        "min_seconds_between_triggers",
        "book_type",
    )

    def __new__(cls, *args, **kwargs):
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        max_trade_size: str,
        trigger_min_size: float = 100.0,
        trigger_imbalance_ratio: float = 0.20,
        min_seconds_between_triggers: float = 1.0,
        book_type: str = "L2_MBP",
        **kwargs,
    ) -> None:
        super().__init__()
        self.instrument_id = instrument_id
        self.max_trade_size = max_trade_size
        self.trigger_min_size = trigger_min_size
        self.trigger_imbalance_ratio = trigger_imbalance_ratio
        self.min_seconds_between_triggers = min_seconds_between_triggers
        self.book_type = book_type


class OrderBookImbalance(Strategy):
    def __init__(self, config: OrderBookImbalanceConfig) -> None:
        if not 0 < config.trigger_imbalance_ratio < 1:
            raise ValueError("trigger_imbalance_ratio must be between 0 and 1")
        if config.min_seconds_between_triggers < 0:
            raise ValueError("min_seconds_between_triggers must be non-negative")

        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._book_type = BookType.from_str(config.book_type)
        self._max_trade_size = Decimal(config.max_trade_size)
        self._trigger_min_size = Decimal(str(config.trigger_min_size))
        self._trigger_imbalance_ratio = Decimal(str(config.trigger_imbalance_ratio))
        self._trigger_interval_ns = int(config.min_seconds_between_triggers * 1_000_000_000)
        self._instrument = None
        self._last_trigger_ns: int | None = None

    def on_start(self) -> None:
        self._instrument = self.cache.instrument(self._instrument_id)
        if self._instrument is None:
            self.log.error(f"Could not find instrument for {self._instrument_id}")
            self.stop()
            return

        self.subscribe_book_deltas(self._instrument_id, self._book_type, managed=True)

    def on_book_deltas(self, deltas: OrderBookDeltas) -> None:
        book = self.cache.order_book(self._instrument_id)
        if book is None or not book.spread():
            return

        bid_size = book.best_bid_size()
        ask_size = book.best_ask_size()
        if bid_size is None or bid_size <= 0 or ask_size is None or ask_size <= 0:
            return

        bid = bid_size.as_decimal()
        ask = ask_size.as_decimal()
        smaller = min(bid, ask)
        larger = max(bid, ask)
        if larger <= self._trigger_min_size or smaller / larger >= self._trigger_imbalance_ratio:
            return

        now = self.clock.timestamp_ns()
        if (
            self._last_trigger_ns is not None
            and now - self._last_trigger_ns < self._trigger_interval_ns
        ):
            return
        if self.cache.orders_inflight(strategy_id=self.strategy_id):
            return

        if bid > ask:
            side = OrderSide.BUY
            price = book.best_ask_price()
            level_size = ask
        else:
            side = OrderSide.SELL
            price = book.best_bid_price()
            level_size = bid

        if price is None or self._instrument is None:
            return

        self._last_trigger_ns = now
        order = self.order_factory.limit(
            instrument_id=self._instrument_id,
            order_side=side,
            quantity=Quantity.from_decimal_dp(
                min(level_size, self._max_trade_size),
                self._instrument.size_precision,
            ),
            price=price,
            time_in_force=TimeInForce.FOK,
            post_only=False,
        )
        self.submit_order(order)

    def on_stop(self) -> None:
        self.cancel_all_orders(self._instrument_id)
        self.close_all_positions(self._instrument_id)

    def on_reset(self) -> None:
        self._instrument = None
        self._last_trigger_ns = None
