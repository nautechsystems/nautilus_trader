#!/usr/bin/env python3
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
Run deterministic, pairwise v1 and v2 BacktestEngine benchmarks.
"""

# ruff: noqa: ANN401, PLC0415

from __future__ import annotations

import argparse
import dataclasses
import datetime as dt
import hashlib
import importlib.metadata
import inspect
import json
import os
import platform
import shutil
import stat
import statistics
import subprocess
import sys
import time
import zipfile
from collections import Counter
from decimal import Decimal
from pathlib import Path
from typing import Any


BASE_TS_NS = 1_735_689_600_000_000_000
EVENT_INTERVAL_NS = 1_000_000
MIN_SESSIONS = 3
PASSIVE_CANCEL_OFFSET = 2


@dataclasses.dataclass(frozen=True)
class Scenario:
    """
    Describe one bounded comparison case without binding to either runtime.
    """

    name: str
    fixture: str
    size: str
    count: int
    strategy: str = "none"
    book_type: str = "L1_MBP"
    streams: int = 1
    instruments: int = 1
    queue_position: bool = False
    liquidity_consumption: bool = False
    reject_stop_orders: bool = True


@dataclasses.dataclass(frozen=True)
class Runtime:
    """
    Describe one isolated comparison environment and its expected implementation.
    """

    python: Path
    artifact: Path
    source_root: Path
    commit: str
    version: str
    backend: str


SCENARIOS = (
    Scenario("quote_trade_replay_small", "quotes_trades", "small", 256),
    Scenario("quote_trade_replay_medium", "quotes_trades", "medium", 2_048),
    Scenario(
        "quote_trade_replay_multi",
        "quotes_trades",
        "medium",
        2_048,
        streams=4,
        instruments=2,
    ),
    Scenario("bar_last_replay", "bars_last", "medium", 2_048),
    Scenario("bar_bid_ask_strategy", "bars_bid_ask", "medium", 2_048, "bar_audit"),
    Scenario(
        "l2_deltas_queue_passive",
        "l2_deltas",
        "medium",
        1_024,
        "passive_cancel",
        book_type="L2_MBP",
        queue_position=True,
    ),
    Scenario(
        "depth10_liquidity_market",
        "depth10",
        "large",
        4_096,
        "alternating_market",
        book_type="L2_MBP",
        liquidity_consumption=True,
    ),
    Scenario("alternating_market_small", "quotes_trades", "small", 256, "alternating_market"),
    Scenario(
        "alternating_market_medium",
        "quotes_trades",
        "medium",
        1_024,
        "alternating_market",
    ),
    Scenario(
        "alternating_market_large",
        "quotes_trades",
        "large",
        4_096,
        "alternating_market",
    ),
    Scenario(
        "accumulating_market_small",
        "quotes_trades",
        "small",
        250,
        "accumulating_market",
    ),
    Scenario(
        "accumulating_market_medium",
        "quotes_trades",
        "medium",
        1_000,
        "accumulating_market",
    ),
    Scenario(
        "accumulating_market_large",
        "quotes_trades",
        "large",
        4_000,
        "accumulating_market",
    ),
    Scenario("passive_cancel", "quotes_trades", "medium", 2_048, "passive_cancel"),
    Scenario("gtd_expiry", "quotes_trades", "medium", 2_048, "gtd_expiry"),
    Scenario(
        "order_type_trigger_sweep",
        "trigger_quotes",
        "medium",
        2_048,
        "order_type_sweep",
        reject_stop_orders=False,
    ),
)

BOUNDARIES = ("run_preloaded", "load_build_run")
BOUNDARY_DESCRIPTIONS = {
    "run_preloaded": "run only, after fixture generation, engine construction, and data load",
    "load_build_run": "fixture generation, engine construction, data load, and run",
}


@dataclasses.dataclass(frozen=True)
class Bindings:
    """
    Hold imports and the small API differences between v1 and v2.
    """

    generation: str
    module: Any
    BacktestEngine: type
    BacktestEngineConfig: type
    LoggingConfig: type
    RiskEngineConfig: type
    Strategy: type

    def subscribe_quotes(self, strategy: Any, instrument_id: Any) -> None:
        """
        Subscribe through the quote API exposed by the active runtime.
        """
        if self.generation == "v1":
            strategy.subscribe_quote_ticks(instrument_id)
        else:
            strategy.subscribe_quotes(instrument_id)

    def expire_time(self, ts_ns: int) -> int | dt.datetime:
        """
        Convert a nanosecond expiry to the representation expected by the runtime.
        """
        if self.generation == "v1":
            return dt.datetime.fromtimestamp(ts_ns / 1_000_000_000, tz=dt.UTC)
        return ts_ns

    def cancel_order(self, strategy: Any, order: Any) -> None:
        """
        Cancel through the order argument accepted by the active runtime.
        """
        if self.generation == "v1":
            strategy.cancel_order(order)
        else:
            strategy.cancel_order(order.client_order_id)

    def accounts(self, engine: Any) -> list[Any]:
        """
        Return equivalent cached accounts across the two cache APIs.
        """
        if self.generation == "v1":
            return list(engine.cache.accounts())
        account = engine.cache.account_for_venue(self.module.Venue("BINANCE"))
        return [] if account is None else [account]

    def engine_config(self) -> Any:
        """
        Build equivalent engine, risk, logging, and analysis controls.
        """
        if self.generation == "v1":
            return self.BacktestEngineConfig(
                logging=self.LoggingConfig(bypass_logging=True),
                risk_engine=self.RiskEngineConfig(max_order_submit_rate="1000000/00:00:01"),
                run_analysis=False,
            )
        return self.BacktestEngineConfig(
            logging=self.LoggingConfig.from_spec("bypass_logging"),
            risk_engine=self.RiskEngineConfig(max_order_submit_rate="1000000/00:00:01"),
            bypass_logging=True,
            run_analysis=False,
        )


def load_bindings() -> Bindings:  # noqa: PLR0915
    """
    Import the installed NautilusTrader runtime and normalize its public API.
    """
    version = importlib.metadata.version("nautilus-trader")
    if version.startswith("1."):
        from nautilus_trader.backtest.config import BacktestEngineConfig
        from nautilus_trader.backtest.engine import BacktestEngine
        from nautilus_trader.common.config import LoggingConfig
        from nautilus_trader.model import Symbol
        from nautilus_trader.model.data import Bar
        from nautilus_trader.model.data import BarSpecification
        from nautilus_trader.model.data import BarType
        from nautilus_trader.model.data import BookOrder
        from nautilus_trader.model.data import OrderBookDelta
        from nautilus_trader.model.data import OrderBookDeltas
        from nautilus_trader.model.data import OrderBookDepth10
        from nautilus_trader.model.data import QuoteTick
        from nautilus_trader.model.data import TradeTick
        from nautilus_trader.model.enums import AccountType
        from nautilus_trader.model.enums import AggregationSource
        from nautilus_trader.model.enums import AggressorSide
        from nautilus_trader.model.enums import BarAggregation
        from nautilus_trader.model.enums import BookAction
        from nautilus_trader.model.enums import BookType
        from nautilus_trader.model.enums import OmsType
        from nautilus_trader.model.enums import OrderSide
        from nautilus_trader.model.enums import PriceType
        from nautilus_trader.model.enums import TimeInForce
        from nautilus_trader.model.enums import TrailingOffsetType
        from nautilus_trader.model.enums import TriggerType
        from nautilus_trader.model.identifiers import ClientId
        from nautilus_trader.model.identifiers import InstrumentId
        from nautilus_trader.model.identifiers import TradeId
        from nautilus_trader.model.identifiers import Venue
        from nautilus_trader.model.instruments import CryptoPerpetual
        from nautilus_trader.model.objects import Currency
        from nautilus_trader.model.objects import Money
        from nautilus_trader.model.objects import Price
        from nautilus_trader.model.objects import Quantity
        from nautilus_trader.risk.config import RiskEngineConfig
        from nautilus_trader.trading.strategy import Strategy

        generation = "v1"
    else:
        from nautilus_trader.backtest import BacktestEngine
        from nautilus_trader.backtest import BacktestEngineConfig
        from nautilus_trader.common import LoggerConfig as LoggingConfig
        from nautilus_trader.model import AccountType
        from nautilus_trader.model import AggregationSource
        from nautilus_trader.model import AggressorSide
        from nautilus_trader.model import Bar
        from nautilus_trader.model import BarAggregation
        from nautilus_trader.model import BarSpecification
        from nautilus_trader.model import BarType
        from nautilus_trader.model import BookAction
        from nautilus_trader.model import BookOrder
        from nautilus_trader.model import BookType
        from nautilus_trader.model import ClientId
        from nautilus_trader.model import CryptoPerpetual
        from nautilus_trader.model import Currency
        from nautilus_trader.model import InstrumentId
        from nautilus_trader.model import Money
        from nautilus_trader.model import OmsType
        from nautilus_trader.model import OrderBookDelta
        from nautilus_trader.model import OrderBookDeltas
        from nautilus_trader.model import OrderBookDepth10
        from nautilus_trader.model import OrderSide
        from nautilus_trader.model import Price
        from nautilus_trader.model import PriceType
        from nautilus_trader.model import Quantity
        from nautilus_trader.model import QuoteTick
        from nautilus_trader.model import Symbol
        from nautilus_trader.model import TimeInForce
        from nautilus_trader.model import TradeId
        from nautilus_trader.model import TradeTick
        from nautilus_trader.model import TrailingOffsetType
        from nautilus_trader.model import TriggerType
        from nautilus_trader.model import Venue
        from nautilus_trader.risk import RiskEngineConfig
        from nautilus_trader.trading import Strategy

        generation = "v2"

    module = type(
        "RuntimeModule",
        (),
        {
            "AccountType": AccountType,
            "AggregationSource": AggregationSource,
            "AggressorSide": AggressorSide,
            "Bar": Bar,
            "BarAggregation": BarAggregation,
            "BarSpecification": BarSpecification,
            "BarType": BarType,
            "BookAction": BookAction,
            "BookOrder": BookOrder,
            "BookType": BookType,
            "ClientId": ClientId,
            "CryptoPerpetual": CryptoPerpetual,
            "Currency": Currency,
            "InstrumentId": InstrumentId,
            "Money": Money,
            "OmsType": OmsType,
            "OrderBookDelta": OrderBookDelta,
            "OrderBookDeltas": OrderBookDeltas,
            "OrderBookDepth10": OrderBookDepth10,
            "OrderSide": OrderSide,
            "Price": Price,
            "PriceType": PriceType,
            "Quantity": Quantity,
            "QuoteTick": QuoteTick,
            "Symbol": Symbol,
            "TimeInForce": TimeInForce,
            "TradeId": TradeId,
            "TradeTick": TradeTick,
            "TrailingOffsetType": TrailingOffsetType,
            "TriggerType": TriggerType,
            "Venue": Venue,
        },
    )
    return Bindings(
        generation,
        module,
        BacktestEngine,
        BacktestEngineConfig,
        LoggingConfig,
        RiskEngineConfig,
        Strategy,
    )


def make_instrument(bindings: Bindings, symbol: str) -> Any:
    """
    Construct the same instrument values in both implementations.
    """
    model = bindings.module
    base = symbol[:3]
    return model.CryptoPerpetual(
        instrument_id=model.InstrumentId.from_str(f"{symbol}-PERP.BINANCE"),
        raw_symbol=model.Symbol(symbol),
        base_currency=model.Currency.from_str(base),
        quote_currency=model.Currency.from_str("USDT"),
        settlement_currency=model.Currency.from_str("USDT"),
        is_inverse=False,
        price_precision=2,
        size_precision=3,
        price_increment=model.Price.from_str("0.01"),
        size_increment=model.Quantity.from_str("0.001"),
        ts_event=0,
        ts_init=0,
        max_quantity=model.Quantity.from_str("100000.000"),
        min_quantity=model.Quantity.from_str("0.001"),
        max_price=model.Price.from_str("1000000.00"),
        min_price=model.Price.from_str("0.01"),
        margin_init=Decimal("0.01"),
        margin_maint=Decimal("0.005"),
        maker_fee=Decimal(0),
        taker_fee=Decimal(0),
    )


def price(model: Any, value: Decimal) -> Any:
    """
    Create a two-decimal price through the active model API.
    """
    return model.Price.from_str(f"{value:.2f}")


def quantity(model: Any, value: Decimal | int | str) -> Any:
    """
    Create a three-decimal quantity through the active model API.
    """
    return model.Quantity.from_str(f"{Decimal(value):.3f}")


def aggressor_side(model: Any, *, buy: bool) -> Any:
    """
    Select the equivalent aggressor enum variant across both runtimes.
    """
    current = "BUY" if buy else "SELL"
    legacy = "BUYER" if buy else "SELLER"
    return getattr(model.AggressorSide, current, None) or getattr(model.AggressorSide, legacy)


def make_quotes_trades(bindings: Bindings, instruments: list[Any], count: int) -> list[list[Any]]:
    """
    Generate interleaved quote and trade streams for one or more instruments.
    """
    model = bindings.module
    streams: list[list[Any]] = [[] for _ in range(len(instruments) * 2)]
    for index in range(count):
        instrument_index = index % len(instruments)
        instrument = instruments[instrument_index]
        mid = Decimal("50000.00") + Decimal((index % 40) - 20)
        ts = BASE_TS_NS + index * EVENT_INTERVAL_NS
        streams[instrument_index * 2].append(
            model.QuoteTick(
                instrument_id=instrument.id,
                bid_price=price(model, mid - Decimal("0.05")),
                ask_price=price(model, mid + Decimal("0.05")),
                bid_size=quantity(model, "10"),
                ask_size=quantity(model, "10"),
                ts_event=ts,
                ts_init=ts,
            ),
        )
        streams[(instrument_index * 2) + 1].append(
            model.TradeTick(
                instrument_id=instrument.id,
                price=price(model, mid),
                size=quantity(model, "1"),
                aggressor_side=aggressor_side(model, buy=index % 2 == 0),
                trade_id=model.TradeId(f"T-{index}"),
                ts_event=ts + 1,
                ts_init=ts + 1,
            ),
        )
    return streams


def make_bars(bindings: Bindings, instrument: Any, count: int, price_type: str) -> list[list[Any]]:
    """
    Generate one LAST stream or paired BID and ASK bar streams.
    """
    model = bindings.module
    price_types = [price_type] if price_type == "LAST" else ["BID", "ASK"]
    streams: list[list[Any]] = []
    for type_index, type_name in enumerate(price_types):
        bar_type = model.BarType(
            instrument.id,
            model.BarSpecification(
                1,
                model.BarAggregation.MINUTE,
                getattr(model.PriceType, type_name),
            ),
            model.AggregationSource.EXTERNAL,
        )
        bars = []
        for index in range(count):
            close = Decimal("50000.00") + Decimal((index % 40) - 20 + type_index)
            ts = BASE_TS_NS + index * 60 * EVENT_INTERVAL_NS + type_index
            bars.append(
                model.Bar(
                    bar_type=bar_type,
                    open=price(model, close - Decimal("1.00")),
                    high=price(model, close + Decimal("2.00")),
                    low=price(model, close - Decimal("2.00")),
                    close=price(model, close),
                    volume=quantity(model, "10"),
                    ts_event=ts,
                    ts_init=ts,
                ),
            )
        streams.append(bars)
    return streams


def make_l2_deltas(bindings: Bindings, instrument: Any, count: int) -> list[list[Any]]:
    """
    Generate deterministic two-sided L2 delta batches.
    """
    model = bindings.module
    batches = []
    for index in range(count):
        mid = Decimal("50000.00") + Decimal((index % 40) - 20)
        ts = BASE_TS_NS + index * EVENT_INTERVAL_NS
        action = model.BookAction.ADD if index == 0 else model.BookAction.UPDATE
        batches.append(
            model.OrderBookDeltas(
                instrument_id=instrument.id,
                deltas=[
                    model.OrderBookDelta(
                        instrument.id,
                        action,
                        model.BookOrder(
                            model.OrderSide.BUY,
                            price(model, mid - Decimal("0.05")),
                            quantity(model, "10"),
                            1,
                        ),
                        0,
                        index * 2 + 1,
                        ts,
                        ts,
                    ),
                    model.OrderBookDelta(
                        instrument.id,
                        action,
                        model.BookOrder(
                            model.OrderSide.SELL,
                            price(model, mid + Decimal("0.05")),
                            quantity(model, "10"),
                            2,
                        ),
                        0,
                        index * 2 + 2,
                        ts,
                        ts,
                    ),
                ],
            ),
        )
    return [batches]


def make_depth10(bindings: Bindings, instrument: Any, count: int) -> list[list[Any]]:
    """
    Generate deterministic ten-level order book snapshots.
    """
    model = bindings.module
    depths = []
    for index in range(count):
        mid = Decimal("50000.00") + Decimal((index % 40) - 20)
        bids = []
        asks = []
        for level in range(10):
            offset = Decimal(level) * Decimal("0.01")
            bids.append(
                model.BookOrder(
                    model.OrderSide.BUY,
                    price(model, mid - Decimal("0.05") - offset),
                    quantity(model, level + 1),
                    level + 1,
                ),
            )
            asks.append(
                model.BookOrder(
                    model.OrderSide.SELL,
                    price(model, mid + Decimal("0.05") + offset),
                    quantity(model, level + 1),
                    level + 11,
                ),
            )
        ts = BASE_TS_NS + index * EVENT_INTERVAL_NS
        depths.append(
            model.OrderBookDepth10(
                instrument_id=instrument.id,
                bids=bids,
                asks=asks,
                bid_counts=[1] * 10,
                ask_counts=[1] * 10,
                flags=0,
                sequence=index + 1,
                ts_event=ts,
                ts_init=ts,
            ),
        )
    return [depths]


def make_trigger_quotes(bindings: Bindings, instrument: Any, count: int) -> list[list[Any]]:
    """
    Generate a flat quote stream that keeps trigger outcomes equivalent.
    """
    model = bindings.module
    quotes = []
    for index in range(count):
        mid = Decimal("50000.00")
        ts = BASE_TS_NS + index * EVENT_INTERVAL_NS
        quotes.append(
            model.QuoteTick(
                instrument_id=instrument.id,
                bid_price=price(model, mid - Decimal("0.05")),
                ask_price=price(model, mid + Decimal("0.05")),
                bid_size=quantity(model, "20"),
                ask_size=quantity(model, "20"),
                ts_event=ts,
                ts_init=ts,
            ),
        )
    return [quotes]


def make_fixture(bindings: Bindings, scenario: Scenario, instruments: list[Any]) -> list[list[Any]]:
    """
    Build the streams selected by a version-neutral scenario specification.
    """
    if scenario.fixture == "quotes_trades":
        streams = make_quotes_trades(bindings, instruments, scenario.count)
    elif scenario.fixture == "bars_last":
        streams = make_bars(bindings, instruments[0], scenario.count, "LAST")
    elif scenario.fixture == "bars_bid_ask":
        streams = make_bars(bindings, instruments[0], scenario.count, "BID_ASK")
    elif scenario.fixture == "l2_deltas":
        streams = make_l2_deltas(bindings, instruments[0], scenario.count)
        streams.extend(make_quotes_trades(bindings, instruments[:1], scenario.count)[::2])
    elif scenario.fixture == "depth10":
        streams = make_depth10(bindings, instruments[0], scenario.count)
        streams.extend(make_quotes_trades(bindings, instruments[:1], scenario.count)[::2])
    elif scenario.fixture == "trigger_quotes":
        streams = make_trigger_quotes(bindings, instruments[0], scenario.count)
    else:
        raise ValueError(f"Unknown fixture {scenario.fixture!r}")

    if scenario.streams == 1:
        return [[item for stream in streams for item in stream]]
    return [stream for stream in streams if stream]


def strategy_type(bindings: Bindings) -> type:  # noqa: C901
    """
    Create a strategy subclass bound to the installed runtime.
    """
    model = bindings.module

    class ScenarioStrategy(bindings.Strategy):
        def __init__(self) -> None:
            super().__init__()
            self.bindings = bindings
            self.instrument_id = None
            self.kind = "none"
            self.count = 0
            self.open_order = None
            self.seen = 0
            self.bar_types = []

        def configure(
            self,
            instrument_id: Any,
            kind: str,
            count: int,
            bar_types: list[Any],
        ) -> None:
            self.instrument_id = instrument_id
            self.kind = kind
            self.count = count
            self.bar_types = bar_types

        def on_start(self) -> None:
            if self.kind == "bar_audit":
                for bar_type in self.bar_types:
                    self.subscribe_bars(bar_type)
                return
            self.bindings.subscribe_quotes(self, self.instrument_id)

        def on_quote(self, quote: Any) -> None:
            self._handle_quote(quote)

        def on_quote_tick(self, quote: Any) -> None:
            self._handle_quote(quote)

        def on_bar(self, _bar: Any) -> None:
            self.seen += 1

        def on_stop(self) -> None:
            if self.open_order is not None and not value_or_call(self.open_order, "is_closed"):
                self.bindings.cancel_order(self, self.open_order)

        def _handle_quote(self, quote: Any) -> None:
            if quote.instrument_id != self.instrument_id:
                return
            self.seen += 1
            if self.kind == "alternating_market" and self.seen % 10 == 0:
                side = model.OrderSide.BUY if (self.seen // 10) % 2 else model.OrderSide.SELL
                self._submit_market(side)
            elif self.kind == "accumulating_market":
                self._submit_market(model.OrderSide.BUY)
            elif self.kind == "passive_cancel" and self.seen % 20 == 1:
                self._submit_passive()
            elif self.kind == "passive_cancel" and self.open_order is not None:
                if self.seen % 20 == PASSIVE_CANCEL_OFFSET and not value_or_call(
                    self.open_order,
                    "is_closed",
                ):
                    self.bindings.cancel_order(self, self.open_order)
                    self.open_order = None
            elif self.kind == "gtd_expiry" and self.seen % 20 == 1:
                self._submit_gtd(quote.ts_event + 5 * EVENT_INTERVAL_NS)
            elif self.kind == "order_type_sweep" and self.seen == 1:
                self._submit_order_types()

        def _submit_market(self, side: Any) -> None:
            self.submit_order(
                self.order_factory.market(
                    instrument_id=self.instrument_id,
                    order_side=side,
                    quantity=quantity(model, "0.010"),
                ),
            )

        def _submit_passive(self) -> None:
            self.open_order = self.order_factory.limit(
                instrument_id=self.instrument_id,
                order_side=model.OrderSide.BUY,
                quantity=quantity(model, "0.010"),
                price=price(model, Decimal("40000.00")),
            )
            self.submit_order(self.open_order)

        def _submit_gtd(self, expire_time_ns: int) -> None:
            self.submit_order(
                self.order_factory.limit(
                    instrument_id=self.instrument_id,
                    order_side=model.OrderSide.BUY,
                    quantity=quantity(model, "0.010"),
                    price=price(model, Decimal("40000.00")),
                    time_in_force=model.TimeInForce.GTD,
                    expire_time=self.bindings.expire_time(expire_time_ns),
                ),
            )

        def _submit_order_types(self) -> None:
            common = {
                "instrument_id": self.instrument_id,
                "quantity": quantity(model, "0.010"),
            }
            orders = [
                self.order_factory.market(order_side=model.OrderSide.BUY, **common),
                self.order_factory.limit(
                    order_side=model.OrderSide.BUY,
                    price=price(model, Decimal("49950.00")),
                    **common,
                ),
                self.order_factory.stop_market(
                    order_side=model.OrderSide.BUY,
                    trigger_price=price(model, Decimal("50020.00")),
                    trigger_type=model.TriggerType.BID_ASK,
                    **common,
                ),
                self.order_factory.stop_limit(
                    order_side=model.OrderSide.BUY,
                    price=price(model, Decimal("50021.00")),
                    trigger_price=price(model, Decimal("50020.00")),
                    trigger_type=model.TriggerType.BID_ASK,
                    **common,
                ),
                self.order_factory.market_if_touched(
                    order_side=model.OrderSide.SELL,
                    trigger_price=price(model, Decimal("50010.00")),
                    trigger_type=model.TriggerType.BID_ASK,
                    **common,
                ),
                self.order_factory.limit_if_touched(
                    order_side=model.OrderSide.SELL,
                    price=price(model, Decimal("50009.00")),
                    trigger_price=price(model, Decimal("50010.00")),
                    trigger_type=model.TriggerType.BID_ASK,
                    **common,
                ),
                self.order_factory.trailing_stop_market(
                    order_side=model.OrderSide.SELL,
                    trailing_offset=Decimal(10),
                    trailing_offset_type=model.TrailingOffsetType.PRICE,
                    **common,
                ),
                self.order_factory.trailing_stop_limit(
                    order_side=model.OrderSide.SELL,
                    price=None,
                    limit_offset=Decimal(1),
                    trailing_offset=Decimal(10),
                    trailing_offset_type=model.TrailingOffsetType.PRICE,
                    **common,
                ),
                self.order_factory.market_to_limit(order_side=model.OrderSide.BUY, **common),
            ]
            for order in orders:
                self.submit_order(order)

    return ScenarioStrategy


def build_engine(bindings: Bindings, scenario: Scenario, fixture: list[list[Any]]) -> Any:
    """
    Construct and load an engine from one version-neutral scenario.
    """
    model = bindings.module
    engine = bindings.BacktestEngine(bindings.engine_config())
    engine.add_venue(
        venue=model.Venue("BINANCE"),
        oms_type=model.OmsType.NETTING,
        account_type=model.AccountType.MARGIN,
        starting_balances=[model.Money.from_str("1_000_000 USDT")],
        base_currency=model.Currency.from_str("USDT"),
        book_type=getattr(model.BookType, scenario.book_type),
        reject_stop_orders=scenario.reject_stop_orders,
        liquidity_consumption=scenario.liquidity_consumption,
        queue_position=scenario.queue_position,
    )
    instruments = [make_instrument(bindings, symbol) for symbol in ("BTCUSDT", "ETHUSDT")]
    for instrument in instruments[: scenario.instruments]:
        engine.add_instrument(instrument)
    if scenario.strategy != "none":
        strategy = strategy_type(bindings)()
        bar_types = []
        if scenario.strategy == "bar_audit":
            bar_types = sorted(
                {
                    str(item.bar_type): item.bar_type for stream in fixture for item in stream
                }.values(),
                key=str,
            )
        strategy.configure(instruments[0].id, scenario.strategy, scenario.count, bar_types)
        engine.add_strategy(strategy)
    for stream_index, stream in enumerate(fixture):
        client_id = model.ClientId(f"BENCH-{stream_index + 1}") if len(fixture) > 1 else None
        engine.add_data(stream, client_id=client_id, sort=True)
    return engine


def value_or_call(value: Any, name: str, default: Any = None) -> Any:
    """
    Read an attribute that may be a property in one runtime and a method in the other.
    """
    attribute = getattr(value, name, default)
    return attribute() if callable(attribute) else attribute


def enum_name(value: Any) -> str:
    """
    Return a stable enum member name across Python and PyO3 representations.
    """
    name = value_or_call(value, "name")
    return str(name if name is not None else value).split(".")[-1]


def stable_value(value: Any) -> str | int | bool | None:
    """
    Project domain objects into exact JSON-compatible scalar values.
    """
    if value is None or isinstance(value, (str, int, bool)):
        return value
    as_decimal = getattr(value, "as_decimal", None)
    if callable(as_decimal):
        return format(as_decimal().normalize(), "f")
    if isinstance(value, Decimal):
        return format(value.normalize(), "f")
    return str(value)


def stable_decimal(value: Any, precision: int) -> str | None:
    """
    Normalize a numeric value at its domain precision.
    """
    if value is None:
        return None
    decimal_value = Decimal(str(value))
    quantum = Decimal(1).scaleb(-precision)
    return format(decimal_value.quantize(quantum).normalize(), "f")


def digest(value: Any) -> str:
    """
    Hash a canonical JSON representation.
    """
    payload = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return f"sha256:{hashlib.sha256(payload).hexdigest()}"


def require_fingerprint_match(
    expected: dict[str, Any],
    actual: dict[str, Any],
    context: str,
) -> None:
    """
    Stop a comparison when a semantic fingerprint changes.
    """
    if actual != expected:
        raise RuntimeError(f"Fingerprint mismatch for {context}")


def fingerprint(bindings: Bindings, engine: Any) -> dict[str, Any]:
    """
    Project exact event, order, position, and account state after a run.
    """
    result = engine.get_result()
    event_counts = {
        "iterations": int(value_or_call(result, "iterations")),
        "total_events": int(value_or_call(result, "total_events")),
        "total_orders": int(value_or_call(result, "total_orders")),
        "total_positions": int(value_or_call(result, "total_positions")),
    }
    orders = []
    for order in engine.cache.orders():
        instrument = engine.cache.instrument(value_or_call(order, "instrument_id"))
        price_precision = int(value_or_call(instrument, "price_precision"))
        filled_qty = stable_value(value_or_call(order, "filled_qty"))
        orders.append(
            {
                "avg_px": (
                    stable_decimal(value_or_call(order, "avg_px"), price_precision)
                    if Decimal(str(filled_qty)) != 0
                    else None
                ),
                "event_count": int(value_or_call(order, "event_count", 0)),
                "filled_qty": filled_qty,
                "leaves_qty": stable_value(value_or_call(order, "leaves_qty")),
                "order_side": enum_name(value_or_call(order, "side")),
                "order_type": enum_name(value_or_call(order, "order_type")),
                "price": stable_value(value_or_call(order, "price")),
                "quantity": stable_value(value_or_call(order, "quantity")),
                "status": enum_name(value_or_call(order, "status")),
                "time_in_force": enum_name(value_or_call(order, "time_in_force")),
                "trigger_price": stable_value(value_or_call(order, "trigger_price")),
            },
        )
    orders.sort(key=lambda item: json.dumps(item, sort_keys=True))
    order_statuses = dict(sorted(Counter(order["status"] for order in orders).items()))
    closed_statuses = {"CANCELED", "DENIED", "EXPIRED", "FILLED", "REJECTED"}
    order_counts = {
        "canceled": order_statuses.get("CANCELED", 0),
        "expired": order_statuses.get("EXPIRED", 0),
        "filled": order_statuses.get("FILLED", 0),
        "open": sum(
            count for status, count in order_statuses.items() if status not in closed_statuses
        ),
        "rejected": order_statuses.get("DENIED", 0) + order_statuses.get("REJECTED", 0),
        "submitted": event_counts["total_orders"],
    }

    positions = []
    for position in engine.cache.positions():
        instrument = engine.cache.instrument(value_or_call(position, "instrument_id"))
        price_precision = int(value_or_call(instrument, "price_precision"))
        is_closed = bool(value_or_call(position, "is_closed"))
        positions.append(
            {
                "avg_px_close": (
                    stable_decimal(value_or_call(position, "avg_px_close"), price_precision)
                    if is_closed
                    else None
                ),
                "avg_px_open": stable_decimal(
                    value_or_call(position, "avg_px_open"),
                    price_precision,
                ),
                "entry": enum_name(value_or_call(position, "entry")),
                "event_count": int(value_or_call(position, "event_count", 0)),
                "instrument_id": stable_value(value_or_call(position, "instrument_id")),
                "quantity": stable_value(value_or_call(position, "quantity")),
                "realized_pnl": stable_value(value_or_call(position, "realized_pnl")),
                "realized_return": stable_value(value_or_call(position, "realized_return")),
                "side": enum_name(value_or_call(position, "side")),
            },
        )
    positions.sort(key=lambda item: json.dumps(item, sort_keys=True))

    accounts = []
    for account in bindings.accounts(engine):
        balances = value_or_call(account, "balances", {})
        balance_values = []
        for currency, balance in balances.items():
            balance_values.append(
                {
                    "currency": stable_value(currency),
                    "free": stable_value(value_or_call(balance, "free")),
                    "locked": stable_value(value_or_call(balance, "locked")),
                    "total": stable_value(value_or_call(balance, "total")),
                },
            )
        balance_values.sort(key=lambda item: json.dumps(item, sort_keys=True))
        accounts.append(
            {
                "balances": balance_values,
                "event_count": int(value_or_call(account, "event_count", 0)),
            },
        )
    accounts.sort(key=lambda item: json.dumps(item, sort_keys=True))

    components = {
        "account_digest": digest(accounts),
        "event_digest": digest(event_counts),
        "order_digest": digest(
            {"counts": order_counts, "orders": orders, "statuses": order_statuses},
        ),
        "position_digest": digest(positions),
    }
    return {
        "account_count": len(accounts),
        "event_counts": event_counts,
        "order_counts": order_counts,
        "order_statuses": order_statuses,
        "position_count": len(positions),
        "digests": {**components, "result_digest": digest(components)},
    }


def run_iteration(bindings: Bindings, scenario: Scenario, boundary: str) -> dict[str, Any]:
    """
    Time one selected boundary and fingerprint the resulting engine state.
    """
    if boundary == "run_preloaded":
        instruments = [make_instrument(bindings, symbol) for symbol in ("BTCUSDT", "ETHUSDT")]
        fixture = make_fixture(bindings, scenario, instruments[: scenario.instruments])
        engine = build_engine(bindings, scenario, fixture)
        started = time.perf_counter_ns()
        engine.run()
        elapsed_ns = time.perf_counter_ns() - started
    elif boundary == "load_build_run":
        started = time.perf_counter_ns()
        instruments = [make_instrument(bindings, symbol) for symbol in ("BTCUSDT", "ETHUSDT")]
        fixture = make_fixture(bindings, scenario, instruments[: scenario.instruments])
        engine = build_engine(bindings, scenario, fixture)
        engine.run()
        elapsed_ns = time.perf_counter_ns() - started
    else:
        raise ValueError(f"Unknown boundary {boundary!r}")
    result = fingerprint(bindings, engine)
    engine.dispose()
    return {"elapsed_ns": elapsed_ns, "fingerprint": result}


def scenario_by_name(name: str) -> Scenario:
    """
    Resolve a requested scenario from the closed comparison matrix.
    """
    try:
        return next(scenario for scenario in SCENARIOS if scenario.name == name)
    except StopIteration as e:
        raise ValueError(f"Unknown scenario {name!r}") from e


def run_worker(args: argparse.Namespace) -> None:
    """
    Warm up and run one isolated scenario with post-sample identity checks.
    """
    bindings = load_bindings()
    scenario = scenario_by_name(args.scenario)
    warmup = run_iteration(bindings, scenario, args.boundary)
    samples = []
    for iteration in range(args.iterations):
        sample = run_iteration(bindings, scenario, args.boundary)
        require_fingerprint_match(
            warmup["fingerprint"],
            sample["fingerprint"],
            f"{scenario.name}/{args.boundary} timed iteration {iteration}",
        )
        sample_identity = runtime_identity(args)
        identity_digest = digest(sample_identity)
        if identity_digest != args.expected_identity_digest:
            raise RuntimeError(
                f"Identity mismatch for {scenario.name}/{args.boundary} "
                f"timed iteration {iteration}: produced {identity_digest}, "
                f"expected {args.expected_identity_digest}",
            )
        sample["runtime_identity"] = sample_identity
        samples.append(sample)
    print(json.dumps({"samples": samples}, sort_keys=True))  # noqa: T201


def file_sha256(path: Path) -> str:
    """
    Hash a file without loading it wholly into memory.
    """
    hasher = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def file_contains(path: Path, expected: bytes) -> bool:
    """
    Search a binary file while preserving matches across read blocks.
    """
    overlap = b""
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            content = overlap + block
            if expected in content:
                return True
            overlap = content[-(len(expected) - 1) :]
    return False


def installed_artifact_url() -> str:
    """
    Return the wheel URL recorded by the installed distribution.
    """
    direct_url = importlib.metadata.distribution("nautilus-trader").read_text("direct_url.json")
    if direct_url is None:
        raise RuntimeError("Installed distribution does not record its source artifact")
    try:
        installed_url = json.loads(direct_url)["url"]
    except (json.JSONDecodeError, KeyError, TypeError) as e:
        raise RuntimeError("Installed distribution has invalid source metadata") from e
    if not isinstance(installed_url, str):
        raise TypeError("Installed distribution has a non-string source URL")
    return installed_url


def require_installed_artifact(artifact: Path) -> str:
    """
    Require the installed distribution to name the supplied wheel.
    """
    installed_url = installed_artifact_url()
    expected_url = artifact.resolve().as_uri()
    if installed_url != expected_url:
        raise RuntimeError(
            f"Installed distribution came from {installed_url!r}, expected {expected_url!r}",
        )
    return installed_url


def extension_path(bindings: Bindings) -> Path:
    """
    Locate the implementation module loaded by the active runtime.
    """
    engine_module = inspect.getmodule(bindings.BacktestEngine)
    path = Path(inspect.getfile(engine_module)).resolve()
    if bindings.generation == "v2":
        import nautilus_trader._libnautilus as extension_module

        path = Path(extension_module.__file__).resolve()
    return path


def require_wheel_extension(artifact: Path, extension: Path) -> tuple[str, str]:
    """
    Require the loaded extension bytes to match the extension stored in the wheel.
    """
    extension_sha256 = file_sha256(extension)
    try:
        package_index = len(extension.parts) - 1 - extension.parts[::-1].index("nautilus_trader")
    except ValueError as e:
        raise RuntimeError(f"Extension is outside the nautilus_trader package: {extension}") from e
    expected_member = "/".join(extension.parts[package_index:])
    with zipfile.ZipFile(artifact) as wheel:
        members = [member for member in wheel.infolist() if member.filename == expected_member]
        if len(members) != 1:
            raise RuntimeError(
                f"Wheel contains {len(members)} members at {expected_member!r}, expected one",
            )
        member = members[0]
        hasher = hashlib.sha256()
        with wheel.open(member) as file:
            for block in iter(lambda: file.read(1024 * 1024), b""):
                hasher.update(block)
    wheel_extension_sha256 = hasher.hexdigest()
    if wheel_extension_sha256 != extension_sha256:
        raise RuntimeError(
            "Loaded extension SHA-256 does not match the extension stored in the wheel",
        )
    return member.filename, wheel_extension_sha256


def require_identity_match(expected: dict[str, Any], actual: dict[str, Any], context: str) -> None:
    """
    Require every named identity field to match its recorded value.
    """
    for field, value in expected.items():
        if actual.get(field) != value:
            raise RuntimeError(
                f"{context} field {field} was {actual.get(field)!r}, expected {value!r}",
            )


def runtime_identity(args: argparse.Namespace) -> dict[str, Any]:
    """
    Prove and record the complete identity of one isolated runtime.
    """
    bindings = load_bindings()
    if bindings.generation == "v1":
        from nautilus_trader.model.objects import HIGH_PRECISION
    else:
        from nautilus_trader.model import HIGH_PRECISION

    version = importlib.metadata.version("nautilus-trader")
    source_root_arg = Path(args.source_root)
    source_root = source_root_arg.resolve()
    git = shutil.which("git")
    if git is None:
        raise RuntimeError("Git is required to verify source identity")
    source_commit = capture_git(git, source_root, "rev-parse", "HEAD").decode().strip()
    if source_commit != args.source_commit:
        raise RuntimeError(
            f"Source root HEAD was {source_commit!r}, expected {args.source_commit!r}",
        )
    source_status = capture_git(git, source_root, "status", "--short", "--untracked-files=all")
    source_index_diff = capture_git(
        git,
        source_root,
        "diff",
        "--cached",
        "--binary",
        "--no-ext-diff",
        "--no-textconv",
    )
    source_worktree_diff = capture_git(
        git,
        source_root,
        "diff",
        "--binary",
        "--no-ext-diff",
        "--no-textconv",
    )
    source_untracked = capture_git(
        git,
        source_root,
        "ls-files",
        "--others",
        "--exclude-standard",
        "-z",
    )
    artifact = Path(args.artifact)
    loaded_extension = extension_path(bindings)
    wheel_extension_path, wheel_extension_sha256 = require_wheel_extension(
        artifact,
        loaded_extension,
    )
    embedded_source_commit = None
    if bindings.generation == "v2" and file_contains(loaded_extension, args.source_commit.encode()):
        embedded_source_commit = args.source_commit
    identity = {
        "artifact_path": str(artifact.resolve()),
        "artifact_sha256": file_sha256(artifact),
        "backend": "cython" if bindings.generation == "v1" else "pyo3",
        "engine_module": bindings.BacktestEngine.__module__,
        "embedded_source_commit": embedded_source_commit,
        "extension_path": str(loaded_extension),
        "extension_sha256": wheel_extension_sha256,
        "high_precision": bool(HIGH_PRECISION),
        "installed_artifact_url": require_installed_artifact(artifact),
        "package_version": version,
        "python_executable": sys.executable,
        "python_version": platform.python_version(),
        "source_commit": source_commit,
        "source_index_diff_sha256": hashlib.sha256(source_index_diff).hexdigest(),
        "source_root": str(source_root_arg),
        "source_status_count": len(source_status.splitlines()),
        "source_status_sha256": hashlib.sha256(source_status).hexdigest(),
        "source_untracked_sha256": untracked_files_sha256(source_root, source_untracked),
        "source_worktree_diff_sha256": hashlib.sha256(source_worktree_diff).hexdigest(),
        "wheel_extension_path": wheel_extension_path,
        "wheel_extension_sha256": wheel_extension_sha256,
    }
    expected = {
        "backend": args.expected_backend,
        "engine_module": (
            "nautilus_trader.backtest.engine"
            if args.expected_backend == "cython"
            else "nautilus_trader.backtest"
        ),
        "package_version": args.expected_version,
    }
    if args.expected_backend == "pyo3":
        expected["embedded_source_commit"] = args.source_commit
    require_identity_match(expected, identity, "Identity")
    return identity


def capture_git(git: str, source_root: Path, *arguments: str) -> bytes:
    """
    Capture exact bytes from one read-only Git command.
    """
    return subprocess.run(  # noqa: S603
        [git, *arguments],
        cwd=source_root,
        check=True,
        capture_output=True,
    ).stdout


def untracked_files_sha256(source_root: Path, paths: bytes) -> str:
    """
    Hash untracked paths, file modes, and contents without following symlinks.
    """
    hasher = hashlib.sha256()
    for raw_path in sorted(filter(None, paths.split(b"\0"))):
        path = source_root / os.fsdecode(raw_path)
        file_stat = path.lstat()
        hasher.update(len(raw_path).to_bytes(8, "big"))
        hasher.update(raw_path)
        hasher.update(file_stat.st_mode.to_bytes(8, "big"))
        hasher.update(file_stat.st_size.to_bytes(8, "big"))
        if stat.S_ISLNK(file_stat.st_mode):
            hasher.update(os.fsencode(path.readlink()))
        elif stat.S_ISREG(file_stat.st_mode):
            with path.open("rb") as file:
                for block in iter(lambda: file.read(1024 * 1024), b""):
                    hasher.update(block)
        else:
            raise RuntimeError(f"Unsupported untracked file type: {path}")
    return hasher.hexdigest()


def run_identity(args: argparse.Namespace) -> None:
    """
    Emit one verified runtime identity as JSON.
    """
    print(json.dumps(runtime_identity(args), sort_keys=True))  # noqa: T201


def environment_metadata() -> dict[str, Any]:
    """
    Capture host controls and load without changing them.
    """
    cpu_model = "unknown"
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.exists():
        for line in cpuinfo.read_text(encoding="utf-8").splitlines():
            if line.startswith("model name"):
                cpu_model = line.partition(":")[2].strip()
                break
    governor_paths = Path("/sys/devices/system/cpu").glob("cpu*/cpufreq/scaling_governor")
    governors = sorted({path.read_text(encoding="utf-8").strip() for path in governor_paths})
    paranoid_path = Path("/proc/sys/kernel/perf_event_paranoid")
    return {
        "argv": sys.argv,
        "cpu_count": os.cpu_count(),
        "cpu_governors": governors,
        "cpu_model": cpu_model,
        "kernel": platform.release(),
        "load_average": os.getloadavg(),
        "machine": platform.machine(),
        "perf_event_paranoid": (
            paranoid_path.read_text(encoding="utf-8").strip() if paranoid_path.exists() else None
        ),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "sessions": None,
    }


def invoke_json(command: list[str]) -> dict[str, Any]:
    """
    Run a constructed worker command and decode its JSON response.
    """
    completed = subprocess.run(  # noqa: S603
        command,
        check=True,
        capture_output=True,
        text=True,
    )
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as e:
        details = f"{completed.stdout}\n{completed.stderr}"
        raise RuntimeError(
            f"Command did not return JSON: {' '.join(command)}\n{details}",
        ) from e


def identity_command(
    script: Path,
    runtime: Runtime,
) -> list[str]:
    """
    Build the subprocess command that proves a complete runtime identity.
    """
    return [str(runtime.python), str(script), "identity", *runtime_identity_arguments(runtime)]


def runtime_identity_arguments(runtime: Runtime) -> list[str]:
    """
    Build arguments shared by identity and timed worker commands.
    """
    return [
        "--artifact",
        str(runtime.artifact),
        "--source-commit",
        runtime.commit,
        "--source-root",
        str(runtime.source_root),
        "--expected-version",
        runtime.version,
        "--expected-backend",
        runtime.backend,
    ]


def worker_command(
    script: Path,
    runtime: Runtime,
    identity: dict[str, Any],
    case: tuple[Scenario, str],
    iterations: int,
) -> list[str]:
    """
    Build a timed worker command bound to its initial runtime identity.
    """
    scenario, boundary = case
    return [
        str(runtime.python),
        str(script),
        "worker",
        "--scenario",
        scenario.name,
        "--boundary",
        boundary,
        "--iterations",
        str(iterations),
        "--expected-identity-digest",
        digest(identity),
        *runtime_identity_arguments(runtime),
    ]


def select_cases(
    scenario_names: list[str] | None = None,
    boundary_names: list[str] | None = None,
) -> list[tuple[Scenario, str]]:
    """
    Select benchmark cases while retaining canonical matrix order.
    """
    known_scenarios = {scenario.name for scenario in SCENARIOS}
    requested_scenarios = set(scenario_names or known_scenarios)
    unknown_scenarios = requested_scenarios - known_scenarios
    if unknown_scenarios:
        raise ValueError(f"Unknown scenarios: {sorted(unknown_scenarios)!r}")

    requested_boundaries = set(boundary_names or BOUNDARIES)
    unknown_boundaries = requested_boundaries - set(BOUNDARIES)
    if unknown_boundaries:
        raise ValueError(f"Unknown boundaries: {sorted(unknown_boundaries)!r}")

    return [
        (scenario, boundary)
        for scenario in SCENARIOS
        if scenario.name in requested_scenarios
        for boundary in BOUNDARIES
        if boundary in requested_boundaries
    ]


def ordered_cases(
    session: int,
    cases: list[tuple[Scenario, str]] | None = None,
) -> list[tuple[Scenario, str]]:
    """
    Rotate and reverse cases so neighboring runtime order changes by session.
    """
    cases = list(cases if cases is not None else select_cases())
    if not cases:
        raise ValueError("At least one benchmark case is required")
    offset = session % len(cases)
    if session % 2:
        cases.reverse()
        offset = (offset + 1) % len(cases)
    return cases[offset:] + cases[:offset]


def summarize(samples: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """
    Calculate per-case medians, spreads, ratios, and gaps from raw samples.
    """
    grouped: dict[tuple[str, str], dict[str, list[int]]] = {}
    for sample in samples:
        key = (sample["scenario"], sample["boundary"])
        grouped.setdefault(key, {"v1": [], "v2": []})[sample["version"]].append(
            sample["elapsed_ns"],
        )
    summaries = []
    for (scenario, boundary), versions in sorted(grouped.items()):
        v1 = versions["v1"]
        v2 = versions["v2"]
        v1_median = statistics.median(v1)
        v2_median = statistics.median(v2)
        summaries.append(
            {
                "boundary": boundary,
                "scenario": scenario,
                "v1_max_ns": max(v1),
                "v1_median_ns": v1_median,
                "v1_min_ns": min(v1),
                "v1_spread_percent": (max(v1) - min(v1)) / v1_median * 100,
                "v1_spread_ns": max(v1) - min(v1),
                "v2_max_ns": max(v2),
                "v2_median_ns": v2_median,
                "v2_min_ns": min(v2),
                "v2_spread_percent": (max(v2) - min(v2)) / v2_median * 100,
                "v2_spread_ns": max(v2) - min(v2),
                "v2_v1_gap_ns": v2_median - v1_median,
                "v2_v1_gap_percent": (v2_median / v1_median - 1) * 100,
                "v2_v1_ratio": v2_median / v1_median,
            },
        )
    return summaries


def run_compare(args: argparse.Namespace) -> None:
    """
    Coordinate the exact-identity interleaved comparison and write its raw record.
    """
    if args.sessions < MIN_SESSIONS:
        raise ValueError("At least three full sessions are required")
    script = Path(__file__).resolve()
    cases = select_cases(args.scenarios, args.boundaries)
    case_indices = {
        (scenario.name, boundary): index for index, (scenario, boundary) in enumerate(cases)
    }
    scenarios = tuple(dict.fromkeys(scenario for scenario, _boundary in cases))
    boundaries = tuple(dict.fromkeys(boundary for _scenario, boundary in cases))
    runtimes = {
        "v1": Runtime(
            python=Path(args.v1_python),
            artifact=Path(args.v1_artifact),
            source_root=Path(args.v1_source),
            commit=args.v1_commit,
            version="1.231.0",
            backend="cython",
        ),
        "v2": Runtime(
            python=Path(args.v2_python),
            artifact=Path(args.v2_artifact),
            source_root=Path(args.v2_source),
            commit=args.v2_commit,
            version="2.0.0rc5",
            backend="pyo3",
        ),
    }
    started_at = dt.datetime.now(tz=dt.UTC)
    environment_before = environment_metadata()
    identities = {}
    for name, runtime in runtimes.items():
        identities[name] = invoke_json(identity_command(script, runtime))
    if identities["v1"]["python_version"] != identities["v2"]["python_version"]:
        raise RuntimeError("The v1 and v2 runtimes do not use the same Python version")
    if identities["v1"]["high_precision"] != identities["v2"]["high_precision"]:
        raise RuntimeError("The v1 and v2 runtimes do not use the same precision mode")

    samples = []
    expected_fingerprints: dict[tuple[str, str], dict[str, Any]] = {}
    for session in range(args.sessions):
        for scenario, boundary in ordered_cases(session, cases):
            case_index = case_indices[(scenario.name, boundary)]
            version_order = ["v1", "v2"] if (session + case_index) % 2 == 0 else ["v2", "v1"]
            for version in version_order:
                runtime = runtimes[version]
                output = invoke_json(
                    worker_command(
                        script,
                        runtime,
                        identities[version],
                        (scenario, boundary),
                        args.iterations,
                    ),
                )
                for iteration, sample in enumerate(output["samples"]):
                    require_identity_match(
                        identities[version],
                        sample["runtime_identity"],
                        f"{version} sample identity",
                    )
                    key = (scenario.name, boundary)
                    expected = expected_fingerprints.setdefault(key, sample["fingerprint"])
                    require_fingerprint_match(
                        expected,
                        sample["fingerprint"],
                        f"{scenario.name}/{boundary} {version} timed iteration {iteration}",
                    )
                    samples.append(
                        {
                            "boundary": boundary,
                            "elapsed_ns": sample["elapsed_ns"],
                            "fingerprint_digest": digest(sample["fingerprint"]),
                            "iteration": iteration,
                            "runtime_identity_digest": digest(sample["runtime_identity"]),
                            "scenario": scenario.name,
                            "session": session + 1,
                            "version": version,
                        },
                    )

    final_identities = {
        name: invoke_json(identity_command(script, runtime)) for name, runtime in runtimes.items()
    }
    require_identity_match(identities, final_identities, "Final runtime identity")
    environment_after = environment_metadata()
    environment_after["sessions"] = args.sessions
    fingerprints = []
    for scenario, boundary in cases:
        fingerprint_value = expected_fingerprints[(scenario.name, boundary)]
        fingerprints.append(
            {
                "boundary": boundary,
                "fingerprint": fingerprint_value,
                "fingerprint_digest": digest(fingerprint_value),
                "scenario": scenario.name,
            },
        )
    output = {
        "boundaries": {boundary: BOUNDARY_DESCRIPTIONS[boundary] for boundary in boundaries},
        "driver_sha256": file_sha256(script),
        "ended_at_utc": dt.datetime.now(tz=dt.UTC).isoformat(),
        "environment_after": environment_after,
        "environment_before": environment_before,
        "fingerprints": fingerprints,
        "identities": identities,
        "iterations_per_case": args.iterations,
        "matrix": [dataclasses.asdict(scenario) for scenario in scenarios],
        "samples": samples,
        "sessions": args.sessions,
        "started_at_utc": started_at.isoformat(),
        "summaries": summarize(samples),
    }
    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def parser() -> argparse.ArgumentParser:
    """
    Build the identity, worker, and comparison command-line interface.
    """
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="command", required=True)

    identity = commands.add_parser("identity")
    add_runtime_identity_arguments(identity)
    identity.set_defaults(func=run_identity)

    worker = commands.add_parser("worker")
    worker.add_argument("--scenario", choices=tuple(case.name for case in SCENARIOS), required=True)
    worker.add_argument("--boundary", choices=BOUNDARIES, required=True)
    worker.add_argument("--iterations", type=int, default=1)
    worker.add_argument("--expected-identity-digest", required=True)
    add_runtime_identity_arguments(worker)
    worker.set_defaults(func=run_worker)

    compare = commands.add_parser("compare")
    compare.add_argument("--v1-python", required=True)
    compare.add_argument("--v1-artifact", required=True)
    compare.add_argument("--v1-commit", required=True)
    compare.add_argument("--v1-source", required=True)
    compare.add_argument("--v2-python", required=True)
    compare.add_argument("--v2-artifact", required=True)
    compare.add_argument("--v2-commit", required=True)
    compare.add_argument("--v2-source", required=True)
    compare.add_argument(
        "--scenario",
        action="append",
        choices=tuple(case.name for case in SCENARIOS),
        dest="scenarios",
    )
    compare.add_argument(
        "--boundary",
        action="append",
        choices=BOUNDARIES,
        dest="boundaries",
    )
    compare.add_argument("--sessions", type=int, default=3)
    compare.add_argument("--iterations", type=int, default=1)
    compare.add_argument("--output", required=True)
    compare.set_defaults(func=run_compare)
    return root


def add_runtime_identity_arguments(command: argparse.ArgumentParser) -> None:
    """
    Add the arguments required to prove a complete runtime identity.
    """
    command.add_argument("--artifact", required=True)
    command.add_argument("--source-commit", required=True)
    command.add_argument("--source-root", required=True)
    command.add_argument("--expected-version", required=True)
    command.add_argument("--expected-backend", choices=("cython", "pyo3"), required=True)


def main() -> None:
    """
    Parse command-line arguments and run the selected mode.
    """
    args = parser().parse_args()
    if getattr(args, "iterations", 1) < 1:
        raise ValueError("Iterations must be positive")
    args.func(args)


if __name__ == "__main__":
    main()
