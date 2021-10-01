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

import dataclasses
import importlib
from datetime import datetime
from typing import Dict, List, Optional, Union

import pydantic
from dask.base import tokenize

from nautilus_trader.cache.cache import CacheConfig
from nautilus_trader.data.engine import DataEngineConfig
from nautilus_trader.execution.engine import ExecEngineConfig
from nautilus_trader.infrastructure.cache import CacheDatabaseConfig
from nautilus_trader.risk.engine import RiskEngineConfig
from nautilus_trader.trading.config import ImportableStrategyConfig


class Partialable:
    """
    The abstract base class for all partialable configurations.
    """

    def fields(self) -> Dict[str, dataclasses.Field]:
        return {field.name: field for field in dataclasses.fields(self)}

    def missing(self):
        return [x for x in self.fields() if getattr(self, x) is None]

    def optional_fields(self):
        for field in self.fields().values():
            if (
                hasattr(field.type, "__args__")
                and len(field.type.__args__) == 2
                and field.type.__args__[-1] is type(None)  # noqa: E721
            ):
                # Check if exactly two arguments exists and one of them are None type
                yield field.name

    def is_partial(self):
        return any(self.missing())

    def check(self, ignore=None):
        optional = tuple(self.optional_fields())
        missing = [
            name for name in self.missing() if not (name in (ignore or {}) or name in optional)
        ]
        if missing:
            raise AssertionError(f"Missing fields: {missing}")

    def _check_kwargs(self, kw):
        for k in kw:
            assert k in self.fields(), f"Unknown kwarg: {k}"

    def update(self, **kwargs):
        """Update attributes on this instance."""
        self._check_kwargs(kwargs)
        self.__dict__.update(kwargs)
        return self

    def replace(self, **kwargs):
        """Return a new instance with some attributes replaced."""
        return self.__class__(**{**{k: getattr(self, k) for k in self.fields()}, **kwargs})

    def __dask_tokenize__(self):
        return tuple(self.fields())

    def __repr__(self):
        dataclass_repr_func = dataclasses._repr_fn(
            fields=list(self.fields().values()), globals=self.__dict__
        )
        r = dataclass_repr_func(self)
        if self.missing():
            return "Partial-" + r
        return r


@pydantic.dataclasses.dataclass()
class BacktestVenueConfig(Partialable):
    """
    Represents the venue configuration for one specific backtest engine.
    """

    name: str
    venue_type: str
    oms_type: str
    account_type: str
    base_currency: Optional[str]
    starting_balances: List[str]
    # fill_model: Optional[FillModel] = None  # TODO(cs): Implement next iteration
    # modules: Optional[List[SimulationModule]] = None  # TODO(cs): Implement next iteration

    def __dask_tokenize__(self):
        values = [
            self.name,
            self.venue_type,
            self.oms_type,
            self.account_type,
            self.base_currency,
            ",".join(sorted([b for b in self.starting_balances])),
            # self.modules,  # TODO(cs): Implement next iteration
        ]
        return tuple(values)


@pydantic.dataclasses.dataclass()
class BacktestDataConfig(Partialable):
    """
    Represents the data configuration for one specific backtest run.
    """

    catalog_path: str
    data_cls_path: str
    catalog_fs_protocol: str = None
    catalog_fs_storage_options: Optional[Dict] = None
    instrument_id: Optional[str] = None
    start_time: Optional[Union[datetime, str, int]] = None
    end_time: Optional[Union[datetime, str, int]] = None
    filters: Optional[dict] = None
    client_id: Optional[str] = None

    @property
    def data_type(self):
        mod_path, cls_name = self.data_cls_path.rsplit(".", maxsplit=1)
        mod = importlib.import_module(mod_path)
        return getattr(mod, cls_name)

    @property
    def query(self):
        return dict(
            cls=self.data_type,
            instrument_ids=[self.instrument_id] if self.instrument_id else None,
            start=self.start_time,
            end=self.end_time,
            as_nautilus=True,
        )

    def catalog(self):
        from nautilus_trader.persistence.catalog import DataCatalog

        return DataCatalog(
            path=self.catalog_path,
            fs_protocol=self.catalog_fs_protocol,
            fs_storage_options=self.catalog_fs_storage_options,
        )

    def load(self, start_time=None, end_time=None):
        query = self.query
        query.update(
            {
                "start": start_time or query["start"],
                "end": end_time or query["end"],
            }
        )

        catalog = self.catalog()
        return {
            "type": query["cls"],
            "data": catalog.query(**query),
            "instrument": catalog.instruments(instrument_ids=self.instrument_id, as_nautilus=True)[
                0
            ],
            "client_id": self.client_id,
        }


class BacktestEngineConfig(pydantic.BaseModel):
    """
    Configuration for ``BacktestEngine`` instances.

    trader_id : str, default="BACKTESTER-000"
        The trader ID.
    log_level : str, default="INFO"
        The minimum log level for logging messages to stdout.
    cache : CacheConfig, optional
        The configuration for the cache.
    cache_database : CacheDatabaseConfig, optional
        The configuration for the cache database.
    data_engine : DataEngineConfig, optional
        The configuration for the data engine.
    risk_engine : RiskEngineConfig, optional
        The configuration for the risk engine.
    exec_engine : ExecEngineConfig, optional
        The configuration for the execution engine.
    bypass_logging : bool, default=False
        If logging should be bypassed.
    run_analysis : bool, default=True
        If post backtest performance analysis should be run.

    """

    trader_id: str = "BACKTESTER-000"
    log_level: str = "INFO"
    cache: Optional[CacheConfig] = None
    cache_database: Optional[CacheDatabaseConfig] = None
    data_engine: Optional[DataEngineConfig] = None
    risk_engine: Optional[RiskEngineConfig] = None
    exec_engine: Optional[ExecEngineConfig] = None
    bypass_logging: bool = False
    run_analysis: bool = True

    def __dask_tokenize__(self):
        return tuple(self.dict().items())


@pydantic.dataclasses.dataclass()
class BacktestRunConfig(Partialable):
    """
    Represents the configuration for one specific backtest run (a single set of
    data / strategies / parameters).
    """

    engine: Optional[BacktestEngineConfig] = None
    venues: Optional[List[BacktestVenueConfig]] = None
    data: Optional[List[BacktestDataConfig]] = None
    strategies: Optional[List[ImportableStrategyConfig]] = None
    batch_size_bytes: Optional[int] = None  # TODO(cs): Useful for cached batches

    @property
    def id(self):
        return tokenize(self)
