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
Acceptance tests for the v2 BacktestEngine.

This suite mirrors the v1 acceptance suite under `tests/acceptance_tests/test_backtest.py`
so we can validate v2 feature parity. Tests that depend on v2 features that have not yet
been ported are marked with `pytest.skip` and a `v2 missing: ...` reason.

Magic-number assertions from the v1 suite (msgbus counts, exact balances) are not
replicated since v2's runtime has different internal counters; instead we assert on the
publicly-exposed `BacktestResult` (iterations, total_orders, total_positions,
total_events) and broad invariants (e.g. balance moved, ran without error).

"""

from __future__ import annotations

import math

import pytest

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.execution import ExecutionEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import BarType
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import Venue
from nautilus_trader.risk import RiskEngineConfig
from nautilus_trader.trading import ImportableStrategyConfig
from tests.providers import TestDataProvider
from tests.providers import TestInstrumentProvider


EMA_CROSS_STRATEGY = "strategies.ema_cross:EMACross"
EMA_CROSS_CONFIG = "strategies.ema_cross:EMACrossConfig"

BAR_ENTRY_EXIT_STRATEGY = "strategies.acceptance:BarEntryExit"
BAR_ENTRY_EXIT_CONFIG = "strategies.acceptance:BarEntryExitConfig"

TICK_SCHEDULED_STRATEGY = "strategies.acceptance:TickScheduled"
TICK_SCHEDULED_CONFIG = "strategies.acceptance:TickScheduledConfig"

CASCADING_STOP_STRATEGY = "strategies.acceptance:CascadingStop"
CASCADING_STOP_CONFIG = "strategies.acceptance:CascadingStopConfig"

MULTI_CASCADE_STRATEGY = "strategies.acceptance:MultiCascade"
MULTI_CASCADE_CONFIG = "strategies.acceptance:MultiCascadeConfig"

DUAL_TIMER_STRATEGY = "strategies.acceptance:DualTimer"
DUAL_TIMER_CONFIG = "strategies.acceptance:DualTimerConfig"


def _engine(
    *,
    snapshot_orders: bool = False,
    snapshot_positions: bool = False,
    risk_bypass: bool = False,
) -> BacktestEngine:
    if snapshot_orders or snapshot_positions:
        exec_engine = ExecutionEngineConfig(
            snapshot_orders=snapshot_orders,
            snapshot_positions=snapshot_positions,
            snapshot_positions_interval_secs=10,
        )
    else:
        exec_engine = None

    config = BacktestEngineConfig(
        bypass_logging=True,
        run_analysis=False,
        exec_engine=exec_engine,
        risk_engine=RiskEngineConfig(bypass=True) if risk_bypass else None,
    )
    return BacktestEngine(config)


def _ema_config(instrument_id, bar_type, trade_size="1000000", fast=10, slow=20):
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


class TestBacktestAcceptanceTestsUSDJPY:
    def setup_method(self):
        self.engine = _engine(snapshot_orders=True, snapshot_positions=True)
        self.venue = Venue("SIM")
        self.usdjpy = TestInstrumentProvider.usdjpy_sim()

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=Currency.from_str("USD"),
            starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
            # FXRolloverInterestModule(records=[...]) supported in v2 via
            # InterestRateRecord; not exercised here for parity-validation focus.
        )
        self.engine.add_instrument(self.usdjpy)

        ticks = TestDataProvider.quotes_from_fxcm_bars(
            self.usdjpy,
            bid_csv="fxcm/usdjpy-m1-bid-2013.csv",
            ask_csv="fxcm/usdjpy-m1-ask-2013.csv",
            max_rows=2_000,  # ~8k ticks (4 ticks/bar) — keeps suite under a minute
        )
        self.engine.add_data(ticks)

    def teardown_method(self):
        self.engine.dispose()

    def test_run_ema_cross_strategy(self):
        self.engine.add_strategy_from_config(
            _ema_config(self.usdjpy.id, "USD/JPY.SIM-15-MINUTE-BID-INTERNAL"),
        )

        self.engine.run()
        result = self.engine.get_result()

        assert result.iterations > 0
        assert result.total_orders > 0
        assert result.total_positions > 0
        assert result.total_events > 0

    def test_rerun_ema_cross_strategy_returns_identical_performance(self):
        self.engine.add_strategy_from_config(
            _ema_config(self.usdjpy.id, "USD/JPY.SIM-15-MINUTE-BID-INTERNAL"),
        )

        usd = Currency.from_str("USD")

        self.engine.run()
        result1 = self.engine.get_result()
        cache = self.engine.cache
        orders_total_1 = cache.orders_total_count()
        positions_total_1 = cache.positions_total_count()
        account_1 = cache.account_for_venue(self.venue)
        assert account_1 is not None
        balance_1 = account_1.balance_total(usd)
        event_count_1 = account_1.event_count

        self.engine.reset()
        self.engine.run()
        result2 = self.engine.get_result()
        cache = self.engine.cache
        orders_total_2 = cache.orders_total_count()
        positions_total_2 = cache.positions_total_count()
        account_2 = cache.account_for_venue(self.venue)
        assert account_2 is not None
        balance_2 = account_2.balance_total(usd)
        event_count_2 = account_2.event_count

        # Sanity: the strategy actually traded. Without these, the cross-run
        # equality assertions could pass trivially (e.g. a regression that
        # returns 0/None for both runs).
        assert result1.iterations > 0
        assert result1.total_orders > 0
        assert result1.total_positions > 0
        assert orders_total_1 > 0
        assert positions_total_1 > 0
        assert balance_1 is not None
        assert event_count_1 >= 1

        assert result1.iterations == result2.iterations
        assert result1.total_orders == result2.total_orders
        assert result1.total_positions == result2.total_positions
        assert orders_total_1 == orders_total_2
        assert positions_total_1 == positions_total_2
        assert balance_1 == balance_2
        assert event_count_1 == event_count_2

    def test_run_multiple_strategies(self):
        # v1 uses order_id_tag="001" / "002" to disambiguate two EMACross instances.
        # In v2 the StrategyConfig is a Rust @final type whose pyo3 init enforces
        # `strategy_id: StrategyId | None`, so we cannot route order_id_tag through
        # `super().__init__(**kwargs)`. EMACrossConfig instead exposes a string
        # strategy_id that the engine converts via `StrategyId.new_checked`.
        self.engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path=EMA_CROSS_STRATEGY,
                config_path=EMA_CROSS_CONFIG,
                config={
                    "instrument_id": str(self.usdjpy.id),
                    "bar_type": "USD/JPY.SIM-15-MINUTE-BID-INTERNAL",
                    "trade_size": "1000000",
                    "fast_ema_period": 10,
                    "slow_ema_period": 20,
                    "strategy_id": "EMACross-001",
                },
            ),
        )
        self.engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path=EMA_CROSS_STRATEGY,
                config_path=EMA_CROSS_CONFIG,
                config={
                    "instrument_id": str(self.usdjpy.id),
                    "bar_type": "USD/JPY.SIM-15-MINUTE-BID-INTERNAL",
                    "trade_size": "1000000",
                    "fast_ema_period": 20,
                    "slow_ema_period": 40,
                    "strategy_id": "EMACross-002",
                },
            ),
        )

        self.engine.run()
        result = self.engine.get_result()

        assert result.iterations > 0
        assert result.total_orders > 0
        assert result.total_positions > 0


class TestBacktestAcceptanceTestsGBPUSDBarsInternal:
    def setup_method(self):
        self.engine = _engine(snapshot_orders=True, snapshot_positions=True)
        self.venue = Venue("SIM")
        self.gbpusd = TestInstrumentProvider.gbpusd_sim()

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=Currency.from_str("GBP"),
            starting_balances=[Money(1_000_000.0, Currency.from_str("GBP"))],
        )
        self.engine.add_instrument(self.gbpusd)

        ticks = TestDataProvider.quotes_from_fxcm_bars(
            self.gbpusd,
            bid_csv="fxcm/gbpusd-m1-bid-2012.csv",
            ask_csv="fxcm/gbpusd-m1-ask-2012.csv",
            max_rows=2_000,
        )
        self.engine.add_data(ticks)

    def teardown_method(self):
        self.engine.dispose()

    def test_run_ema_cross_with_five_minute_bar_spec(self):
        self.engine.add_strategy_from_config(
            _ema_config(self.gbpusd.id, "GBP/USD.SIM-5-MINUTE-MID-INTERNAL"),
        )

        self.engine.run()
        result = self.engine.get_result()

        assert result.iterations > 0
        assert result.total_orders > 0

    @pytest.mark.skip(reason="v2 missing: EMACrossStopEntry example strategy")
    def test_run_ema_cross_stop_entry_trail_strategy(self):
        pass

    @pytest.mark.skip(reason="v2 missing: EMACrossTrailingStop example strategy + emulator")
    def test_run_ema_cross_stop_entry_trail_strategy_with_emulation(self):
        pass


class TestBacktestAcceptanceTestsGBPUSDBarsExternal:
    def setup_method(self):
        self.engine = _engine(risk_bypass=True)
        self.venue = Venue("SIM")
        self.gbpusd = TestInstrumentProvider.gbpusd_sim()

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=Currency.from_str("USD"),
            starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
        )
        self.engine.add_instrument(self.gbpusd)

        bid_bars = TestDataProvider.bars_from_fxcm_bars(
            self.gbpusd,
            bar_type=BarType.from_str("GBP/USD.SIM-1-MINUTE-BID-EXTERNAL"),
            bid_or_ask_csv="fxcm/gbpusd-m1-bid-2012.csv",
            max_rows=5_000,
        )
        ask_bars = TestDataProvider.bars_from_fxcm_bars(
            self.gbpusd,
            bar_type=BarType.from_str("GBP/USD.SIM-1-MINUTE-ASK-EXTERNAL"),
            bid_or_ask_csv="fxcm/gbpusd-m1-ask-2012.csv",
            max_rows=5_000,
        )
        self.engine.add_data(bid_bars)
        self.engine.add_data(ask_bars)

    def teardown_method(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        self.engine.add_strategy_from_config(
            _ema_config(self.gbpusd.id, "GBP/USD.SIM-1-MINUTE-BID-EXTERNAL"),
        )

        self.engine.run()
        result = self.engine.get_result()

        assert result.iterations > 0
        assert result.total_orders > 0
        assert result.total_positions > 0


class TestBacktestAcceptanceTestsBTCUSDTEmaCrossTWAP:
    def setup_method(self):
        self.engine = _engine(risk_bypass=True)
        self.venue = Venue("BINANCE")
        self.btcusdt = TestInstrumentProvider.btcusdt_binance()

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.CASH,
            starting_balances=[
                Money(10.0, Currency.from_str("BTC")),
                Money(10_000_000.0, Currency.from_str("USDT")),
            ],
        )
        self.engine.add_instrument(self.btcusdt)

    def teardown_method(self):
        self.engine.dispose()

    @pytest.mark.skip(reason="v2 missing: TWAPExecAlgorithm + EMACrossTWAP example strategy")
    def test_run_ema_cross_with_minute_trade_bars(self):
        pass

    def test_run_ema_cross_with_trade_ticks_from_bar_data(self):
        bars = TestDataProvider.bars_from_binance_csv(
            self.btcusdt,
            bar_type=BarType.from_str("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            csv_name="btc-perp-20211231-20220201_1m.csv",
            max_rows=5_000,
        )
        ticks: list[QuoteTick] = [
            QuoteTick(
                instrument_id=self.btcusdt.id,
                bid_price=px,
                ask_price=px,
                bid_size=Quantity(1.0, precision=self.btcusdt.size_precision),
                ask_size=Quantity(1.0, precision=self.btcusdt.size_precision),
                ts_event=bar.ts_event,
                ts_init=bar.ts_init,
            )
            for bar in bars
            for px in (bar.open, bar.high, bar.low, bar.close)
        ]
        self.engine.add_data(ticks)

        self.engine.add_strategy_from_config(
            _ema_config(
                self.btcusdt.id,
                "BTCUSDT.BINANCE-1-MINUTE-BID-INTERNAL",
                trade_size="0.001000",
            ),
        )

        self.engine.run()
        result = self.engine.get_result()
        assert result.iterations == len(ticks)
        assert result.total_events > 0


class TestBacktestAcceptanceTestsAUDUSD:
    def setup_method(self):
        self.engine = _engine(snapshot_orders=True, snapshot_positions=True)
        self.venue = Venue("SIM")
        self.audusd = TestInstrumentProvider.audusd_sim()

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=Currency.from_str("AUD"),
            starting_balances=[Money(1_000_000.0, Currency.from_str("AUD"))],
        )
        self.engine.add_instrument(self.audusd)

        ticks = TestDataProvider.quotes_from_truefx_csv(
            self.audusd,
            csv_name="truefx/audusd-ticks.csv",
            max_rows=20_000,
        )
        self.engine.add_data(ticks)

    def teardown_method(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        self.engine.add_strategy_from_config(
            _ema_config(self.audusd.id, "AUD/USD.SIM-1-MINUTE-MID-INTERNAL"),
        )

        self.engine.run()
        result = self.engine.get_result()

        assert result.iterations > 0
        assert result.total_orders > 0
        assert result.total_positions > 0

    def test_run_ema_cross_with_tick_bar_spec(self):
        self.engine.add_strategy_from_config(
            _ema_config(self.audusd.id, "AUD/USD.SIM-100-TICK-MID-INTERNAL"),
        )

        self.engine.run()
        result = self.engine.get_result()

        assert result.iterations > 0
        assert result.total_orders > 0


class TestBacktestAcceptanceTestsETHUSDT:
    def setup_method(self):
        self.engine = _engine(snapshot_orders=True, snapshot_positions=True)
        self.venue = Venue("BINANCE")
        self.ethusdt = TestInstrumentProvider.ethusdt_binance()

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,
            starting_balances=[Money(1_000_000.0, Currency.from_str("USDT"))],
        )
        self.engine.add_instrument(self.ethusdt)

        ticks = TestDataProvider.trades_from_binance_csv(
            self.ethusdt,
            csv_name="binance/ethusdt-trades.csv",
            max_rows=10_000,
        )
        self.engine.add_data(ticks)

    def teardown_method(self):
        self.engine.dispose()

    def test_run_ema_cross_with_tick_bar_spec(self):
        self.engine.add_strategy_from_config(
            _ema_config(
                self.ethusdt.id,
                "ETHUSDT.BINANCE-250-TICK-LAST-INTERNAL",
                trade_size="100",
            ),
        )

        self.engine.run()
        result = self.engine.get_result()

        assert result.iterations > 0
        assert result.total_orders > 0


@pytest.mark.skip(
    reason="v2 missing: Betfair adapter / data provider + OrderBookImbalance strategy",
)
class TestBacktestAcceptanceTestsOrderBookImbalance:
    def test_run_order_book_imbalance(self):
        pass


@pytest.mark.skip(reason="v2 missing: Betfair adapter + MarketMaker example strategy")
class TestBacktestAcceptanceTestsMarketMaking:
    def test_run_market_maker(self):
        pass


def test_correct_account_balance_from_issue_2632():
    """
    Mirrors `test_correct_account_balance_from_issue_2632` from v1.

    https://github.com/nautechsystems/nautilus_trader/issues/2632

    """
    engine = _engine()
    venue = Venue("BINANCE")
    instrument = TestInstrumentProvider.btcusdt_perp_binance()

    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USDT"),
        starting_balances=[Money(1_000_000.0, Currency.from_str("USDT"))],
    )
    engine.add_instrument(instrument)

    bars = TestDataProvider.bars_from_binance_csv(
        instrument,
        bar_type=BarType.from_str("BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL"),
        csv_name="btc-perp-20211231-20220201_1m.csv",
        max_rows=60,
    )
    engine.add_data(bars)

    quotes: list[QuoteTick] = [
        QuoteTick(
            instrument_id=instrument.id,
            bid_price=bar.close,
            ask_price=bar.close,
            bid_size=Quantity(1.0, precision=instrument.size_precision),
            ask_size=Quantity(1.0, precision=instrument.size_precision),
            ts_event=bar.ts_event,
            ts_init=bar.ts_init,
        )
        for bar in bars
    ]
    engine.add_data(quotes)

    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path=BAR_ENTRY_EXIT_STRATEGY,
            config_path=BAR_ENTRY_EXIT_CONFIG,
            config={
                "instrument_id": str(instrument.id),
                "bar_type": "BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL",
                "trade_size": "10.000",
                "entry_bar": 0,
                "exit_bar": 10,
            },
        ),
    )

    engine.run()
    result = engine.get_result()

    assert result.iterations > 0
    assert result.total_orders == 2
    assert result.total_positions >= 1

    engine.dispose()


def _build_pnl_quotes(audusd, periods: int, scenario: str) -> list[QuoteTick]:
    base_ns = 1_577_836_800_000_000_000  # 2020-01-01T00:00:00Z
    out: list[QuoteTick] = []

    for i in range(periods):
        if scenario == "multi_cycle":
            if i < 20:
                bid = 0.70000 + (i * 0.00002)
            elif i < 40:
                bid = 0.70040 - ((i - 20) * 0.00001)
            else:
                bid = 0.70020 - ((i - 40) * 0.00002)
        elif scenario == "flips":
            if i < 40:
                bid = 0.70000 + (i * 0.00001)
            else:
                bid = 0.70040 - ((i - 40) * 0.00001)
        elif scenario == "rising":
            bid = 0.70000 + (i * 0.00001)
        else:
            raise ValueError(scenario)

        ask = bid + 0.00002
        ts = base_ns + i * 60_000_000_000  # 1-minute spacing

        out.append(
            QuoteTick(
                instrument_id=audusd.id,
                bid_price=Price(bid, precision=5),
                ask_price=Price(ask, precision=5),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=ts,
                ts_init=ts,
            ),
        )
    return out


class TestBacktestPnLAlignmentAcceptance:
    """
    Validates that PnL is consistently calculated across the system.

    The v1 suite asserts equality between trader.generate_positions_report,
    portfolio.realized_pnl, and account balance changes. v2's BacktestEngine does not
    yet expose the trader/portfolio/account APIs externally, so we assert that the
    relevant strategy ran and produced position cycles via BacktestResult.

    """

    def _build_engine(self, oms_type=OmsType.NETTING) -> tuple[BacktestEngine, object]:
        engine = _engine()
        audusd = TestInstrumentProvider.audusd_sim()
        engine.add_venue(
            venue=Venue("SIM"),
            oms_type=oms_type,
            account_type=AccountType.MARGIN,
            base_currency=Currency.from_str("USD"),
            starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
        )
        engine.add_instrument(audusd)
        return engine, audusd

    def test_pnl_alignment_multiple_position_cycles(self):
        engine, audusd = self._build_engine(oms_type=OmsType.NETTING)
        engine.add_data(_build_pnl_quotes(audusd, periods=70, scenario="multi_cycle"))

        actions = [
            [10, "BUY", "100000"],
            [20, "SELL", "100000"],
            [30, "BUY", "100000"],
            [40, "SELL", "100000"],
            [50, "SELL", "100000"],
            [60, "BUY", "100000"],
        ]
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path=TICK_SCHEDULED_STRATEGY,
                config_path=TICK_SCHEDULED_CONFIG,
                config={"instrument_id": str(audusd.id), "actions": actions},
            ),
        )

        engine.run()
        result = engine.get_result()

        assert result.iterations == 70
        assert result.total_orders == len(actions)
        assert result.total_positions >= 1
        engine.dispose()

    def test_pnl_alignment_position_flips(self):
        engine, audusd = self._build_engine(oms_type=OmsType.HEDGING)
        engine.add_data(_build_pnl_quotes(audusd, periods=100, scenario="flips"))

        actions = [
            [20, "BUY", "100000"],
            [40, "SELL", "150000"],
            [60, "BUY", "100000"],
            [80, "SELL", "50000"],
        ]
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path=TICK_SCHEDULED_STRATEGY,
                config_path=TICK_SCHEDULED_CONFIG,
                config={"instrument_id": str(audusd.id), "actions": actions},
            ),
        )

        engine.run()
        result = engine.get_result()

        assert result.iterations == 100
        assert result.total_orders == len(actions)
        engine.dispose()

    def test_backtest_postrun_pnl_alignment(self):
        """
        Mirrors GitHub issue #2856: positions report PnL == backtest post-run total PnL.

        v2 backtest result does not expose the analyzer or positions report externally,
        so we verify the engine ran the configured cycles and produced position events.

        """
        engine, audusd = self._build_engine(oms_type=OmsType.NETTING)
        engine.add_data(_build_pnl_quotes(audusd, periods=35, scenario="rising"))

        actions = [
            [10, "BUY", "100000"],
            [20, "SELL", "100000"],
            [30, "BUY", "100000"],
        ]
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path=TICK_SCHEDULED_STRATEGY,
                config_path=TICK_SCHEDULED_CONFIG,
                config={"instrument_id": str(audusd.id), "actions": actions},
            ),
        )

        engine.run()
        result = engine.get_result()

        assert result.iterations == 35
        assert result.total_orders == len(actions)
        engine.dispose()


def _build_audusd_engine_with_quotes(periods: int = 3, oms_type=OmsType.HEDGING):
    engine = _engine()
    audusd = TestInstrumentProvider.audusd_sim()
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=oms_type,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USD"),
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
    )
    engine.add_instrument(audusd)

    base_ns = 1_577_836_800_000_000_000
    quotes: list[QuoteTick] = []

    for i in range(periods):
        ts = base_ns + i * 60_000_000_000
        bid = 0.70000 + i * 0.00001
        quotes.append(
            QuoteTick(
                instrument_id=audusd.id,
                bid_price=Price(bid, precision=5),
                ask_price=Price(bid + 0.00002, precision=5),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=ts,
                ts_init=ts,
            ),
        )
    engine.add_data(quotes)
    return engine, audusd


class TestBacktestCommandSettling:
    def test_cascading_stop_loss_on_fill_processed_same_tick(self):
        engine, audusd = _build_audusd_engine_with_quotes(periods=3)

        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path=CASCADING_STOP_STRATEGY,
                config_path=CASCADING_STOP_CONFIG,
                config={
                    "instrument_id": str(audusd.id),
                    "trade_size": "100000",
                    "stop_price": "0.69950",
                },
            ),
        )

        engine.run()
        result = engine.get_result()

        # Entry market + cascading stop-market = 2 orders. Stop must be accepted at
        # the same tick as the entry fill (not stranded until the next data point).
        assert result.total_orders == 2
        engine.dispose()

    def test_multi_level_cascade_all_settled_same_tick(self):
        engine, audusd = _build_audusd_engine_with_quotes(periods=3)

        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path=MULTI_CASCADE_STRATEGY,
                config_path=MULTI_CASCADE_CONFIG,
                config={
                    "instrument_id": str(audusd.id),
                    "trade_size": "100000",
                    "stop_price": "0.69950",
                    "limit_price": "0.70100",
                },
            ),
        )

        engine.run()
        result = engine.get_result()

        # Entry market + stop-market + limit = 3 orders, all on the same tick
        assert result.total_orders == 3
        engine.dispose()

    def test_all_same_timestamp_timer_commands_settled(self):
        engine, audusd = _build_audusd_engine_with_quotes(periods=3)

        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path=DUAL_TIMER_STRATEGY,
                config_path=DUAL_TIMER_CONFIG,
                config={
                    "instrument_id": str(audusd.id),
                    "trade_size": "100000",
                    "alert_iso": "2020-01-01T00:00:30+00:00",
                },
            ),
        )

        engine.run()
        result = engine.get_result()

        # Two timers fire on the same timestamp → two market orders submitted.
        assert result.total_orders == 2
        engine.dispose()


@pytest.mark.skip(
    reason="v2 missing: databento data_utils + options/spreads + StreamingConfig + DataCatalogConfig wiring",
)
class TestBacktestNodeWithBacktestDataIterator:
    def test_backtest_same_with_and_without_data_configs(self):
        pass

    def test_spread_execution_functionality(self):
        pass

    def test_spread_quote_bars_values(self):
        pass

    def test_create_bars_with_fills_basic(self):
        pass

    def test_create_tearsheet_with_bars_with_fills(self):
        pass


@pytest.fixture
def usdjpy_engine_synthetic():
    engine = _engine()
    venue = Venue("SIM")
    usdjpy = TestInstrumentProvider.usdjpy_sim()

    engine.add_venue(
        venue=venue,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USD"),
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
    )
    engine.add_instrument(usdjpy)
    engine.add_data(TestDataProvider.usdjpy_quotes())
    yield engine, usdjpy
    engine.dispose()


def test_synthetic_run_ema_cross_strategy(usdjpy_engine_synthetic):
    engine, usdjpy = usdjpy_engine_synthetic
    engine.add_strategy_from_config(
        _ema_config(usdjpy.id, "USD/JPY.SIM-1-MINUTE-BID-INTERNAL", trade_size="100000"),
    )
    engine.run()
    result = engine.get_result()

    assert result.iterations > 0
    assert result.total_orders > 0
    assert result.total_positions > 0
    assert result.total_events > 0


def test_synthetic_run_with_synthetic_trades():
    engine = _engine()
    ethusdt = TestInstrumentProvider.ethusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        starting_balances=[
            Money(10.0, Currency.from_str("ETH")),
            Money(10_000_000.0, Currency.from_str("USDT")),
        ],
    )
    engine.add_instrument(ethusdt)

    base_ns = 1_546_383_600_000_000_000
    ticks: list[TradeTick] = []

    for i in range(5_000):
        ts = base_ns + i * 500_000_000
        price = 1500.00 + 50.0 * math.sin(i / 200.0)
        ticks.append(
            TradeTick(
                instrument_id=ethusdt.id,
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
        _ema_config(
            ethusdt.id,
            "ETHUSDT.BINANCE-100-TICK-LAST-INTERNAL",
            trade_size="0.10000",
        ),
    )

    engine.run()
    result = engine.get_result()

    assert result.iterations == len(ticks)
    assert result.total_events > 0
    engine.dispose()


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


def test_engine_run_empty_produces_zero_iterations():
    engine = _engine()
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
        base_currency=Currency.from_str("USD"),
    )
    engine.run()
    assert engine.get_result().iterations == 0
    engine.dispose()


def test_engine_reset_allows_rerun():
    engine = _engine()
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
    assert engine.get_result().iterations == 0
    engine.dispose()


def test_engine_cache_shares_kernel_state():
    """
    The ``BacktestEngine.cache`` getter must return a wrapper backed by the kernel's own
    cache (not a fresh detached one).

    A regression that constructs
    a new ``Cache`` per call would silently break parity assertions in the
    rerun acceptance test.

    """
    engine = _engine()
    venue = Venue("SIM")
    instrument = TestInstrumentProvider.audusd_sim()
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
        base_currency=Currency.from_str("USD"),
    )
    engine.add_instrument(instrument)

    cache_a = engine.cache
    cache_b = engine.cache

    # Both wrappers see the instrument written into the kernel cache by
    # `add_instrument`. A detached/fresh cache would return None here.
    assert cache_a.instrument(instrument.id) is not None
    assert cache_b.instrument(instrument.id) is not None
    engine.dispose()


def test_two_venues_with_separate_instruments():
    engine = _engine()
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
