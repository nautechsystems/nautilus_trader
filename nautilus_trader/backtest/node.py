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
import pickle
from typing import Dict, List, Optional

import cloudpickle
import dask
from dask.base import normalize_token
from dask.delayed import Delayed

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.config import ActorFactory
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.model.c_enums.book_type import BookTypeParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientId
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
        run_configs : list[BacktestRunConfig]
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
                actor_configs=config.actors,
                strategy_configs=config.strategies,
                persistence=config.persistence,
                batch_size_bytes=config.batch_size_bytes,
            )
            results.append(result)

        return self._gather_delayed(results)

    def run_sync(self, run_configs: List[BacktestRunConfig]) -> List[BacktestResult]:
        """
        Run a list of backtest configs synchronously.

        Parameters
        ----------
        run_configs : list[BacktestRunConfig]
            The backtest run configurations.

        Returns
        -------
        list[BacktestResult]
            The results of the backtest runs.

        """
        results: List[BacktestResult] = []
        for config in run_configs:
            config.check()  # check all values set
            result = self._run(
                run_config_id=config.id,
                engine_config=config.engine,
                venue_configs=config.venues,
                data_configs=config.data,
                actor_configs=config.actors,
                strategy_configs=config.strategies,
                persistence=config.persistence,
                batch_size_bytes=config.batch_size_bytes,
            )
            results.append(result)

        return results

    @dask.delayed
    def _run_delayed(
        self,
        run_config_id: str,
        engine_config: BacktestEngineConfig,
        venue_configs: List[BacktestVenueConfig],
        data_configs: List[BacktestDataConfig],
        actor_configs: List[ImportableActorConfig],
        strategy_configs: List[ImportableStrategyConfig],
        persistence: Optional[PersistenceConfig] = None,
        batch_size_bytes: Optional[int] = None,
    ) -> BacktestResult:
        return self._run(
            run_config_id=run_config_id,
            engine_config=engine_config,
            venue_configs=venue_configs,
            data_configs=data_configs,
            actor_configs=actor_configs,
            strategy_configs=strategy_configs,
            persistence=persistence,
            batch_size_bytes=batch_size_bytes,
        )

    @dask.delayed
    def _gather_delayed(self, *results):
        return results

    def _run(
        self,
        run_config_id: str,
        engine_config: BacktestEngineConfig,
        venue_configs: List[BacktestVenueConfig],
        data_configs: List[BacktestDataConfig],
        actor_configs: List[ImportableActorConfig],
        strategy_configs: List[ImportableStrategyConfig],
        persistence: Optional[PersistenceConfig] = None,
        batch_size_bytes: Optional[int] = None,
    ) -> BacktestResult:
        engine: BacktestEngine = self._create_engine(
            config=engine_config,
            venue_configs=venue_configs,
            data_configs=data_configs,
        )

        # Setup persistence
        writer = None
        if persistence is not None:
            catalog = persistence.as_catalog()
            backtest_dir = f"{persistence.catalog_path.strip('/')}/backtest/"
            if not catalog.fs.exists(backtest_dir):
                catalog.fs.mkdir(backtest_dir)
            writer = FeatherWriter(
                path=f"{persistence.catalog_path}/backtest/{run_config_id}.feather",
                fs_protocol=persistence.fs_protocol,
                flush_interval=persistence.flush_interval,
                replace=persistence.replace_existing,
            )
            engine.trader.subscribe("*", writer.write)
            # Manually write instruments
            instrument_ids = set(filter(None, (data.instrument_id for data in data_configs)))
            for instrument in catalog.instruments(
                instrument_ids=list(instrument_ids), as_nautilus=True
            ):
                writer.write(instrument)

        # Create actors
        if actor_configs:
            actors: List[Actor] = [ActorFactory.create(config) for config in actor_configs]
            if actors:
                engine.add_actors(actors)

        # Create strategies
        if strategy_configs:
            strategies: List[TradingStrategy] = [
                StrategyFactory.create(config) for config in strategy_configs
            ]
            if strategies:
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
        config: BacktestEngineConfig,
        venue_configs: List[BacktestVenueConfig],
        data_configs: List[BacktestDataConfig],
    ):
        # Build the backtest engine
        engine = BacktestEngine(config=config)

        # Add instruments
        for config in data_configs:
            if is_nautilus_class(config.data_type):
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
    if data["type"] in (QuoteTick, TradeTick):
        engine.add_ticks(data=data["data"])
    elif data["type"] == Bar:
        engine.add_bars(data=data["data"])
    elif data["type"] in (OrderBookDelta, OrderBookData):
        engine.add_order_book_data(data=data["data"])
    elif data["type"] in (InstrumentStatusUpdate,):
        engine.add_data(data=data["data"])
    elif not is_nautilus_class(data["type"]):
        engine.add_generic_data(client_id=data["client_id"], data=data["data"])
    else:
        raise ValueError(f"Data type {data['type']} not setup for loading into backtest engine")


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
        if config.instrument_id and d["instrument"] is None:
            print(f"Requested instrument_id={d['instrument']} from data_config not found catalog")
            continue
        if not d["data"]:
            print(f"No data found for {config}")
            continue
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


def _extract_generic_data_client_id(data_configs: List[BacktestDataConfig]) -> Dict:
    """
    Extract a mapping of data_type : client_id from the list of `data_configs`. In the process of merging the streaming
    data, we lose the client_id for generic data, we need to inject this back in so the backtest engine can be
    correctly loaded.
    """
    data_client_ids = [
        (config.data_type, config.client_id) for config in data_configs if config.client_id
    ]
    assert len(set(data_client_ids)) == len(
        dict(data_client_ids)
    ), "data_type found with multiple client_ids"
    return dict(data_client_ids)


def streaming_backtest_runner(
    run_config_id: str,
    engine: BacktestEngine,
    data_configs: List[BacktestDataConfig],
    batch_size_bytes: Optional[int] = None,
):
    config = data_configs[0]
    catalog: DataCatalog = config.catalog()

    data_client_ids = _extract_generic_data_client_id(data_configs=data_configs)

    for data in batch_files(
        catalog=catalog,
        data_configs=data_configs,
        target_batch_size_bytes=batch_size_bytes,
    ):
        engine.clear_data()
        for data in groupby_datatype(data):
            if data["type"] in data_client_ids:
                # Generic data - manually re-add client_id as it gets lost in the streaming join
                data.update({"client_id": ClientId(data_client_ids[data["type"]])})
                data["data"] = [GenericData(data_type=DataType(cls), data=d) for d in data["data"]]
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
