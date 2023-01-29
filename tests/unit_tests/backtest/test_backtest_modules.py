# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.data.providers import TestDataProvider
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.modules import FXRolloverInterestConfig
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.backtest.modules import SimulationModule
from nautilus_trader.backtest.modules import SimulationModuleConfig
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders.base import Order


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestSimulationModules:
    def setup(self):
        self.instrument = USDJPY_SIM

    def create_engine(self, modules: list):
        engine = BacktestEngine()
        engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=modules,
        )
        wrangler = QuoteTickDataWrangler(self.instrument)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv")[:10],
            ask_data=provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv")[:10],
        )
        config = EMACrossConfig(
            instrument_id=self.instrument.id.value,
            bar_type="USD/JPY.SIM-1-MINUTE-MID-INTERNAL",
            fast_ema_period=1,
            slow_ema_period=2,
            trade_size=Decimal(1_000_000),
        )
        strategy = EMACross(config)
        engine.add_instrument(self.instrument)
        engine.add_data(ticks)
        engine.add_strategy(strategy)
        return engine

    def test_fx_rollover_interest_module(self):
        # Arrange
        config = FXRolloverInterestConfig(pd.DataFrame(columns=["LOCATION"]))
        module = FXRolloverInterestModule(config)
        engine = self.create_engine(modules=[module])

        # Act, Assert
        [venue] = engine.list_venues()
        assert venue

    def test_python_module(self):
        # Arrange
        instrument = self.instrument

        class PythonModule(SimulationModule):
            def process(self, now_ns: int):
                assert self.exchange
                self.now = now_ns

                if self.cache is not None:
                    orders = self.cache.orders()
                    if orders:
                        self.manually_fill_order(order=orders[0])

            def log_diagnostics(self, log: LoggerAdapter):
                pass

            def manually_fill_order(self, order: Order):
                client = self.exec_client
                client.generate_order_filled(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    venue_position_id=None,
                    trade_id=TradeId("1"),
                    order_side=order.side,
                    order_type=order.order_type,
                    last_qty=order.quantity,
                    last_px=Price(100, instrument.price_precision),
                    quote_currency=instrument.quote_currency,
                    commission=Money.from_str(f"0 {instrument.quote_currency}"),
                    liquidity_side=LiquiditySide.TAKER,
                    ts_event=self.now,
                )

        config = SimulationModuleConfig()
        engine = self.create_engine(modules=[PythonModule(config)])

        # Act
        engine.run()
