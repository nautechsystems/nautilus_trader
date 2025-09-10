#!/usr/bin/env python3
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

import asyncio
import time
from decimal import Decimal

import pandas as pd

from nautilus_trader.adapters.binance import BINANCE_VENUE
from nautilus_trader.adapters.binance import get_cached_binance_http_client
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.component import LiveClock
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.ema_cross_trailing_stop import EMACrossTrailingStop
from nautilus_trader.examples.strategies.ema_cross_trailing_stop import EMACrossTrailingStopConfig
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider


async def create_provider():
    """
    Create a provider to load all instrument data from live exchange.
    """
    clock = LiveClock()

    client = get_cached_binance_http_client(
        clock=clock,
        account_type=BinanceAccountType.USDT_FUTURES,
        is_testnet=True,
    )

    binance_provider = BinanceFuturesInstrumentProvider(
        client=client,
        clock=clock,
        config=InstrumentProviderConfig(load_all=True, log_warnings=False),
    )

    await binance_provider.load_all_async()
    return binance_provider


if __name__ == "__main__":
    # Configure backtest engine
    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
        logging=LoggingConfig(log_level="INFO"),
    )

    # Build the backtest engine
    engine = BacktestEngine(config=config)

    # Add a trading venue (multiple venues possible)
    # Use actual Binance instrument for backtesting
    provider: BinanceFuturesInstrumentProvider = asyncio.run(create_provider())

    instrument_id = InstrumentId(symbol=Symbol("ETHUSDT-PERP"), venue=BINANCE_VENUE)
    instrument = provider.find(instrument_id)
    if instrument is None:
        raise RuntimeError(f"Unable to find instrument {instrument_id}")

    engine.add_venue(
        venue=BINANCE_VENUE,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=None,
        starting_balances=[Money(1_000_000, instrument.quote_currency)],
    )

    engine.add_instrument(instrument)

    bar_type = BarType.from_str(f"{instrument_id.value}-1-MINUTE-BID-INTERNAL")
    wrangler = QuoteTickDataWrangler(instrument=instrument)
    ticks = wrangler.process_bar_data(
        bid_data=TestDataProvider().read_csv_bars("btc-perp-20211231-20220201_1m.csv"),
        ask_data=TestDataProvider().read_csv_bars("btc-perp-20211231-20220201_1m.csv"),
    )

    engine.add_data(ticks)

    # Configure your strategy
    strategy_config = EMACrossTrailingStopConfig(
        instrument_id=instrument.id,
        bar_type=bar_type,
        trade_size=Decimal("1"),
        fast_ema_period=10,
        slow_ema_period=20,
        atr_period=20,
        trailing_atr_multiple=3.0,
        trailing_offset_type="PRICE",
        trigger_type="LAST_PRICE",
    )
    # Instantiate and add your strategy
    strategy = EMACrossTrailingStop(config=strategy_config)
    engine.add_strategy(strategy=strategy)

    time.sleep(0.1)
    input("Press Enter to continue...")

    # Run the engine (from start to end of data)
    engine.run()

    # Optionally view reports
    with pd.option_context(
        "display.max_rows",
        100,
        "display.max_columns",
        None,
        "display.width",
        300,
    ):
        print(engine.trader.generate_account_report(BINANCE_VENUE))
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    # For repeated backtest runs make sure to reset the engine
    engine.reset()

    # Good practice to dispose of the object
    engine.dispose()
