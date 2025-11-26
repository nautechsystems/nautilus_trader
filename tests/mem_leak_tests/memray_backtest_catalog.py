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

import shutil
import tempfile
from decimal import Decimal
from pathlib import Path

from nautilus_trader.backtest.node import BacktestDataConfig
from nautilus_trader.backtest.node import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.node import BacktestRunConfig
from nautilus_trader.backtest.node import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def setup_catalog_with_data():
    """
    Set up a temporary catalog with test data.
    """
    # Create temporary directory for catalog
    catalog_path = Path(tempfile.mkdtemp())
    catalog = ParquetDataCatalog(catalog_path)

    # Set up instrument and data
    AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    provider = TestDataProvider()
    wrangler = QuoteTickDataWrangler(instrument=AUDUSD_SIM)

    # Process test data and write to catalog
    ticks = wrangler.process(provider.read_csv_ticks("truefx/audusd-ticks.csv"))
    ticks.sort(key=lambda x: x.ts_init)

    catalog.write_data([AUDUSD_SIM])
    catalog.write_data(ticks)

    return catalog_path, AUDUSD_SIM, provider


count = 0
total_runs = 128

if __name__ == "__main__":
    while count < total_runs:
        print(f"Run: {count}/{total_runs}")

        # Set up catalog with data for each run
        catalog_path, instrument, provider = setup_catalog_with_data()

        try:
            # Configure backtest with catalog streaming
            venues = [
                BacktestVenueConfig(
                    name="SIM",
                    oms_type="HEDGING",
                    account_type="MARGIN",  # Use MARGIN account for FX pairs
                    base_currency="USD",
                    starting_balances=["1_000_000 USD"],
                ),
            ]

            # Get data range from test file
            test_data = provider.read_csv_ticks("truefx/audusd-ticks.csv")
            start_time = dt_to_unix_nanos(test_data.index[0])
            end_time = dt_to_unix_nanos(test_data.index[-1])

            data_configs = [
                BacktestDataConfig(
                    catalog_path=str(catalog_path),
                    data_cls=QuoteTick,
                    instrument_id=instrument.id,
                    start_time=start_time,
                    end_time=end_time,
                ),
            ]

            strategies = [
                ImportableStrategyConfig(
                    strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
                    config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
                    config={
                        "instrument_id": instrument.id,
                        "bar_type": "AUD/USD.SIM-15-MINUTE-BID-INTERNAL",
                        "fast_ema_period": 10,
                        "slow_ema_period": 20,
                        "trade_size": Decimal(100_000),
                    },
                ),
            ]

            # Configure backtest run with streaming (small chunk size for memory testing)
            config = BacktestRunConfig(
                engine=BacktestEngineConfig(
                    trader_id=TraderId("BACKTESTER-001"),
                    strategies=strategies,
                    logging=LoggingConfig(bypass_logging=True),
                ),
                data=data_configs,
                venues=venues,
                chunk_size=1000,  # Enable streaming mode with small chunks
                raise_exception=True,  # Raise exceptions to catch errors
            )

            # Build and run backtest node
            node = BacktestNode(configs=[config])

            try:
                results = node.run()

                # Clean up
                del results
                del node
            except Exception as e:
                print(f"Run {count} failed with error: {e}")
                del node
                break  # Stop execution on error
        finally:
            # Clean up temporary catalog directory
            if catalog_path.exists():
                shutil.rmtree(catalog_path)

        count += 1
