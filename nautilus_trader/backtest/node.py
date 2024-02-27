# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.common.component import Logger
from nautilus_trader.common.config import ActorFactory
from nautilus_trader.common.config import InvalidConfiguration
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.core.nautilus_pyo3 import DataBackendSession
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import capsule_to_list
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import book_type_from_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.catalog.types import CatalogDataResult


class BacktestNode:
    """
    Provides a node for orchestrating groups of backtest runs.

    Parameters
    ----------
    configs : list[BacktestRunConfig]
        The backtest run configurations.

    Raises
    ------
    ValueError
        If `configs` is ``None`` or empty.
    ValueError
        If `configs` contains a type other than `BacktestRunConfig`.

    """

    def __init__(self, configs: list[BacktestRunConfig]):
        PyCondition.not_none(configs, "configs")
        PyCondition.not_empty(configs, "configs")
        PyCondition.true(
            all(isinstance(config, BacktestRunConfig) for config in configs),
            "configs",
        )

        self._validate_configs(configs)

        # Configuration
        self._configs: list[BacktestRunConfig] = configs
        self._engines: dict[str, BacktestEngine] = {}

    @property
    def configs(self) -> list[BacktestRunConfig]:
        """
        Return the loaded backtest run configs for the node.

        Returns
        -------
        list[BacktestRunConfig]

        """
        return self._configs

    def get_engine(self, run_config_id: str) -> BacktestEngine | None:
        """
        Return the backtest engine associated with the given run config ID (if found).

        Parameters
        ----------
        run_config_id : str
            The run configuration ID for the created engine.

        Returns
        -------
        BacktestEngine or ``None``

        """
        return self._engines.get(run_config_id)

    def get_engines(self) -> list[BacktestEngine]:
        """
        Return all backtest engines created by the node.

        Returns
        -------
        list[BacktestEngine]

        """
        return list(self._engines.values())

    def run(self) -> list[BacktestResult]:
        """
        Run the backtest node which will synchronously execute the list of loaded
        backtest run configs.

        Any exceptions raised from a backtest will be printed to stdout and
        the next backtest run will commence (if any).

        Returns
        -------
        list[BacktestResult]
            The results of the backtest runs.

        """
        results: list[BacktestResult] = []
        for config in self._configs:
            try:
                result = self._run(
                    run_config_id=config.id,
                    engine_config=config.engine,
                    venue_configs=config.venues,
                    data_configs=config.data,
                    batch_size_bytes=config.batch_size_bytes,
                )
                results.append(result)
            except Exception as e:
                # Broad catch all prevents a single backtest run from halting
                # the execution of the other backtests (such as a zero balance exception).
                Logger(type(self).__name__).error(f"Error running back: {e}")
                Logger(type(self).__name__).info(f"Config: {config}")

        return results

    def _validate_configs(self, configs: list[BacktestRunConfig]) -> None:
        venue_ids: list[Venue] = []
        for config in configs:
            venue_ids += [Venue(c.name) for c in config.venues]

        for config in configs:
            for data_config in config.data:
                if data_config.instrument_id is None:
                    continue  # No instrument associated with data

                if data_config.start_time is not None and data_config.end_time is not None:
                    start = dt_to_unix_nanos(data_config.start_time)
                    end = dt_to_unix_nanos(data_config.end_time)

                    if end < start:
                        raise InvalidConfiguration(
                            f"`end_time` ({data_config.end_time}) is before `start_time` ({data_config.start_time})",
                        )

                instrument_id: InstrumentId = data_config.instrument_id
                if instrument_id.venue not in venue_ids:
                    raise InvalidConfiguration(
                        f"Venue '{instrument_id.venue}' for {instrument_id} "
                        f"does not have a `BacktestVenueConfig`",
                    )

    def _create_engine(
        self,
        run_config_id: str,
        config: BacktestEngineConfig,
        venue_configs: list[BacktestVenueConfig],
        data_configs: list[BacktestDataConfig],
    ) -> BacktestEngine:
        # Build the backtest engine
        engine = BacktestEngine(config=config)
        self._engines[run_config_id] = engine

        # Add venues (must be added prior to instruments)
        for config in venue_configs:
            base_currency: str | None = config.base_currency
            leverages = (
                {InstrumentId.from_str(i): Decimal(v) for i, v in config.leverages.items()}
                if config.leverages
                else {}
            )
            engine.add_venue(
                venue=Venue(config.name),
                oms_type=OmsType[config.oms_type],
                account_type=AccountType[config.account_type],
                base_currency=Currency.from_str(base_currency) if base_currency else None,
                starting_balances=[Money.from_str(m) for m in config.starting_balances],
                default_leverage=Decimal(config.default_leverage),
                leverages=leverages,
                book_type=book_type_from_str(config.book_type),
                routing=config.routing,
                modules=[ActorFactory.create(module) for module in (config.modules or [])],
                frozen_account=config.frozen_account,
                reject_stop_orders=config.reject_stop_orders,
                support_gtd_orders=config.support_gtd_orders,
                support_contingent_orders=config.support_contingent_orders,
                use_position_ids=config.use_position_ids,
                use_random_ids=config.use_random_ids,
                use_reduce_only=config.use_reduce_only,
            )

        # Add instruments
        for config in data_configs:
            if is_nautilus_class(config.data_type):
                catalog = self.load_catalog(config)
                instruments = catalog.instruments(instrument_ids=config.instrument_id)
                for instrument in instruments or []:
                    if instrument.id not in engine.cache.instrument_ids():
                        engine.add_instrument(instrument)

        return engine

    def _load_engine_data(self, engine: BacktestEngine, result: CatalogDataResult) -> None:
        if is_nautilus_class(result.data_cls):
            engine.add_data(data=result.data)
        else:
            if not result.client_id:
                raise ValueError(
                    f"Data type {result.data_cls} not setup for loading into `BacktestEngine`",
                )
            engine.add_data(data=result.data, client_id=result.client_id)

    def _run(
        self,
        run_config_id: str,
        engine_config: BacktestEngineConfig,
        venue_configs: list[BacktestVenueConfig],
        data_configs: list[BacktestDataConfig],
        batch_size_bytes: int | None = None,
    ) -> BacktestResult:
        engine: BacktestEngine = self._create_engine(
            run_config_id=run_config_id,
            config=engine_config,
            venue_configs=venue_configs,
            data_configs=data_configs,
        )

        # Run backtest
        if batch_size_bytes is not None:
            self._run_streaming(
                run_config_id=run_config_id,
                engine=engine,
                data_configs=data_configs,
                batch_size_bytes=batch_size_bytes,
            )
        else:
            self._run_oneshot(
                run_config_id=run_config_id,
                engine=engine,
                data_configs=data_configs,
            )

        # Release data objects
        engine.dispose()

        return engine.get_result()

    def _run_streaming(
        self,
        run_config_id: str,
        engine: BacktestEngine,
        data_configs: list[BacktestDataConfig],
        batch_size_bytes: int,
    ) -> None:
        # Create session for entire stream
        session = DataBackendSession(chunk_size=batch_size_bytes)

        # Add query for all data configs
        for config in data_configs:
            catalog = self.load_catalog(config)
            if config.data_type == Bar:
                # TODO: Temporary hack - improve bars config and decide implementation with `filter_expr`
                assert config.instrument_id, "No `instrument_id` for Bar data config"
                assert config.bar_spec, "No `bar_spec` for Bar data config"
                bar_type = config.instrument_id + "-" + config.bar_spec + "-EXTERNAL"
            else:
                bar_type = None
            session = catalog.backend_session(
                data_cls=config.data_type,
                instrument_ids=(
                    [config.instrument_id] if config.instrument_id and not bar_type else []
                ),
                bar_types=[bar_type] if bar_type else [],
                start=config.start_time,
                end=config.end_time,
                session=session,
            )

        # Stream data
        for chunk in session.to_query_result():
            engine.add_data(
                data=capsule_to_list(chunk),
                validate=False,  # Cannot validate mixed type stream
                sort=True,  # Temporarily sorting  # Already sorted from kmerge
            )
            engine.run(
                run_config_id=run_config_id,
                streaming=True,
            )

        engine.end()
        engine.dispose()

    def _run_oneshot(
        self,
        run_config_id: str,
        engine: BacktestEngine,
        data_configs: list[BacktestDataConfig],
    ) -> None:
        # Load data
        for config in data_configs:
            t0 = pd.Timestamp.now()
            engine.logger.info(
                f"Reading {config.data_type} data for instrument={config.instrument_id}.",
            )
            result: CatalogDataResult = self.load_data_config(config)
            if config.instrument_id and result.instrument is None:
                engine.logger.warning(
                    f"Requested instrument_id={result.instrument} from data_config not found in catalog",
                )
                continue
            if not result.data:
                engine.logger.warning(f"No data found for {config}")
                continue

            t1 = pd.Timestamp.now()
            engine.logger.info(
                f"Read {len(result.data):,} events from parquet in {pd.Timedelta(t1 - t0)}s.",
            )
            self._load_engine_data(engine=engine, result=result)
            t2 = pd.Timestamp.now()
            engine.logger.info(f"Engine load took {pd.Timedelta(t2 - t1)}s")

        engine.run(run_config_id=run_config_id)
        engine.dispose()

    @classmethod
    def load_catalog(cls, config: BacktestDataConfig) -> ParquetDataCatalog:
        return ParquetDataCatalog(
            path=config.catalog_path,
            fs_protocol=config.catalog_fs_protocol,
            fs_storage_options=config.catalog_fs_storage_options,
        )

    @classmethod
    def load_data_config(cls, config: BacktestDataConfig) -> CatalogDataResult:
        catalog: ParquetDataCatalog = cls.load_catalog(config)

        instruments = (
            catalog.instruments(instrument_ids=[config.instrument_id])
            if config.instrument_id
            else None
        )
        if config.instrument_id and not instruments:
            return CatalogDataResult(data_cls=config.data_type, data=[])

        return CatalogDataResult(
            data_cls=config.data_type,
            data=catalog.query(**config.query),
            instrument=instruments[0] if instruments else None,
            client_id=ClientId(config.client_id) if config.client_id else None,
        )

    def dispose(self):
        for engine in self.get_engines():
            if not engine.trader.is_disposed:
                engine.dispose()
