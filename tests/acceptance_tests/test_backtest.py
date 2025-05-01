# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

import pandas as pd

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.databento.data_utils import databento_data
from nautilus_trader.adapters.databento.data_utils import load_catalog
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.engine import ExecEngineConfig
from nautilus_trader.backtest.engine import RiskEngineConfig
from nautilus_trader.backtest.modules import FXRolloverInterestConfig
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import StreamingConfig
from nautilus_trader.core.datetime import unix_nanos_to_iso8601
from nautilus_trader.examples.algorithms.twap import TWAPExecAlgorithm
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.examples.strategies.ema_cross_stop_entry import EMACrossStopEntry
from nautilus_trader.examples.strategies.ema_cross_stop_entry import EMACrossStopEntryConfig
from nautilus_trader.examples.strategies.ema_cross_trailing_stop import EMACrossTrailingStop
from nautilus_trader.examples.strategies.ema_cross_trailing_stop import EMACrossTrailingStopConfig
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAP
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAPConfig
from nautilus_trader.examples.strategies.market_maker import MarketMaker
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.greeks_data import GreeksData
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.types import CatalogWriteMode
from nautilus_trader.persistence.config import DataCatalogConfig
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.mocks.data import setup_catalog
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.trading.strategy import Strategy
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider


class TestBacktestAcceptanceTestsUSDJPY:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)

        self.venue = Venue("SIM")
        interest_rate_data = pd.read_csv(TEST_DATA_DIR / "short-term-interest.csv")
        config = FXRolloverInterestConfig(interest_rate_data)
        fx_rollover_interest = FXRolloverInterestModule(config)

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=[fx_rollover_interest],
        )

        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        # Set up data
        wrangler = QuoteTickDataWrangler(instrument=self.usdjpy)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv"),
            ask_data=provider.read_csv_bars("fxcm/usdjpy-m1-ask-2013.csv"),
        )
        self.engine.add_instrument(self.usdjpy)
        self.engine.add_data(ticks)

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_strategy(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=self.usdjpy.id,
            bar_type=BarType.from_str("USD/JPY.SIM-15-MINUTE-BID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert - Should return expected PnL
        assert strategy.fast_ema.count == 2689
        assert self.engine.iteration == 115_044
        assert self.engine.portfolio.account(self.venue).balance_total(USD) == Money(
            996_498.80,
            USD,
        )

    def test_rerun_ema_cross_strategy_returns_identical_performance(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=self.usdjpy.id,
            bar_type=BarType.from_str("USD/JPY.SIM-15-MINUTE-BID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        self.engine.run()
        result1 = self.engine.portfolio.analyzer.get_performance_stats_pnls()

        # Act
        self.engine.reset()
        self.engine.run()
        result2 = self.engine.portfolio.analyzer.get_performance_stats_pnls()

        # Assert
        assert all(result2) == all(result1)

    def test_run_multiple_strategies(self):
        # Arrange
        config1 = EMACrossConfig(
            instrument_id=self.usdjpy.id,
            bar_type=BarType.from_str("USD/JPY.SIM-15-MINUTE-BID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
            order_id_tag="001",
        )
        strategy1 = EMACross(config=config1)

        config2 = EMACrossConfig(
            instrument_id=self.usdjpy.id,
            bar_type=BarType.from_str("USD/JPY.SIM-15-MINUTE-BID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=20,
            slow_ema_period=40,
            order_id_tag="002",
        )
        strategy2 = EMACross(config=config2)

        # Note since these strategies are operating on the same instrument_id as per
        # the EMACross BUY/SELL logic they will be closing each others positions.
        # The purpose of the test is just to ensure multiple strategies can run together.
        self.engine.add_strategies(strategies=[strategy1, strategy2])

        # Act
        self.engine.run()

        # Assert
        assert strategy1.fast_ema.count == 2689
        assert strategy2.fast_ema.count == 2689
        assert self.engine.iteration == 115_044
        assert self.engine.portfolio.account(self.venue).balance_total(USD) == Money(
            974_269.71,
            USD,
        )


class TestBacktestAcceptanceTestsGBPUSDBarsInternal:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)
        self.venue = Venue("SIM")

        interest_rate_data = pd.read_csv(TEST_DATA_DIR / "short-term-interest.csv")
        config = FXRolloverInterestConfig(interest_rate_data)
        fx_rollover_interest = FXRolloverInterestModule(config)

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=GBP,
            starting_balances=[Money(1_000_000, GBP)],
            modules=[fx_rollover_interest],
        )

        self.gbpusd = TestInstrumentProvider.default_fx_ccy("GBP/USD")

        # Set up data
        wrangler = QuoteTickDataWrangler(self.gbpusd)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm/gbpusd-m1-bid-2012.csv"),
            ask_data=provider.read_csv_bars("fxcm/gbpusd-m1-ask-2012.csv"),
        )
        self.engine.add_instrument(self.gbpusd)
        self.engine.add_data(ticks)

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_five_minute_bar_spec(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=self.gbpusd.id,
            bar_type=BarType.from_str("GBP/USD.SIM-5-MINUTE-MID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 8353
        assert self.engine.iteration == 120_468
        assert self.engine.portfolio.account(self.venue).balance_total(GBP) == Money(
            987_607.44,
            GBP,
        )

    def test_run_ema_cross_stop_entry_trail_strategy(self):
        # Arrange
        config = EMACrossStopEntryConfig(
            instrument_id=self.gbpusd.id,
            bar_type=BarType.from_str("GBP/USD.SIM-5-MINUTE-BID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
            atr_period=20,
            trailing_atr_multiple=3.0,
            trailing_offset_type="PRICE",
            trailing_offset=Decimal("0.01"),
            trigger_type="LAST_PRICE",
        )
        strategy = EMACrossStopEntry(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert - Should return expected PnL
        assert strategy.fast_ema.count == 8_353
        assert self.engine.iteration == 120_468
        assert self.engine.portfolio.account(self.venue).balance_total(GBP) == Money(
            998_967.44,
            GBP,
        )

    def test_run_ema_cross_stop_entry_trail_strategy_with_emulation(self):
        # Arrange
        config = EMACrossTrailingStopConfig(
            instrument_id=self.gbpusd.id,
            bar_type=BarType.from_str("GBP/USD.SIM-1-MINUTE-BID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
            atr_period=20,
            trailing_atr_multiple=2.0,
            trailing_offset_type="PRICE",
            trigger_type="BID_ASK",
            emulation_trigger="BID_ASK",
        )
        strategy = EMACrossTrailingStop(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert - Should return expected PnL
        assert strategy.fast_ema.count == 41761
        assert self.engine.iteration == 120_468
        assert self.engine.portfolio.account(self.venue).balance_total(GBP) == Money(
            850_143.71,
            GBP,
        )


class TestBacktestAcceptanceTestsGBPUSDBarsExternal:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
            risk_engine=RiskEngineConfig(
                bypass=True,  # Example of bypassing pre-trade risk checks for backtests
                max_notional_per_order={"GBP/USD.SIM": 2_000_000},
            ),
        )
        self.engine = BacktestEngine(config=config)
        self.venue = Venue("SIM")

        interest_rate_data = pd.read_csv(TEST_DATA_DIR / "short-term-interest.csv")
        config = FXRolloverInterestConfig(interest_rate_data)
        fx_rollover_interest = FXRolloverInterestModule(config)

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=[fx_rollover_interest],
        )

        self.gbpusd = TestInstrumentProvider.default_fx_ccy("GBP/USD")

        # Set up wranglers
        bid_wrangler = BarDataWrangler(
            bar_type=BarType.from_str("GBP/USD.SIM-1-MINUTE-BID-EXTERNAL"),
            instrument=self.gbpusd,
        )
        ask_wrangler = BarDataWrangler(
            bar_type=BarType.from_str("GBP/USD.SIM-1-MINUTE-ASK-EXTERNAL"),
            instrument=self.gbpusd,
        )

        # Set up data
        provider = TestDataProvider()

        # Build externally aggregated bars
        bid_bars = bid_wrangler.process(
            data=provider.read_csv_bars("fxcm/gbpusd-m1-bid-2012.csv"),
        )
        ask_bars = ask_wrangler.process(
            data=provider.read_csv_bars("fxcm/gbpusd-m1-ask-2012.csv"),
        )

        self.engine.add_instrument(self.gbpusd)
        self.engine.add_data(bid_bars)
        self.engine.add_data(ask_bars)

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=self.gbpusd.id,
            bar_type=BarType.from_str("GBP/USD.SIM-1-MINUTE-BID-EXTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 30_117
        assert self.engine.iteration == 60_234
        ending_balance = self.engine.portfolio.account(self.venue).balance_total(USD)
        assert ending_balance == Money(953_220.61, USD)


class TestBacktestAcceptanceTestsBTCUSDTEmaCrossTWAP:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            run_analysis=False,
            logging=LoggingConfig(bypass_logging=True),
            exec_engine=ExecEngineConfig(),
            risk_engine=RiskEngineConfig(bypass=True),
        )
        self.engine = BacktestEngine(
            config=config,
        )
        self.venue = Venue("BINANCE")

        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.CASH,  # <-- Spot exchange
            starting_balances=[Money(10, BTC), Money(10_000_000, USDT)],
            base_currency=None,
        )

        self.btcusdt = TestInstrumentProvider.btcusdt_binance()
        self.engine.add_instrument(self.btcusdt)

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_trade_bars(self):
        # Arrange
        wrangler = BarDataWrangler(
            bar_type=BarType.from_str("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            instrument=self.btcusdt,
        )

        provider = TestDataProvider()

        # Build externally aggregated bars
        bars = wrangler.process(
            data=provider.read_csv_bars("btc-perp-20211231-20220201_1m.csv")[:10_000],
        )

        self.engine.add_data(bars)

        config = EMACrossTWAPConfig(
            instrument_id=self.btcusdt.id,
            bar_type=BarType.from_str("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            trade_size=Decimal("0.01"),
            fast_ema_period=10,
            slow_ema_period=20,
            twap_horizon_secs=10.0,
            twap_interval_secs=2.5,
        )
        strategy = EMACrossTWAP(config=config)
        self.engine.add_strategy(strategy)

        exec_algorithm = TWAPExecAlgorithm()
        self.engine.add_exec_algorithm(exec_algorithm)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 10_000
        assert self.engine.iteration == 10_000
        btc_ending_balance = self.engine.portfolio.account(self.venue).balance_total(BTC)
        usdt_ending_balance = self.engine.portfolio.account(self.venue).balance_total(USDT)
        assert btc_ending_balance == Money(10.00000000, BTC)
        assert usdt_ending_balance == Money(9_999_549.43133000, USDT)

    def test_run_ema_cross_with_trade_ticks_from_bar_data(self):
        # Arrange
        wrangler = QuoteTickDataWrangler(instrument=self.btcusdt)

        provider = TestDataProvider()

        # Build ticks from bar data
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("btc-perp-20211231-20220201_1m.csv")[:10_000],
            ask_data=provider.read_csv_bars("btc-perp-20211231-20220201_1m.csv")[:10_000],
        )

        self.engine.add_data(ticks)

        config = EMACrossConfig(
            instrument_id=self.btcusdt.id,
            bar_type=BarType.from_str("BTCUSDT.BINANCE-1-MINUTE-BID-INTERNAL"),
            trade_size=Decimal("0.001"),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert len(ticks) == 40_000
        assert strategy.fast_ema.count == 10_000
        assert self.engine.iteration == 40_000
        btc_ending_balance = self.engine.portfolio.account(self.venue).balance_total(BTC)
        usdt_ending_balance = self.engine.portfolio.account(self.venue).balance_total(USDT)
        assert btc_ending_balance == Money(10.00000000, BTC)
        assert usdt_ending_balance == Money(9_999_954.94313300, USDT)


class TestBacktestAcceptanceTestsAUDUSD:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)
        self.venue = Venue("SIM")

        # Set up venue
        provider = TestDataProvider()
        interest_rate_data = provider.read_csv("short-term-interest.csv")
        config = FXRolloverInterestConfig(interest_rate_data)
        fx_rollover_interest = FXRolloverInterestModule(config)

        self.engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=AUD,
            starting_balances=[Money(1_000_000, AUD)],
            modules=[fx_rollover_interest],
        )

        # Set up data
        self.audusd = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        wrangler = QuoteTickDataWrangler(self.audusd)
        ticks = wrangler.process(provider.read_csv_ticks("truefx/audusd-ticks.csv"))
        self.engine.add_instrument(self.audusd)
        self.engine.add_data(ticks)

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
            bar_type=BarType.from_str("AUD/USD.SIM-1-MINUTE-MID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 1771
        assert self.engine.iteration == 100_000
        assert self.engine.portfolio.account(self.venue).balance_total(AUD) == Money(
            996_601.43,
            AUD,
        )

    def test_run_ema_cross_with_tick_bar_spec(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=self.audusd.id,
            bar_type=BarType.from_str("AUD/USD.SIM-100-TICK-MID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 1_000
        assert self.engine.iteration == 100_000
        assert self.engine.portfolio.account(self.venue).balance_total(AUD) == Money(
            998_121.60,
            AUD,
        )


class TestBacktestAcceptanceTestsETHUSDT:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)
        self.venue = Venue("BINANCE")

        # Set up venue
        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-currency account
            starting_balances=[Money(1_000_000, USDT)],
        )

        # Add instruments
        self.ethusdt = TestInstrumentProvider.ethusdt_binance()
        self.engine.add_instrument(self.ethusdt)

        # Add data
        provider = TestDataProvider()
        wrangler = TradeTickDataWrangler(instrument=self.ethusdt)
        ticks = wrangler.process(provider.read_csv_ticks("binance/ethusdt-trades.csv"))
        self.engine.add_data(ticks)

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_tick_bar_spec(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=self.ethusdt.id,
            bar_type=BarType.from_str("ETHUSDT.BINANCE-250-TICK-LAST-INTERNAL"),
            trade_size=Decimal(100),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 279
        assert self.engine.iteration == 69_806
        expected_commission = Money(127.56763570, USDT)
        expected_usdt = Money(999_764.32147162, USDT)
        assert self.engine.portfolio.account(self.venue).commission(USDT) == expected_commission
        assert self.engine.portfolio.account(self.venue).balance_total(USDT) == expected_usdt


class TestBacktestAcceptanceTestsOrderBookImbalance:
    def setup(self):
        # Fixture Setup
        setup_catalog(protocol="memory")

        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)
        self.venue = Venue("BETFAIR")

        # Set up venue
        self.engine.add_venue(
            venue=self.venue,
            account_type=AccountType.MARGIN,
            base_currency=None,
            oms_type=OmsType.NETTING,
            starting_balances=[Money(100_000, GBP)],
            book_type=BookType.L2_MBP,
        )

        # Set up data
        data = BetfairDataProvider.betfair_feed_parsed(market_id="1-166811431")
        instruments = [d for d in data if isinstance(d, BettingInstrument)]
        assert instruments

        for instrument in instruments[:1]:
            trade_ticks = [
                d for d in data if isinstance(d, TradeTick) and d.instrument_id == instrument.id
            ]
            order_book_deltas = [
                d
                for d in data
                if isinstance(d, OrderBookDelta | OrderBookDeltas)
                and d.instrument_id == instrument.id
            ]
            self.engine.add_instrument(instrument)
            self.engine.add_data(trade_ticks)
            self.engine.add_data(order_book_deltas)
            self.instrument = instrument

    def teardown(self):
        self.engine.dispose()

    def test_run_order_book_imbalance(self):
        # Arrange
        config = OrderBookImbalanceConfig(
            instrument_id=self.instrument.id,
            max_trade_size=Decimal(20),
        )
        strategy = OrderBookImbalance(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert self.engine.iteration in (8198, 7812)


class TestBacktestAcceptanceTestsMarketMaking:
    def setup(self):
        # Fixture Setup
        setup_catalog(protocol="memory")

        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)
        self.venue = Venue("BETFAIR")

        self.engine.add_venue(
            venue=self.venue,
            account_type=AccountType.MARGIN,
            base_currency=None,
            oms_type=OmsType.NETTING,
            starting_balances=[Money(10_000, GBP)],
            book_type=BookType.L2_MBP,
        )

        data = BetfairDataProvider.betfair_feed_parsed(market_id="1-166811431")
        instruments = [d for d in data if isinstance(d, BettingInstrument)]

        for instrument in instruments[:1]:
            trade_ticks = [
                d for d in data if isinstance(d, TradeTick) and d.instrument_id == instrument.id
            ]
            order_book_deltas = [
                d
                for d in data
                if isinstance(d, OrderBookDelta | OrderBookDeltas)
                and d.instrument_id == instrument.id
            ]
            self.engine.add_instrument(instrument)
            self.engine.add_data(trade_ticks)
            self.engine.add_data(order_book_deltas)
            self.instrument = instrument

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
        assert self.engine.iteration == 4216
        assert self.engine.portfolio.account(self.venue).balance_total(GBP) == Money(927.19, GBP)


class TestBacktestNodeWithBacktestDataIterator:
    def test_backtest_same_with_and_without_data_configs(self) -> None:
        # Arrange
        messages_with_data: list = []
        messages_without_data: list = []

        # Act
        run_backtest(messages_with_data.append, with_data=True)
        run_backtest(messages_without_data.append, with_data=False)

        assert messages_with_data == messages_without_data


def run_backtest(test_callback=None, with_data=True, log_path=None):
    catalog_folder = "options_catalog"
    catalog = load_catalog(catalog_folder)

    future_symbols = ["ESM4"]
    option_symbols = ["ESM4 P5230", "ESM4 P5250"]

    start_time = "2024-05-09T10:00"
    end_time = "2024-05-09T10:05"

    _ = databento_data(
        future_symbols,
        start_time,
        end_time,
        "ohlcv-1m",
        "futures",
        catalog_folder,
    )
    _ = databento_data(
        option_symbols,
        start_time,
        end_time,
        "bbo-1m",
        "options",
        catalog_folder,
    )

    # for saving and loading custom data greeks, use True, False then False, True below
    stream_data, load_greeks = False, False

    # actors = [
    #     ImportableActorConfig(
    #         actor_path=InterestRateProvider.fully_qualified_name(),
    #         config_path=InterestRateProviderConfig.fully_qualified_name(),
    #         config={
    #             "interest_rates_file": str(
    #                 data_path(catalog_folder, "usd_short_term_rate.xml"),
    #             ),
    #         },
    #     ),
    # ]

    strategies = [
        ImportableStrategyConfig(
            strategy_path=OptionStrategy.fully_qualified_name(),
            config_path=OptionConfig.fully_qualified_name(),
            config={
                "future_id": InstrumentId.from_str(f"{future_symbols[0]}.GLBX"),
                "option_id": InstrumentId.from_str(f"{option_symbols[0]}.GLBX"),
                "option_id2": InstrumentId.from_str(f"{option_symbols[1]}.GLBX"),
                "load_greeks": load_greeks,
            },
        ),
    ]

    streaming = StreamingConfig(
        catalog_path=catalog.path,
        fs_protocol="file",
        include_types=[GreeksData],
    )

    logging = LoggingConfig(
        bypass_logging=False,
        log_colors=True,
        log_level="WARN",
        log_level_file="WARN",
        log_directory=log_path,  # must be the same as conftest.py
        log_file_format=None,  # "json" or None
        log_file_name="test_logs",  # must be the same as conftest.py
        clear_log_file=True,
        print_config=False,
        use_pyo3=False,
    )

    catalogs = [
        DataCatalogConfig(
            path=catalog.path,
        ),
    ]

    engine_config = BacktestEngineConfig(
        logging=logging,
        # actors=actors,
        strategies=strategies,
        streaming=(streaming if stream_data else None),
        catalogs=catalogs,
    )

    if with_data:
        data = [
            BacktestDataConfig(
                data_cls=QuoteTick,
                catalog_path=catalog.path,
                instrument_id=InstrumentId.from_str(f"{option_symbols[0]}.GLBX"),
            ),
            BacktestDataConfig(
                data_cls=QuoteTick,
                catalog_path=catalog.path,
                instrument_id=InstrumentId.from_str(f"{option_symbols[1]}.GLBX"),
            ),
            BacktestDataConfig(
                data_cls=Bar,
                catalog_path=catalog.path,
                instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.GLBX"),
                bar_spec="1-MINUTE-LAST",
            ),
        ]
    else:
        data = []

    if load_greeks:
        data = [
            BacktestDataConfig(
                data_cls=GreeksData.fully_qualified_name(),
                catalog_path=catalog.path,
                client_id="GreeksDataProvider",
                metadata={"instrument_id": "ES"},
            ),
            *data,
        ]

    venues = [
        BacktestVenueConfig(
            name="GLBX",
            oms_type="NETTING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1_000_000 USD"],
        ),
    ]

    configs = [
        BacktestRunConfig(
            engine=engine_config,
            data=data,
            venues=venues,
            chunk_size=None,  # use None when loading custom data, else a value of 10_000 for example
            start=start_time,
            end=end_time,
        ),
    ]

    node = BacktestNode(configs=configs)
    node.build()

    if test_callback:
        node.get_engine(configs[0].id).kernel.msgbus.subscribe("test", test_callback)

    results = node.run()

    if stream_data:
        catalog.convert_stream_to_data(
            results[0].instance_id,
            GreeksData,
            mode=CatalogWriteMode.NEWFILE,
        )

    engine: BacktestEngine = node.get_engine(configs[0].id)
    engine.trader.generate_order_fills_report()
    engine.trader.generate_positions_report()
    engine.trader.generate_account_report(Venue("GLBX"))
    node.dispose()


class OptionConfig(StrategyConfig, frozen=True):
    future_id: InstrumentId
    option_id: InstrumentId
    option_id2: InstrumentId
    load_greeks: bool = False


class OptionStrategy(Strategy):
    def __init__(self, config: OptionConfig):
        super().__init__(config=config)
        self.start_orders_done = False

    def on_start(self):
        self.bar_type = BarType.from_str(f"{self.config.future_id}-1-MINUTE-LAST-EXTERNAL")

        self.request_instrument(self.config.option_id)
        self.request_instrument(self.config.option_id2)
        self.request_instrument(self.bar_type.instrument_id)

        self.subscribe_quote_ticks(self.config.option_id2)
        self.subscribe_quote_ticks(
            self.config.option_id,
            params={
                "duration_seconds": pd.Timedelta(minutes=1).seconds,
                "append_data": False,
            },
        )
        self.subscribe_bars(self.bar_type)

        if self.config.load_greeks:
            self.greeks.subscribe_greeks("ES")

    def on_quote_tick(self, data):
        self.user_log(data)

    def init_portfolio(self):
        self.submit_market_order(instrument_id=self.config.option_id, quantity=-10)
        self.submit_market_order(instrument_id=self.config.option_id2, quantity=10)
        self.submit_market_order(instrument_id=self.config.future_id, quantity=1)

        self.start_orders_done = True

    # def on_bar(self, data):
    #     self.user_log(data)

    def on_bar(self, bar):
        self.user_log(
            f"bar ts_init = {unix_nanos_to_iso8601(bar.ts_init)}, bar close = {bar.close}",
        )

        if not self.start_orders_done:
            self.user_log("Initializing the portfolio with some trades")
            self.init_portfolio()
            return

        self.display_greeks()

    def display_greeks(self, alert=None):
        portfolio_greeks = self.greeks.portfolio_greeks(
            use_cached_greeks=self.config.load_greeks,
            publish_greeks=(not self.config.load_greeks),
        )
        self.user_log(f"{portfolio_greeks=}")

    def submit_market_order(self, instrument_id, quantity):
        order = self.order_factory.market(
            instrument_id=instrument_id,
            order_side=(OrderSide.BUY if quantity > 0 else OrderSide.SELL),
            quantity=Quantity.from_int(abs(quantity)),
        )

        self.submit_order(order)

    def submit_limit_order(self, instrument_id, price, quantity):
        order = self.order_factory.limit(
            instrument_id=instrument_id,
            order_side=(OrderSide.BUY if quantity > 0 else OrderSide.SELL),
            quantity=Quantity.from_int(abs(quantity)),
            price=Price(price),
        )

        self.submit_order(order)

    def user_log(self, msg):
        self.log.warning(str(msg), color=LogColor.GREEN)
        self.msgbus.publish(topic="test", msg=str(msg))

    def on_stop(self):
        self.unsubscribe_bars(self.bar_type)
