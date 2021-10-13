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

import itertools
import logging
import pickle
from typing import List, Optional

import cloudpickle
import dask
import pandas as pd
from dask.base import normalize_token
from dask.delayed import Delayed

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.core.datetime import maybe_dt_to_unix_nanos
from nautilus_trader.model.c_enums.book_type import BookTypeParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.persistence.batching import batch_files
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.config import PersistenceConfig
from nautilus_trader.persistence.streaming import FeatherWriter
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
        results: List[BacktestResult] = []
        for config in run_configs:
            config.check()  # check all values set
            result = self._run_delayed(
                run_config_id=config.id,
                venue_configs=config.venues,
                data_configs=config.data,
                strategy_configs=config.strategies,
                batch_size_bytes=config.batch_size_bytes,
            )
            results.append(result)

        return self._gather_delayed(results)

    def run_sync(self, run_configs: List[BacktestRunConfig]) -> List[BacktestResult]:
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
            config.check()  # check all values set
            result = self._run(
                run_config_id=config.id,
                venue_configs=config.venues,
                data_configs=config.data,
                persistence=config.persistence,
                strategy_configs=config.strategies,
                batch_size_bytes=config.batch_size_bytes,
            )
            results.append(result)

        return results

    @dask.delayed
    def _run_delayed(
        self,
        run_config_id: str,
        venue_configs: List[BacktestVenueConfig],
        data_configs: List[BacktestDataConfig],
        strategy_configs: List[ImportableStrategyConfig],
        batch_size_bytes: Optional[int] = None,
    ) -> BacktestResult:
        return self._run(
            run_config_id=run_config_id,
            venue_configs=venue_configs,
            data_configs=data_configs,
            strategy_configs=strategy_configs,
            batch_size_bytes=batch_size_bytes,
        )

    @dask.delayed
    def _gather_delayed(self, *results):
        return results

    def _run(
        self,
        run_config_id: str,
        venue_configs: List[BacktestVenueConfig],
        data_configs: List[BacktestDataConfig],
        strategy_configs: List[ImportableStrategyConfig],
        persistence: Optional[PersistenceConfig] = None,
        batch_size_bytes: Optional[int] = None,
    ) -> BacktestResult:
        engine: BacktestEngine = self._create_engine(
            venue_configs=venue_configs,
            data_configs=data_configs,
        )
        # Create strategies
        strategies: List[TradingStrategy] = [
            StrategyFactory.create(config) for config in strategy_configs
        ]

        # Setup persistence
        writer = None
        if persistence is not None:
            catalog = persistence.as_catalog()
            catalog.fs.mkdir(f"{persistence.catalog_path}/backtest/")
            writer = FeatherWriter(
                path=f"{persistence.catalog_path}/backtest/{run_config_id}.feather",
                fs_protocol=persistence.fs_protocol,
                flush_interval=persistence.flush_interval,
            )
            engine.trader.subscribe("*", writer.write)

        engine.add_strategies(strategies)

        # Run backtest
        backtest_runner(
            run_config_id=run_config_id,
            engine=engine,
            data_configs=data_configs,
            batch_size_bytes=batch_size_bytes,
        )

        result = engine.get_result()

        engine.dispose()
        if writer is not None:
            writer.close()

        return result

    def _create_engine(
        self,
        venue_configs: List[BacktestVenueConfig],
        data_configs: List[BacktestDataConfig],
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
                book_type=BookTypeParser.from_str_py(config.book_type),
            )
        return engine


def _load_engine_data(engine: BacktestEngine, data):
    if data["type"] == QuoteTick:
        engine.add_ticks(data=data["data"])
    elif data["type"] == TradeTick:
        engine.add_ticks(data=data["data"])
    elif data["type"] in (OrderBookDelta, OrderBookData):
        engine.add_order_book_data(data=data["data"])
    else:
        engine.add_generic_data(client_id=data["client_id"], data=data["data"])


def backtest_runner(
    run_config_id: str,
    engine: BacktestEngine,
    data_configs: List[BacktestDataConfig],
    batch_size_bytes: Optional[int] = None,
):
    """Execute a backtest run."""
    if batch_size_bytes is not None:
        return streaming_backtest_runner(
            run_config_id=run_config_id,
            engine=engine,
            data_configs=data_configs,
            batch_size_bytes=batch_size_bytes,
        )

    # Load data
    for config in data_configs:
        d = config.load()
        _load_engine_data(engine=engine, data=d)

    return engine.run(run_config_id=run_config_id)


def _groupby_key(x):
    return type(x).__name__


def groupby_datatype(data):
    return [
        {"type": type(v[0]), "data": v}
        for v in [
            list(v) for _, v in itertools.groupby(sorted(data, key=_groupby_key), key=_groupby_key)
        ]
    ]


def streaming_backtest_runner(
    run_config_id: str,
    engine: BacktestEngine,
    data_configs: List[BacktestDataConfig],
    batch_size_bytes: Optional[int] = None,
):
    config = data_configs[0]
    catalog: DataCatalog = config.catalog()
    start_time = maybe_dt_to_unix_nanos(pd.Timestamp(min(dc.start_time for dc in data_configs)))
    end_time = maybe_dt_to_unix_nanos(pd.Timestamp(max(dc.end_time for dc in data_configs)))

    for data in batch_files(
        catalog=catalog,
        data_configs=data_configs,
        start_time=start_time,
        end_time=end_time,
        target_batch_size_bytes=batch_size_bytes,
    ):
        engine.clear_data()
        for data in groupby_datatype(data):
            _load_engine_data(engine=engine, data=data)
        engine.run_streaming(run_config_id=run_config_id)
    engine.end_streaming()


# Register tokenization methods with dask
for cls in Instrument.__subclasses__():
    normalize_token.register(cls, func=cls.to_dict)


@normalize_token.register(object)
def nautilus_tokenize(o: object):
    return cloudpickle.dumps(o, protocol=pickle.DEFAULT_PROTOCOL)


@normalize_token.register(ImportableStrategyConfig)
def tokenize_strategy_config(config: ImportableStrategyConfig):
    return config.dict()


@normalize_token.register(BacktestRunConfig)
def tokenize_backtest_run_config(config: BacktestRunConfig):
    return config.__dict__
