# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import os
from decimal import Decimal

import pandas as pd
import pytest

from nautilus_trader.backtest.data.providers import TestDataProvider
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data.wranglers import BarDataWrangler
from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler
from nautilus_trader.backtest.data.wranglers import TradeTickDataWrangler
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.examples.strategies.market_maker import MarketMaker
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.data import OrderBookData
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks import data_catalog_setup


class TestBacktestAcceptanceTestsUSDJPY:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)

        self.venue = Venue("SIM")
        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        # Setup data
        wrangler = QuoteTickDataWrangler(instrument=self.usdjpy)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv"),
            ask_data=provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv"),
        )
        self.engine.add_instrument(self.usdjpy)
        self.engine.add_ticks(ticks)

        interest_rate_data = pd.read_csv(
            os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv")
        )
        fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=[fx_rollover_interest],
        )

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_strategy(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=str(self.usdjpy.id),
            bar_type="USD/JPY.SIM-15-MINUTE-BID-INTERNAL",
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert - Should return expected PnL
        assert strategy.fast_ema.count == 2689
        assert self.engine.iteration == 115044
        assert self.engine.portfolio.account(self.venue).balance_total(USD) == Money(992811.26, USD)

    def test_rerun_ema_cross_strategy_returns_identical_performance(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=str(self.usdjpy.id),
            bar_type="USD/JPY.SIM-15-MINUTE-BID-INTERNAL",
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        self.engine.run()
        result1 = self.engine.analyzer.get_performance_stats_pnls()

        # Act
        self.engine.reset()
        self.engine.add_instrument(self.usdjpy)  # TODO(cs): Having to replace instrument
        self.engine.run()
        result2 = self.engine.analyzer.get_performance_stats_pnls()

        # Assert
        assert all(result2) == all(result1)

    def test_run_multiple_strategies(self):
        # Arrange
        config1 = EMACrossConfig(
            instrument_id=str(self.usdjpy.id),
            bar_type="USD/JPY.SIM-15-MINUTE-BID-INTERNAL",
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
            order_id_tag="001",
        )
        strategy1 = EMACross(config=config1)

        config2 = EMACrossConfig(
            instrument_id=str(self.usdjpy.id),
            bar_type="USD/JPY.SIM-15-MINUTE-BID-INTERNAL",
            trade_size=Decimal(1_000_000),
            fast_ema=20,
            slow_ema=40,
            order_id_tag="002",
        )
        strategy2 = EMACross(config=config2)

        # Note since these strategies are operating on the same instrument_id as per
        # the EMACross BUY/SELL logic they will be flattening each others positions.
        # The purpose of the test is just to ensure multiple strategies can run together.
        self.engine.add_strategies(strategies=[strategy1, strategy2])

        # Act
        self.engine.run()

        # Assert
        assert strategy1.fast_ema.count == 2689
        assert strategy2.fast_ema.count == 2689
        assert self.engine.iteration == 115044
        assert self.engine.portfolio.account(self.venue).balance_total(USD) == Money(985622.52, USD)


class TestBacktestAcceptanceTestsGBPUSDBarsInternal:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)

        self.venue = Venue("SIM")
        self.gbpusd = TestInstrumentProvider.default_fx_ccy("GBP/USD")

        # Setup data
        wrangler = QuoteTickDataWrangler(self.gbpusd)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm-gbpusd-m1-bid-2012.csv"),
            ask_data=provider.read_csv_bars("fxcm-gbpusd-m1-ask-2012.csv"),
        )
        self.engine.add_instrument(self.gbpusd)
        self.engine.add_ticks(ticks)

        interest_rate_data = pd.read_csv(
            os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv")
        )
        fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=GBP,
            starting_balances=[Money(1_000_000, GBP)],
            modules=[fx_rollover_interest],
        )

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=str(self.gbpusd.id),
            bar_type="GBP/USD.SIM-5-MINUTE-MID-INTERNAL",
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 8353
        assert self.engine.iteration == 120468
        assert self.engine.portfolio.account(self.venue).balance_total(GBP) == Money(931346.81, GBP)


class TestBacktestAcceptanceTestsGBPUSDBarsExternal:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            bypass_logging=False,
            run_analysis=False,
            risk_engine={
                "bypass": True,  # Example of bypassing pre-trade risk checks for backtests
                "max_notional_per_order": {"GBP/USD.SIM": 2_000_000},
            },
        )
        self.engine = BacktestEngine(config=config)

        self.venue = Venue("SIM")
        self.gbpusd = TestInstrumentProvider.default_fx_ccy("GBP/USD")

        # Setup wranglers
        bid_wrangler = BarDataWrangler(
            bar_type=BarType.from_str("GBP/USD.SIM-1-MINUTE-BID-EXTERNAL"),
            instrument=self.gbpusd,
        )
        ask_wrangler = BarDataWrangler(
            bar_type=BarType.from_str("GBP/USD.SIM-1-MINUTE-ASK-EXTERNAL"),
            instrument=self.gbpusd,
        )

        # Setup data
        provider = TestDataProvider()

        # Build externally aggregated bars
        bid_bars = bid_wrangler.process(
            data=provider.read_csv_bars("fxcm-gbpusd-m1-bid-2012.csv"),
        )
        ask_bars = ask_wrangler.process(
            data=provider.read_csv_bars("fxcm-gbpusd-m1-ask-2012.csv"),
        )

        self.engine.add_instrument(self.gbpusd)
        self.engine.add_bars(bid_bars)
        self.engine.add_bars(ask_bars)

        interest_rate_data = pd.read_csv(
            os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv")
        )
        fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=[fx_rollover_interest],
        )

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=str(self.gbpusd.id),
            bar_type="GBP/USD.SIM-1-MINUTE-BID-EXTERNAL",
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 30117
        assert self.engine.iteration == 60234
        ending_balance = self.engine.portfolio.account(self.venue).balance_total(USD)
        assert ending_balance == Money(1016188.45, USD)


class TestBacktestAcceptanceTestsBTCPERPTradeBars:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            bypass_logging=False,
            run_analysis=False,
            risk_engine={"bypass": True},
        )
        self.engine = BacktestEngine(config=config)

        self.venue = Venue("BINANCE")
        self.btcusdt = TestInstrumentProvider.btcusdt_binance()

        self.engine.add_instrument(self.btcusdt)
        self.engine.add_venue(
            venue=self.venue,
            oms_type=OMSType.NETTING,
            account_type=AccountType.CASH,
            base_currency=None,
            starting_balances=[Money(10, BTC), Money(10_000_000, USDT)],
        )

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_trade_bars(self):
        # Arrange
        wrangler = BarDataWrangler(
            bar_type=BarType.from_str("BTC/USDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            instrument=self.btcusdt,
        )

        provider = TestDataProvider()

        # Build externally aggregated bars
        bars = wrangler.process(
            data=provider.read_csv_bars("ftx-btc-perp-20211231-20220201_1m.csv")[:10000],
        )

        self.engine.add_bars(bars)

        config = EMACrossConfig(
            instrument_id=str(self.btcusdt.id),
            bar_type="BTC/USDT.BINANCE-1-MINUTE-LAST-EXTERNAL",
            trade_size=Decimal(0.001),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 10000
        assert self.engine.iteration == 10000
        btc_ending_balance = self.engine.portfolio.account(self.venue).balance_total(BTC)
        usdt_ending_balance = self.engine.portfolio.account(self.venue).balance_total(USDT)
        assert btc_ending_balance == Money(9.57200000, BTC)
        assert usdt_ending_balance == Money(10016993.04994300, USDT)

    def test_run_ema_cross_with_trade_ticks_from_bar_data(self):
        # Arrange
        wrangler = QuoteTickDataWrangler(instrument=self.btcusdt)

        provider = TestDataProvider()

        # Build ticks from bar data
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("ftx-btc-perp-20211231-20220201_1m.csv")[:10000],
            ask_data=provider.read_csv_bars("ftx-btc-perp-20211231-20220201_1m.csv")[:10000],
        )

        self.engine.add_ticks(ticks)

        config = EMACrossConfig(
            instrument_id=str(self.btcusdt.id),
            bar_type="BTC/USDT.BINANCE-1-MINUTE-BID-INTERNAL",
            trade_size=Decimal(0.001),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 10000
        assert self.engine.iteration == 40000
        btc_ending_balance = self.engine.portfolio.account(self.venue).balance_total(BTC)
        usdt_ending_balance = self.engine.portfolio.account(self.venue).balance_total(USDT)
        assert btc_ending_balance == Money(9.57200000, BTC)
        assert usdt_ending_balance == Money(10017114.27716700, USDT)


class TestBacktestAcceptanceTestsAUDUSD:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)

        self.venue = Venue("SIM")
        self.audusd = TestInstrumentProvider.default_fx_ccy("AUD/USD")

        # Setup data
        wrangler = QuoteTickDataWrangler(self.audusd)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("truefx-audusd-ticks.csv"))
        self.engine.add_instrument(self.audusd)
        self.engine.add_ticks(ticks)

        interest_rate_data = provider.read_csv("short-term-interest.csv")
        fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

        self.engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=AUD,
            starting_balances=[Money(1_000_000, AUD)],
            modules=[fx_rollover_interest],
        )

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id="AUD/USD.SIM",
            bar_type="AUD/USD.SIM-1-MINUTE-MID-INTERNAL",
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 1771
        assert self.engine.iteration == 100000
        assert self.engine.portfolio.account(self.venue).balance_total(AUD) == Money(987920.04, AUD)

    def test_run_ema_cross_with_tick_bar_spec(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=str(self.audusd.id),
            bar_type="AUD/USD.SIM-100-TICK-MID-INTERNAL",
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 1000
        assert self.engine.iteration == 100000
        assert self.engine.portfolio.account(self.venue).balance_total(AUD) == Money(994441.60, AUD)


class TestBacktestAcceptanceTestsETHUSDT:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)

        self.venue = Venue("BINANCE")
        self.ethusdt = TestInstrumentProvider.ethusdt_binance()

        # Setup data
        wrangler = TradeTickDataWrangler(instrument=self.ethusdt)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("binance-ethusdt-trades.csv"))
        self.engine.add_instrument(self.ethusdt)
        self.engine.add_ticks(ticks)

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OMSType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-currency account
            starting_balances=[Money(1_000_000, USDT)],
        )

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_tick_bar_spec(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=str(self.ethusdt.id),
            bar_type="ETH/USDT.BINANCE-250-TICK-LAST-INTERNAL",
            trade_size=Decimal(100),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 279
        assert self.engine.iteration == 69806
        assert self.engine.portfolio.account(self.venue).balance_total(USDT) == Money(
            977078.56596150, USDT
        )


class TestBacktestAcceptanceTestsOrderBookImbalance:
    def setup(self):
        # Fixture Setup
        data_catalog_setup()

        config = BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)

        self.venue = Venue("BETFAIR")

        data = BetfairDataProvider.betfair_feed_parsed(
            market_id="1.166811431.bz2", folder="data/betfair"
        )
        instruments = [d for d in data if isinstance(d, BettingInstrument)]

        for instrument in instruments[:1]:
            trade_ticks = [
                d for d in data if isinstance(d, TradeTick) and d.instrument_id == instrument.id
            ]
            order_book_deltas = [
                d for d in data if isinstance(d, OrderBookData) and d.instrument_id == instrument.id
            ]
            self.engine.add_instrument(instrument)
            self.engine.add_ticks(trade_ticks)
            self.engine.add_order_book_data(order_book_deltas)
            self.instrument = instrument
        self.engine.add_venue(
            venue=self.venue,
            account_type=AccountType.MARGIN,
            base_currency=None,
            oms_type=OMSType.NETTING,
            starting_balances=[Money(100_000, GBP)],
            book_type=BookType.L2_MBP,
        )

    def teardown(self):
        self.engine.dispose()

    def test_run_order_book_imbalance(self):
        # Arrange
        config = OrderBookImbalanceConfig(
            instrument_id=str(self.instrument.id),
            max_trade_size=20,
        )
        strategy = OrderBookImbalance(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert self.engine.iteration in (8199, 7812)


@pytest.mark.skip(reason="bm to fix")
class TestBacktestAcceptanceTestsMarketMaking:
    def setup(self):
        # Fixture Setup
        data_catalog_setup()

        config = BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)

        self.venue = Venue("BETFAIR")

        data = BetfairDataProvider.betfair_feed_parsed(
            market_id="1.166811431.bz2", folder="data/betfair"
        )
        instruments = [d for d in data if isinstance(d, BettingInstrument)]

        for instrument in instruments[:1]:
            trade_ticks = [
                d for d in data if isinstance(d, TradeTick) and d.instrument_id == instrument.id
            ]
            order_book_deltas = [
                d for d in data if isinstance(d, OrderBookData) and d.instrument_id == instrument.id
            ]
            self.engine.add_instrument(instrument)
            self.engine.add_ticks(trade_ticks)
            self.engine.add_order_book_data(order_book_deltas)
            self.instrument = instrument
        self.engine.add_venue(
            venue=self.venue,
            account_type=AccountType.MARGIN,
            base_currency=None,
            oms_type=OMSType.NETTING,
            starting_balances=[Money(10_000, GBP)],
            book_type=BookType.L2_MBP,
        )

    def teardown(self):
        self.engine.dispose()

    def test_run_market_maker(self):
        # Arrange
        strategy = MarketMaker(
            instrument_id=self.instrument.id,
            trade_size=Decimal(10),
            max_size=Decimal(30),
        )
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        # TODO - Unsure why this is not deterministic ?
        assert self.engine.iteration in (7812, 8199, 9319)
        assert self.engine.portfolio.account(self.venue).balance_total(GBP) == Money(
            "10000.00", GBP
        )
