# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from functools import partial
from typing import Any, Dict, List, Tuple

import dask
import pandas as pd
from dask.base import normalize_token
from dask.base import tokenize
from dask.delayed import Delayed

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.config import StrategyFactory
from nautilus_trader.trading.strategy import TradingStrategy


# Register tokenization methods with dask
for cls in Instrument.__subclasses__():
    normalize_token.register(cls, func=cls.to_dict)


class BacktestNode:
    """
    Provides a node for managing backtest runs.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``BacktestNode`` class.
        """

    def build_graph(self, run_configs: List[BacktestRunConfig]) -> Delayed:
        """
        Build a `Delayed` graph from `backtest_configs` which can be passed to a dask executor.

        Returns
        -------
        Delayed

        """
        results: List[Tuple[str, Dict[str, Any]]] = []
        for config in run_configs:
            config.check(ignore=("name",))  # check all values set
            input_data = []
            for data_config in config.data:
                load_func = partial(
                    self._load_delayed,
                    dask_key_name=f"load-{tokenize(data_config.query)}",
                )
                input_data.append(load_func(data_config))
            results.append(
                self._run_delayed(
                    name=config.name or f"backtest-{tokenize(config)}",
                    venue_configs=config.venues,
                    input_data=input_data,
                    strategy_configs=config.strategies,
                )
            )
        return self._gather_delayed(results)

    def run_sync(self, run_configs: List[BacktestRunConfig]) -> Dict[str, Any]:
        """
        Run a list of backtest configs synchronously.

        Parameters
        ----------
        run_configs : Union[BacktestRunConfig, List[BacktestRunConfig]]
            The backtest run configurations.

        """
        results: List[Tuple[str, Dict[str, Any]]] = []
        for config in run_configs:
            config.check(ignore=("name",))  # check all values set
            input_data = []
            for data_config in config.data:
                input_data.append(self._load(data_config))
            results.append(
                self._run(
                    name=config.name or f"backtest-{tokenize(config)}",
                    venue_configs=config.venues,
                    input_data=input_data,
                    strategy_configs=config.strategies,
                )
            )
        return self._gather(results)

    @dask.delayed(pure=True)
    def _load_delayed(self, config: BacktestDataConfig):
        return self._load(config=config)

    def _load(self, config: BacktestDataConfig):
        catalog = DataCatalog(
            path=config.catalog_path,
            fs_protocol=config.catalog_fs_protocol,
        )
        query = config.query
        return {
            "type": query["cls"],
            "data": catalog.query(**query),
            "instrument": catalog.instruments(
                instrument_ids=config.instrument_id, as_nautilus=True
            )[0],
            "client_id": config.client_id,
        }

    @dask.delayed
    def _run_delayed(self, name, venue_configs, input_data, strategy_configs):
        return self._run(
            name=name,
            venue_configs=venue_configs,
            input_data=input_data,
            strategy_configs=strategy_configs,
        )

    def _run(self, name, venue_configs, input_data, strategy_configs) -> Tuple[str, Dict[str, Any]]:
        engine: BacktestEngine = self._create_engine(
            venue_configs=venue_configs,
            input_data=input_data,
        )
        results: Dict[str, Any] = self._run_engine(
            engine=engine,
            strategy_configs=strategy_configs,
        )
        return name, results

    def _create_engine(
        self,
        venue_configs: List[BacktestVenueConfig],
        input_data,
    ):
        # Configure backtest engine
        config = BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=True,
        )
        # Build the backtest engine
        engine = BacktestEngine(config=config)

        # Add data
        for d in input_data:
            instrument = d["instrument"]
            if instrument is not None:
                engine.add_instrument(instrument)

            if d["type"] == QuoteTick:
                engine.add_quote_ticks_objects(data=d["data"], instrument_id=d["instrument"].id)
            elif d["type"] == TradeTick:
                engine.add_trade_tick_objects(data=d["data"], instrument_id=d["instrument"].id)
            elif d["type"] == OrderBookDelta:
                engine.add_order_book_data(data=d["data"])
            else:
                engine.add_generic_data(client_id=d["client_id"], data=d["data"])

        # Add venues
        for config in venue_configs:
            engine.add_venue(
                venue=Venue(config.name),
                venue_type=VenueType[config.venue_type],
                oms_type=OMSType[config.oms_type],
                account_type=AccountType[config.account_type],
                base_currency=Currency.from_str(config.base_currency),
                starting_balances=[Money.from_str(m) for m in config.starting_balances],
            )
        return engine

    def _run_engine(
        self,
        engine: BacktestEngine,
        strategy_configs: List[ImportableStrategyConfig],
    ) -> Dict[str, Any]:
        # Create strategies
        strategies: List[TradingStrategy] = [
            StrategyFactory.create(config) for config in strategy_configs
        ]

        # Run backtest
        engine.run(strategies=strategies)

        # Collect results
        data = {
            "account": pd.concat(
                [
                    engine.trader.generate_account_report(venue).assign(venue=venue)
                    for venue in engine.list_venues()
                ]
            ),
            "fills": engine.trader.generate_order_fills_report(),
            "positions": engine.trader.generate_positions_report(),
        }
        engine.dispose()
        return data

    @dask.delayed
    def _gather_delayed(self, *results):
        return self._gather(*results)

    def _gather(self, *results):
        return {k: v for r in results for k, v in r}
