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
Acceptance tests for the BacktestEngine.

These tests validate the full stack: config -> engine -> venue -> matching ->
strategy -> results.

"""

from __future__ import annotations

import math

import pytest

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import Venue
from nautilus_trader.trading import ImportableStrategyConfig
from tests.providers import TestDataProvider
from tests.providers import TestInstrumentProvider


EMA_CROSS_STRATEGY = "strategies.ema_cross:EMACross"
EMA_CROSS_CONFIG = "strategies.ema_cross:EMACrossConfig"


def _make_ema_config(instrument_id, bar_type, trade_size="100000", fast=10, slow=20):
    return ImportableStrategyConfig(
        strategy_path=EMA_CROSS_STRATEGY,
        config_path=EMA_CROSS_CONFIG,
        config={
            "instrument_id": str(instrument_id),
            "bar_type": bar_type,
            "trade_size": trade_size,
            "fast_ema_period": fast,
            "slow_ema_period": slow,
        },
    )


@pytest.fixture
def usdjpy_engine():
    config = BacktestEngineConfig(bypass_logging=True, run_analysis=False)
    engine = BacktestEngine(config)

    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USD"),
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
    )

    usdjpy = TestInstrumentProvider.usdjpy_sim()
    engine.add_instrument(usdjpy)

    ticks = TestDataProvider.usdjpy_quotes()
    engine.add_data(ticks)

    yield engine, usdjpy
    engine.dispose()


@pytest.fixture
def audusd_engine():
    config = BacktestEngineConfig(bypass_logging=True, run_analysis=False)
    engine = BacktestEngine(config)

    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USD"),
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
    )

    audusd = TestInstrumentProvider.audusd_sim()
    engine.add_instrument(audusd)

    yield engine, audusd
    engine.dispose()


@pytest.fixture
def ethusdt_engine():
    config = BacktestEngineConfig(bypass_logging=True, run_analysis=False)
    engine = BacktestEngine(config)

    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        starting_balances=[
            Money(10.0, Currency.from_str("ETH")),
            Money(10_000_000.0, Currency.from_str("USDT")),
        ],
    )

    ethusdt = TestInstrumentProvider.ethusdt_binance()
    engine.add_instrument(ethusdt)

    yield engine, ethusdt
    engine.dispose()


def test_run_ema_cross_strategy(usdjpy_engine):
    engine, usdjpy = usdjpy_engine

    engine.add_strategy_from_config(
        _make_ema_config(usdjpy.id, "USD/JPY.SIM-1-MINUTE-BID-INTERNAL"),
    )

    engine.run()
    result = engine.get_result()

    assert result.iterations > 0
    assert result.total_orders > 0
    assert result.total_positions > 0
    assert result.total_events > 0


@pytest.mark.skip(reason="WIP: duplicate trade IDs across reset, needs unique ID generation")
def test_run_ema_cross_deterministic_rerun(usdjpy_engine):
    engine, usdjpy = usdjpy_engine

    engine.add_strategy_from_config(
        _make_ema_config(usdjpy.id, "USD/JPY.SIM-1-MINUTE-BID-INTERNAL"),
    )

    engine.run()
    result1 = engine.get_result()

    engine.reset()
    engine.run()
    result2 = engine.get_result()

    assert result1.total_orders == result2.total_orders
    assert result1.total_positions == result2.total_positions
    assert result1.iterations == result2.iterations


def test_run_ema_cross_with_5min_bars(usdjpy_engine):
    engine, usdjpy = usdjpy_engine

    engine.add_strategy_from_config(
        _make_ema_config(usdjpy.id, "USD/JPY.SIM-5-MINUTE-BID-INTERNAL"),
    )

    engine.run()
    result = engine.get_result()

    assert result.iterations > 0
    assert result.total_orders > 0


def test_run_with_synthetic_quotes(audusd_engine):
    engine, audusd = audusd_engine

    instrument_id = audusd.id
    base_ns = 1_000_000_000_000_000_000

    ticks = []
    for i in range(3000):
        ts = base_ns + i * 1_000_000_000
        bid = 0.71000 + 0.00500 * math.sin(i / 300.0)
        ask = bid + 0.00010
        ticks.append(
            QuoteTick(
                instrument_id=instrument_id,
                bid_price=Price(bid, precision=5),
                ask_price=Price(ask, precision=5),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=ts,
                ts_init=ts,
            ),
        )

    engine.add_data(ticks)
    engine.add_strategy_from_config(
        _make_ema_config(audusd.id, "AUD/USD.SIM-1-MINUTE-BID-INTERNAL"),
    )

    engine.run()
    result = engine.get_result()

    assert result.iterations == len(ticks)
    assert result.total_events > 0


def test_run_with_synthetic_trades(ethusdt_engine):
    engine, ethusdt = ethusdt_engine

    instrument_id = ethusdt.id
    base_ns = 1_000_000_000_000_000_000

    ticks = []
    for i in range(5000):
        ts = base_ns + i * 500_000_000
        price = 1500.00 + 50.0 * math.sin(i / 200.0)
        ticks.append(
            TradeTick(
                instrument_id=instrument_id,
                price=Price(price, precision=2),
                size=Quantity(1.0, precision=5),
                aggressor_side=AggressorSide.BUYER if i % 2 == 0 else AggressorSide.SELLER,
                trade_id=TradeId(str(i)),
                ts_event=ts,
                ts_init=ts,
            ),
        )

    engine.add_data(ticks)
    engine.add_strategy_from_config(
        _make_ema_config(
            ethusdt.id,
            "ETHUSDT.BINANCE-100-TICK-LAST-INTERNAL",
            trade_size="0.10000",
        ),
    )

    engine.run()
    result = engine.get_result()

    assert result.iterations == len(ticks)
    assert result.total_events > 0


def test_engine_construction():
    config = BacktestEngineConfig()
    engine = BacktestEngine(config)
    assert engine.trader_id is not None
    assert engine.instance_id is not None
    assert engine.iteration == 0
    engine.dispose()


def test_engine_construction_with_bypass_logging():
    config = BacktestEngineConfig(bypass_logging=True)
    engine = BacktestEngine(config)
    assert engine.iteration == 0
    engine.dispose()


def test_engine_add_venue():
    config = BacktestEngineConfig(bypass_logging=True)
    engine = BacktestEngine(config)

    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
        base_currency=Currency.from_str("USD"),
    )

    venues = engine.list_venues()
    assert Venue("SIM") in venues
    engine.dispose()


def test_engine_add_instrument():
    config = BacktestEngineConfig(bypass_logging=True)
    engine = BacktestEngine(config)

    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
        base_currency=Currency.from_str("USD"),
    )

    usdjpy = TestInstrumentProvider.usdjpy_sim()
    engine.add_instrument(usdjpy)
    engine.dispose()


def test_engine_add_data():
    config = BacktestEngineConfig(bypass_logging=True)
    engine = BacktestEngine(config)

    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
        base_currency=Currency.from_str("USD"),
    )

    usdjpy = TestInstrumentProvider.usdjpy_sim()
    engine.add_instrument(usdjpy)

    ticks = [
        QuoteTick(
            instrument_id=usdjpy.id,
            bid_price=Price(110.100, precision=3),
            ask_price=Price(110.110, precision=3),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=1_000_000_000,
            ts_init=1_000_000_000,
        ),
    ]

    engine.add_data(ticks)
    engine.dispose()


def test_engine_run_empty_produces_zero_iterations():
    config = BacktestEngineConfig(bypass_logging=True, run_analysis=False)
    engine = BacktestEngine(config)

    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
        base_currency=Currency.from_str("USD"),
    )

    engine.run()
    result = engine.get_result()
    assert result.iterations == 0
    engine.dispose()


def test_engine_reset_allows_rerun():
    config = BacktestEngineConfig(bypass_logging=True, run_analysis=False)
    engine = BacktestEngine(config)

    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
        base_currency=Currency.from_str("USD"),
    )

    engine.run()
    engine.reset()
    engine.run()
    result = engine.get_result()
    assert result.iterations == 0
    engine.dispose()


def test_result_stats_not_empty_after_run():
    config = BacktestEngineConfig(bypass_logging=True, run_analysis=True)
    engine = BacktestEngine(config)

    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USD"),
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
    )

    usdjpy = TestInstrumentProvider.usdjpy_sim()
    engine.add_instrument(usdjpy)

    ticks = TestDataProvider.usdjpy_quotes()
    engine.add_data(ticks)

    engine.add_strategy_from_config(
        _make_ema_config(usdjpy.id, "USD/JPY.SIM-1-MINUTE-BID-INTERNAL"),
    )

    engine.run()
    result = engine.get_result()

    assert result.trader_id is not None
    assert result.machine_id is not None
    assert result.instance_id is not None
    assert result.elapsed_time_secs > 0
    assert isinstance(result.stats_general, dict)
    assert isinstance(result.stats_pnls, dict)
    assert isinstance(result.stats_returns, dict)
    engine.dispose()


def test_two_venues_with_separate_instruments():
    config = BacktestEngineConfig(bypass_logging=True, run_analysis=False)
    engine = BacktestEngine(config)

    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USD"),
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
    )
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        starting_balances=[
            Money(10.0, Currency.from_str("ETH")),
            Money(100_000.0, Currency.from_str("USDT")),
        ],
    )

    venues = engine.list_venues()
    assert Venue("SIM") in venues
    assert Venue("BINANCE") in venues
    engine.dispose()
