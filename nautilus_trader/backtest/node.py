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

import json
from decimal import Decimal

import pandas as pd

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import LogGuard
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.component import is_logging_initialized
from nautilus_trader.common.config import ActorFactory
from nautilus_trader.common.config import InvalidConfiguration
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import max_date
from nautilus_trader.core.datetime import min_date
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.core.nautilus_pyo3 import DataBackendSession
from nautilus_trader.model import BOOK_DATA_TYPES
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import capsule_to_list
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
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

    def __init__(self, configs: list[BacktestRunConfig]) -> None:
        PyCondition.not_none(configs, "configs")
        PyCondition.not_empty(configs, "configs")
        PyCondition.is_true(
            all(isinstance(config, BacktestRunConfig) for config in configs),
            "configs",
        )
        self._validate_configs(configs)

        self._configs: list[BacktestRunConfig] = configs
        self._engines: dict[str, BacktestEngine] = {}
        self._log_guard: nautilus_pyo3.LogGuard | LogGuard | None = None

    @property
    def configs(self) -> list[BacktestRunConfig]:
        """
        Return the loaded backtest run configs for the node.

        Returns
        -------
        list[BacktestRunConfig]

        """
        return self._configs

    def get_log_guard(self) -> nautilus_pyo3.LogGuard | LogGuard | None:
        """
        Return the global logging systems log guard.

        May return ``None`` if no internal engines are initialized yet.

        Returns
        -------
        nautilus_pyo3.LogGuard | LogGuard | None

        """
        return self._log_guard

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

    def dispose(self):
        for engine in self.get_engines():
            if not engine.trader.is_disposed:
                engine.dispose()

    def _validate_configs(self, configs: list[BacktestRunConfig]) -> None:  # noqa: C901
        venue_ids: list[Venue] = []

        for config in configs:
            venue_ids += [Venue(c.name) for c in config.venues]

        for config in configs:
            for data_config in config.data:
                used_instrument_ids: list[InstrumentId] = get_instrument_ids(data_config)

                if len(used_instrument_ids) == 0:
                    continue  # No instrument associated with data

                if data_config.start_time is not None and data_config.end_time is not None:
                    start = dt_to_unix_nanos(data_config.start_time)
                    end = dt_to_unix_nanos(data_config.end_time)

                    if end < start:
                        raise InvalidConfiguration(
                            f"`end_time` ({data_config.end_time}) is before `start_time` ({data_config.start_time})",
                        )

                for instrument_id in used_instrument_ids:
                    if instrument_id.venue not in venue_ids:
                        raise InvalidConfiguration(
                            f"Venue '{instrument_id.venue}' for {instrument_id} "
                            f"does not have a `BacktestVenueConfig`",
                        )

            for venue_config in config.venues:
                venue = Venue(venue_config.name)
                book_type = book_type_from_str(venue_config.book_type)

                # Check order book data configuration
                if book_type in (BookType.L2_MBP, BookType.L3_MBO):
                    has_book_data = any(
                        data_config.instrument_id
                        and data_config.instrument_id.venue == venue
                        and data_config.data_type in BOOK_DATA_TYPES
                        for data_config in config.data
                    )

                    if not has_book_data:
                        raise InvalidConfiguration(
                            f"No order book data available for {venue} with book type {venue_config.book_type}",
                        )

    def run(self, raise_exception=False) -> list[BacktestResult]:
        """
        Run the backtest node which will synchronously execute the list of loaded
        backtest run configs.

        Parameters
        ----------
        raise_exception : bool, default False
            If True, an exception raised from a backtest will be re-raised and halt the node.
            If False, exceptions raised from backtest(s) will be printed to stdout.

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
                    chunk_size=config.chunk_size,
                    dispose_on_completion=config.dispose_on_completion,
                    start=config.start,
                    end=config.end,
                )
                results.append(result)
            except Exception as e:
                # Broad catch all prevents a single backtest run from halting
                # the execution of the other backtests (such as a zero balance exception).
                if not is_logging_initialized():
                    _guard = init_logging()

                log = Logger(type(self).__name__)
                log.exception("Error running backtest", e)
                if config.engine is not None:
                    log.info("Engine config:", LogColor.MAGENTA)
                    log.info(json.dumps(json.loads(config.engine.json()), indent=2))
                log.info("Venue configs:", LogColor.MAGENTA)
                for venue_config in config.venues:
                    log.info(json.dumps(json.loads(venue_config.json()), indent=2))
                log.info("Data configs:", LogColor.MAGENTA)
                for data_config in config.data:
                    log.info(json.dumps(json.loads(data_config.json()), indent=2))

                if raise_exception:
                    raise e

        return results

    def _run(
        self,
        run_config_id: str,
        engine_config: BacktestEngineConfig,
        venue_configs: list[BacktestVenueConfig],
        data_configs: list[BacktestDataConfig],
        chunk_size: int | None,
        dispose_on_completion: bool,
        start: str | int | None = None,
        end: str | int | None = None,
    ) -> BacktestResult:
        engine: BacktestEngine = self._create_engine(
            run_config_id=run_config_id,
            config=engine_config,
            venue_configs=venue_configs,
            data_configs=data_configs,
        )

        # Run backtest
        if chunk_size is not None:
            self._run_streaming(
                run_config_id=run_config_id,
                engine=engine,
                data_configs=data_configs,
                chunk_size=chunk_size,
                start=start,
                end=end,
            )
        else:
            self._run_oneshot(
                run_config_id=run_config_id,
                engine=engine,
                data_configs=data_configs,
                start=start,
                end=end,
            )

        if dispose_on_completion:
            # Drop data and all state
            engine.dispose()
        else:
            # Drop data
            engine.clear_data()

        return engine.get_result()

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

        # Assign the global logging system guard to keep it alive for
        # the duration of the nodes runs.
        log_guard = engine.kernel.get_log_guard()

        if log_guard:
            self._log_guard = log_guard

        # Add venues (must be added prior to instruments)
        for config in venue_configs:
            engine.add_venue(
                venue=Venue(config.name),
                oms_type=OmsType[config.oms_type],
                account_type=AccountType[config.account_type],
                base_currency=get_base_currency(config),
                starting_balances=get_starting_balances(config),
                default_leverage=Decimal(config.default_leverage),
                leverages=get_leverages(config),
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
                bar_execution=config.bar_execution,
                bar_adaptive_high_low_ordering=config.bar_adaptive_high_low_ordering,
                trade_execution=config.trade_execution,
            )

        # Add instruments
        for config in data_configs:
            if is_nautilus_class(config.data_type):
                catalog = self.load_catalog(config)
                used_instrument_ids = get_instrument_ids(config)

                # None to query all instruments
                instruments = catalog.instruments(
                    instrument_ids=(used_instrument_ids if len(used_instrument_ids) > 0 else None),
                )

                for instrument in instruments or []:
                    if instrument.id not in engine.cache.instrument_ids():
                        engine.add_instrument(instrument)

        return engine

    def _run_streaming(  # noqa: C901
        self,
        run_config_id: str,
        engine: BacktestEngine,
        data_configs: list[BacktestDataConfig],
        chunk_size: int,
        start: str | int | None = None,
        end: str | int | None = None,
    ) -> None:
        # Create session for entire stream
        session = DataBackendSession(chunk_size=chunk_size)

        # Add query for all data configs
        for config in data_configs:
            catalog = self.load_catalog(config)
            used_start = config.start_time
            used_end = config.end_time

            if used_start is not None or start is not None:
                result = max_date(used_start, start)
                used_start = result.isoformat() if result else None

            if used_end is not None or end is not None:
                result = min_date(used_end, end)
                used_end = result.isoformat() if result else None

            used_instrument_ids = get_instrument_ids(config)
            used_bar_types = []

            if config.data_type == Bar:
                if config.bar_types is None and config.instrument_ids is None:
                    assert config.instrument_id, "No `instrument_id` for Bar data config"
                    assert config.bar_spec, "No `bar_spec` for Bar data config"

                if config.instrument_id is not None and config.bar_spec is not None:
                    bar_type = f"{config.instrument_id}-{config.bar_spec}-EXTERNAL"
                    used_bar_types = [bar_type]
                elif config.bar_types is not None:
                    used_bar_types = config.bar_types
                elif config.instrument_ids is not None and config.bar_spec is not None:
                    for instrument_id in config.instrument_ids:
                        used_bar_types.append(f"{instrument_id}-{config.bar_spec}-EXTERNAL")

            session = catalog.backend_session(
                data_cls=config.data_type,
                instrument_ids=used_instrument_ids,
                bar_types=(used_bar_types if len(used_bar_types) > 0 else None),
                start=used_start,
                end=used_end,
                session=session,
            )

        # Stream data
        for chunk in session.to_query_result():
            engine.add_data(
                data=capsule_to_list(chunk),
                validate=False,  # Cannot validate mixed type stream
                sort=True,  # Already sorted from backend
            )
            engine.run(
                start=start,
                end=end,
                run_config_id=run_config_id,
                streaming=True,
            )
            engine.clear_data()

        engine.end()

    def _run_oneshot(
        self,
        run_config_id: str,
        engine: BacktestEngine,
        data_configs: list[BacktestDataConfig],
        start: str | int | None = None,
        end: str | int | None = None,
    ) -> None:
        # Load data
        for config in data_configs:
            t0 = pd.Timestamp.now()
            used_instrument_ids = get_instrument_ids(config)
            engine.logger.info(
                f"Reading {config.data_type} data for instrument_ids={used_instrument_ids}.",
            )
            result: CatalogDataResult = self.load_data_config(config, start, end)

            if len(used_instrument_ids) > 0 and result.instruments is None:
                engine.logger.warning(
                    f"Requested instrument_ids={used_instrument_ids} from data_config not found in catalog",
                )
                continue

            if not result.data:
                engine.logger.warning(f"No data found for {config}")
                continue

            t1 = pd.Timestamp.now()
            engine.logger.info(
                f"Read {len(result.data):,} events from parquet in {pd.Timedelta(t1 - t0)}s",
            )

            self._load_engine_data(engine=engine, result=result)

            t2 = pd.Timestamp.now()
            engine.logger.info(f"Engine load took {pd.Timedelta(t2 - t1)}s")

        engine.run(start=start, end=end, run_config_id=run_config_id)

    @classmethod
    def load_data_config(
        cls,
        config: BacktestDataConfig,
        start: str | int | None = None,
        end: str | int | None = None,
    ) -> CatalogDataResult:
        catalog: ParquetDataCatalog = cls.load_catalog(config)
        used_instrument_ids = get_instrument_ids(config)

        instruments = (
            catalog.instruments(instrument_ids=used_instrument_ids)
            if len(used_instrument_ids) > 0
            else None
        )

        if len(used_instrument_ids) > 0 and not instruments:
            return CatalogDataResult(data_cls=config.data_type, data=[])

        config_query = config.query

        if config_query["start"] is not None or start is not None:
            result = max_date(config_query["start"], start)
            config_query["start"] = result.isoformat() if result else None

        if config_query["end"] is not None or end is not None:
            result = min_date(config_query["end"], end)
            config_query["end"] = result.isoformat() if result else None

        return CatalogDataResult(
            data_cls=config.data_type,
            data=catalog.query(**config_query),
            instruments=instruments,
            client_id=ClientId(config.client_id) if config.client_id else None,
        )

    @classmethod
    def load_catalog(cls, config: BacktestDataConfig) -> ParquetDataCatalog:
        return ParquetDataCatalog(
            path=config.catalog_path,
            fs_protocol=config.catalog_fs_protocol,
            fs_storage_options=config.catalog_fs_storage_options,
        )

    def _load_engine_data(self, engine: BacktestEngine, result: CatalogDataResult) -> None:
        if is_nautilus_class(result.data_cls):
            engine.add_data(
                data=result.data,
                sort=True,  # Already sorted from backend
            )
        else:
            if not result.client_id:
                raise ValueError(
                    f"Data type {result.data_cls} not setup for loading into `BacktestEngine`",
                )

            engine.add_data(
                data=result.data,
                client_id=result.client_id,
                sort=True,  # Already sorted from backend
            )


def get_instrument_ids(config: BacktestDataConfig) -> list[InstrumentId]:
    instrument_ids = []

    if config.instrument_id:
        instrument_id = (
            InstrumentId.from_str(config.instrument_id)
            if type(config.instrument_id) is str
            else config.instrument_id
        )
        instrument_ids = [instrument_id]
    elif config.instrument_ids:
        instrument_ids = [
            (InstrumentId.from_str(instrument_id) if type(instrument_id) is str else instrument_id)
            for instrument_id in config.instrument_ids
        ]
    elif config.bar_types:
        bar_types: list[BarType] = [
            BarType.from_str(bar_type) if type(bar_type) is str else bar_type
            for bar_type in config.bar_types
        ]
        instrument_ids = [bar_type.instrument_id for bar_type in bar_types]

    return instrument_ids


def get_starting_balances(config: BacktestVenueConfig) -> list[Money]:
    starting_balances = []

    for balance in config.starting_balances:
        starting_balances.append(Money.from_str(balance) if type(balance) is str else balance)

    return starting_balances


def get_base_currency(config: BacktestVenueConfig) -> Currency | None:
    base_currency = config.base_currency

    return Currency.from_str(base_currency) if type(base_currency) is str else base_currency


def get_leverages(config: BacktestVenueConfig) -> dict[InstrumentId, Decimal]:
    return (
        {InstrumentId.from_str(i): Decimal(v) for i, v in config.leverages.items()}
        if config.leverages
        else {}
    )
