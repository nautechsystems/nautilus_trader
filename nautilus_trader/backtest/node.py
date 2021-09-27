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
import logging
import pickle
from typing import Any, Dict, List, Optional, Tuple

import cloudpickle
import dask
from dask.base import normalize_token
from dask.base import tokenize
from dask.delayed import Delayed

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.backtest.results import BacktestRunResults
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
from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.config import StrategyFactory
from nautilus_trader.trading.strategy import TradingStrategy


logger = logging.getLogger(__name__)


class BacktestNode:
    """
    Provides a node for orchestrating groups of configurable backtest runs.

    These can be run synchronously, or can be built into a lazily evaluated
    graph for execution by a dask executor.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``BacktestNode`` class.
        """

    def build_graph(self, run_configs: List[BacktestRunConfig]) -> Delayed:
        """
        Build a `Delayed` graph from `backtest_configs` which can be passed to a dask executor.

        Parameters
        ----------
        run_configs : List[BacktestRunConfig]
            The backtest run configurations.

        Returns
        -------
        Delayed
            The delayed graph, yet to be computed.

        """
        results: List[Tuple[str, Dict[str, Any]]] = []
        for config in run_configs:
            config.check(ignore=("name",))  # check all values set
            results.append(
                self._run_delayed(
                    name=config.name or f"backtest-{tokenize(config)}",
                    venue_configs=config.venues,
                    data_configs=config.data,
                    strategy_configs=config.strategies,
                    batch_size_bytes=config.batch_size_bytes,
                )
            )
        return self._gather_delayed(results)

    def run_sync(self, run_configs: List[BacktestRunConfig]) -> BacktestRunResults:
        """
        Run a list of backtest configs synchronously.

        Parameters
        ----------
        run_configs : List[BacktestRunConfig]
            The backtest run configurations.

        Returns
        -------
        Dict[str, Any]
            The results of the backtest runs.

        """
        results: List[BacktestResult] = []
        for config in run_configs:
            config.check(ignore=("name",))  # check all values set
            results.append(
                self._run(
                    name=config.name or f"backtest-{tokenize(config)}",
                    venue_configs=config.venues,
                    data_configs=config.data,
                    strategy_configs=config.strategies,
                    batch_size_bytes=config.batch_size_bytes,
                )
            )
        return self._gather(results)

    # @dask.delayed(pure=True)
    # def _load_delayed(self, config: BacktestDataConfig):
    #     return self._load(config=config)
    #
    # def _load(self, config: BacktestDataConfig):
    #     query = config.query
    #     return {
    #         "type": query["cls"],
    #         "data": config.catalog.query(**query),
    #         "instrument": config.catalog.instruments(
    #             instrument_ids=config.instrument_id, as_nautilus=True
    #         )[0],
    #         "client_id": config.client_id,
    #     }

    @dask.delayed
    def _run_delayed(
        self,
        name: str,
        venue_configs: List[BacktestVenueConfig],
        data_configs: List[BacktestDataConfig],
        strategy_configs: List[ImportableStrategyConfig],
        batch_size_bytes: Optional[int] = None,
    ):
        return self._run(
            name=name,
            venue_configs=venue_configs,
            data_configs=data_configs,
            strategy_configs=strategy_configs,
            batch_size_bytes=batch_size_bytes,
        )

    def _run(
        self,
        name,
        venue_configs: List[BacktestVenueConfig],
        data_configs: List[BacktestDataConfig],
        strategy_configs: List[ImportableStrategyConfig],
        batch_size_bytes: Optional[int] = None,
    ) -> BacktestResult:
        engine: BacktestEngine = self._create_engine(
            venue_configs=venue_configs,
            data_configs=data_configs,
        )
        results: BacktestResult = self._run_engine(
            name=name,
            engine=engine,
            data_configs=data_configs,
            strategy_configs=strategy_configs,
            batch_size_bytes=batch_size_bytes,
        )
        return results

    def _create_engine(
        self, venue_configs: List[BacktestVenueConfig], data_configs: List[BacktestDataConfig]
    ):
        # Configure backtest engine
        config = BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=True,
        )
        # Build the backtest engine
        engine = BacktestEngine(config=config)

        # Add instruments
        for config in data_configs:
            instruments = config.catalog().instruments(
                instrument_ids=config.instrument_id, as_nautilus=True
            )
            for instrument in instruments or []:
                engine.add_instrument(instrument)

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
        name: str,
        engine: BacktestEngine,
        data_configs: List[BacktestDataConfig],
        strategy_configs: List[ImportableStrategyConfig],
        batch_size_bytes: Optional[int] = None,
    ) -> BacktestResult:
        """
        Actual execution of a backtest instance. Creates strategies and runs the engine
        """
        # Create strategies
        strategies: List[TradingStrategy] = [
            StrategyFactory.create(config) for config in strategy_configs
        ]

        engine.add_strategies(strategies)

        # Actual run backtest
        backtest_runner(
            engine=engine,
            data_configs=data_configs,
            batch_size_bytes=batch_size_bytes,
        )

        result = BacktestResult.from_engine(backtest_id=name, engine=engine)

        engine.dispose()

        return result

    @dask.delayed
    def _gather_delayed(self, *results):
        return self._gather(*results)

    def _gather(self, *results) -> BacktestRunResults:
        return BacktestRunResults(sum(results, list()))


def _load_engine_data(engine: BacktestEngine, data):
    if data["type"] == QuoteTick:
        engine.add_ticks(data=data["data"])
    elif data["type"] == TradeTick:
        engine.add_ticks(data=data["data"])
    elif data["type"] == OrderBookDelta:
        engine.add_order_book_data(data=data["data"])
    else:
        engine.add_generic_data(client_id=data["client_id"], data=data["data"])


def backtest_runner(
    engine: BacktestEngine,
    data_configs: List[BacktestDataConfig],
    batch_size_bytes: Optional[int] = None,
):
    """Execute a backtest run"""
    if batch_size_bytes is not None:
        return streaming_backtest_runner(
            engine=engine,
            data_configs=data_configs,
            batch_size_bytes=batch_size_bytes,
        )

    # Load data
    for config in data_configs:
        d = config.load()
        _load_engine_data(engine=engine, data=d)

    return engine.run()


def streaming_backtest_runner(
    engine: BacktestEngine,
    data_configs: List[BacktestDataConfig],
    batch_size_bytes: Optional[int] = None,
):
    config = data_configs[0]
    catalog = config.catalog()

    streaming_kw = merge_data_configs_for_calc_streaming_chunks(data_configs=data_configs)
    for start, end in catalog.calc_streaming_chunks(**streaming_kw, target_size=batch_size_bytes):
        engine.clear_data()
        for config in data_configs:
            data = config.load(start_time=start, end_time=end)
            if not data["data"]:
                continue
            _load_engine_data(engine=engine, data=data)
        engine.run_streaming(start=start, end=end)
    engine.end_streaming()


# Register tokenization methods with dask
for cls in Instrument.__subclasses__():
    normalize_token.register(cls, func=cls.to_dict)


def merge_data_configs_for_calc_streaming_chunks(data_configs: List[BacktestDataConfig]):
    instrument_ids = [c.instrument_id for c in data_configs]
    data_types = [c.data_type for c in data_configs]
    starts = [c.start_time for c in data_configs]
    if len(set(starts)) > 1:
        logger.warning("Multiple start dates in data_configs, using min")
    ends = [c.end_time for c in data_configs]
    if len(set(ends)) > 1:
        logger.warning("Multiple start dates in data_configs, using min")
    return {
        "instrument_ids": instrument_ids,
        "data_types": data_types,
        "start_time": starts[0],
        "end_time": ends[0],
    }


@normalize_token.register(object)
def nautilus_tokenize(o: object):
    return cloudpickle.dumps(o, protocol=pickle.DEFAULT_PROTOCOL)


@normalize_token.register(ImportableStrategyConfig)
def tokenize_strategy_config(config: ImportableStrategyConfig):
    return config.dict()


@normalize_token.register(BacktestRunConfig)
def tokenize_backtest_run_config(config: BacktestRunConfig):
    return config.__dict__
