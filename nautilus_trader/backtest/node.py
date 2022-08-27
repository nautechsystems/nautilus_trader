# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Dict, List, Optional

import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookTypeParser
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.batching import batch_files
from nautilus_trader.persistence.batching import extract_generic_data_client_ids
from nautilus_trader.persistence.batching import groupby_datatype
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog


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

    def __init__(self, configs: List[BacktestRunConfig]):
        PyCondition.not_none(configs, "configs")
        PyCondition.not_empty(configs, "configs")
        PyCondition.list_type(configs, BacktestRunConfig, "configs")

        self._validate_configs(configs)

        # Configuration
        self._configs: List[BacktestRunConfig] = configs
        self._engines: Dict[str, BacktestEngine] = {}

    @property
    def configs(self) -> List[BacktestRunConfig]:
        """
        Return the loaded backtest run configs for the node.

        Returns
        -------
        list[BacktestRunConfig]

        """
        return self._configs

    def get_engine(self, run_config_id: str) -> Optional[BacktestEngine]:
        """
        Return the backtest engine associated with the given run config ID
        (if found).

        Parameters
        ----------
        run_config_id : str
            The run configuration ID for the created engine.

        Returns
        -------
        BacktestEngine or ``None``

        """
        return self._engines.get(run_config_id)

    def get_engines(self) -> List[BacktestEngine]:
        """
        Return all backtest engines created by the node.

        Returns
        -------
        list[BacktestEngine]

        """
        return list(self._engines.values())

    def run(self) -> List[BacktestResult]:  # noqa (kwargs for extensibility)
        """
        Execute a group of backtest run configs synchronously.

        Returns
        -------
        list[BacktestResult]
            The results of the backtest runs.

        """
        results: List[BacktestResult] = []
        for config in self._configs:
            config.check()  # Check all values set
            result = self._run(
                run_config_id=config.id,
                engine_config=config.engine,
                venue_configs=config.venues,
                data_configs=config.data,
                batch_size_bytes=config.batch_size_bytes,
            )
            results.append(result)

        return results

    def _validate_configs(self, configs: List[BacktestRunConfig]):
        venue_ids: List[Venue] = []
        for config in configs:
            venue_ids += [Venue(c.name) for c in config.venues]

        for config in configs:
            for data_config in config.data:
                if data_config.instrument_id is None:
                    continue  # No instrument associated with data
                instrument_id: InstrumentId = InstrumentId.from_str(data_config.instrument_id)
                if instrument_id.venue not in venue_ids:
                    raise ValueError(
                        f"Venue '{instrument_id.venue}' for {instrument_id} "
                        f"does not have a `BacktestVenueConfig`.",
                    )

    def _create_engine(
        self,
        run_config_id: str,
        config: BacktestEngineConfig,
        venue_configs: List[BacktestVenueConfig],
        data_configs: List[BacktestDataConfig],
    ) -> BacktestEngine:
        # Build the backtest engine
        engine = BacktestEngine(config=config)
        self._engines[run_config_id] = engine

        # Add venues (must be added prior to instruments)
        for config in venue_configs:
            base_currency: Optional[str] = config.base_currency
            engine.add_venue(
                venue=Venue(config.name),
                oms_type=OMSType[config.oms_type],
                account_type=AccountType[config.account_type],
                base_currency=Currency.from_str(base_currency) if base_currency else None,
                starting_balances=[Money.from_str(m) for m in config.starting_balances],
                default_leverage=Decimal(config.default_leverage),
                leverages={
                    InstrumentId.from_str(i): Decimal(v) for i, v in config.leverages.items()
                }
                if config.leverages
                else {},
                book_type=BookTypeParser.from_str_py(config.book_type),
                routing=config.routing,
                frozen_account=config.frozen_account,
                reject_stop_orders=config.reject_stop_orders,
            )

        # Add instruments
        for config in data_configs:
            if is_nautilus_class(config.data_type):
                instruments = config.catalog().instruments(
                    instrument_ids=config.instrument_id,
                    as_nautilus=True,
                )
                for instrument in instruments or []:
                    if instrument.id not in engine.cache.instrument_ids():
                        engine.add_instrument(instrument)

        return engine

    def _load_engine_data(self, engine: BacktestEngine, data) -> None:
        if is_nautilus_class(data["type"]):
            engine.add_data(data=data["data"])
        else:
            if "client_id" not in data:
                raise ValueError(
                    f"Data type {data['type']} not setup for loading into backtest engine"
                )
            engine.add_data(data=data["data"], client_id=data["client_id"])

    def _run(
        self,
        run_config_id: str,
        engine_config: BacktestEngineConfig,
        venue_configs: List[BacktestVenueConfig],
        data_configs: List[BacktestDataConfig],
        batch_size_bytes: Optional[int] = None,
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

        return engine.get_result()

    def _run_streaming(
        self,
        run_config_id: str,
        engine: BacktestEngine,
        data_configs: List[BacktestDataConfig],
        batch_size_bytes: int,
    ) -> None:
        config = data_configs[0]
        catalog: ParquetDataCatalog = config.catalog()

        data_client_ids = extract_generic_data_client_ids(data_configs=data_configs)

        for batch in batch_files(
            catalog=catalog,
            data_configs=data_configs,
            target_batch_size_bytes=batch_size_bytes,
        ):
            engine.clear_data()
            grouped = groupby_datatype(batch)
            for data in grouped:
                if data["type"] in data_client_ids:
                    # Generic data - manually re-add client_id as it gets lost in the streaming join
                    data.update({"client_id": ClientId(data_client_ids[data["type"]])})
                    data["data"] = [
                        GenericData(data_type=DataType(data["type"]), data=d) for d in data["data"]
                    ]
                self._load_engine_data(engine=engine, data=data)
            engine.run_streaming(run_config_id=run_config_id)

        engine.end_streaming()

    def _run_oneshot(
        self,
        run_config_id: str,
        engine: BacktestEngine,
        data_configs: List[BacktestDataConfig],
    ) -> None:
        # Load data
        for config in data_configs:
            t0 = pd.Timestamp.now()
            engine._log.info(
                f"Reading {config.data_type} data for instrument={config.instrument_id}."
            )
            d = config.load()
            if config.instrument_id and d["instrument"] is None:
                print(
                    f"Requested instrument_id={d['instrument']} from data_config not found catalog"
                )
                continue
            if not d["data"]:
                print(f"No data found for {config}")
                continue

            t1 = pd.Timestamp.now()
            engine._log.info(
                f"Read {len(d['data']):,} events from parquet in {pd.Timedelta(t1 - t0)}s."
            )
            self._load_engine_data(engine=engine, data=d)
            t2 = pd.Timestamp.now()
            engine._log.info(f"Engine load took {pd.Timedelta(t2 - t1)}s")

        engine.run(run_config_id=run_config_id)

    def dispose(self):
        for engine in self.get_engines():
            engine.dispose()
