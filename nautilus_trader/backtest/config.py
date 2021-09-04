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
from typing import Any, List, Optional, Tuple

import pydantic

from nautilus_trader.cache.cache import CacheConfig
from nautilus_trader.data.engine import DataEngineConfig
from nautilus_trader.execution.engine import ExecEngineConfig
from nautilus_trader.infrastructure.cache import CacheDatabaseConfig
from nautilus_trader.risk.engine import RiskEngineConfig
from nautilus_trader.trading.strategy import TradingStrategyConfig


class Partialable:
    """
    The abstract base class for all partialable configurations.
    """

    def missing(self):
        return [x for x in self.__dataclass_fields__ if getattr(self, x) is None]

    def is_partial(self):
        return any(self.missing())

    def check(self, ignore=None):
        missing = [m for m in self.missing() if m not in (ignore or {})]
        if missing:
            raise AssertionError(f"Missing fields: {missing}")

    def _check_kwargs(self, kw):
        for k in kw:
            assert k in self.__dataclass_fields__, f"Unknown kwarg: {k}"

    def update(self, **kwargs):
        """Update attributes on this instance."""
        self._check_kwargs(kwargs)
        self.__dict__.update(kwargs)
        return self

    def replace(self, **kwargs):
        """Return a new instance with some attributes replaces."""
        return self.__class__(
            **{**{k: getattr(self, k) for k in self.__dataclass_fields__}, **kwargs}
        )

    def __repr__(self):
        dataclass_repr_func = dataclasses._repr_fn(
            fields=list(self.__dataclass_fields__.values()), globals=self.__dict__
        )
        r = dataclass_repr_func(self)
        if self.missing():
            return "Partial-" + r
        return r


@pydantic.dataclasses.dataclass()
class BacktestDataConfig(Partialable):
    """
    Represents the data configuration for one specific backtest run.
    """

    catalog_path: str
    data_type: type
    catalog_fs_protocol: str = None
    instrument_id: Optional[str] = None
    start_time: Optional[int] = None
    end_time: Optional[int] = None
    filters: Optional[dict] = None
    client_id: Optional[str] = None

    @property
    def query(self):
        return dict(
            cls=self.data_type,
            instrument_ids=[self.instrument_id] if self.instrument_id else None,
            start=self.start_time,
            end=self.end_time,
            as_nautilus=True,
        )


@pydantic.dataclasses.dataclass()
class BacktestVenueConfig(Partialable):
    """
    Represents the venue configuration for one specific backtest engine.
    """

    name: str
    venue_type: str
    oms_type: str
    account_type: str
    base_currency: str
    starting_balances: List[str]
    # fill_model: Optional[FillModel] = None
    # modules: Optional[List[SimulationModule]] = None

    def __dask_tokenize__(self):
        values = [
            self.name,
            self.venue_type,
            self.oms_type,
            self.account_type,
            self.base_currency,
            ",".join(sorted([b for b in self.starting_balances])),
            # self.modules,
        ]
        return tuple(values)


class BacktestEngineConfig(pydantic.BaseModel):
    """
    Configuration for ``BacktestEngine`` instances.

    trader_id : str, default="BACKTESTER-000"
        The trader ID.
    log_level : str, default="INFO"
        The minimum log level for logging messages to stdout.
    cache : Optional[CacheConfig]
        The configuration for the cache.
    cache_database : Optional[CacheDatabaseConfig]
        The configuration for the cache database.
    data_engine : Optional[DataEngineConfig]
        The configuration for the data engine.
    risk_engine : Optional[RiskEngineConfig]
        The configuration for the risk engine.
    exec_engine : Optional[ExecEngineConfig]
        The configuration for the execution engine.
    use_data_cache : bool, default=False
        If use cache for DataProducer (increased performance with repeated backtests on same data).
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
    use_data_cache: bool = False
    bypass_logging: bool = False
    run_analysis: bool = True


@pydantic.dataclasses.dataclass()
class BacktestConfig(Partialable):
    """
    Represents the configuration for one specific backtest run (a single set of
    data / strategies / parameters).
    """

    venues: Optional[List[BacktestVenueConfig]] = None
    data_config: Optional[List[BacktestDataConfig]] = None
    engine_config: Optional[BacktestEngineConfig] = None
    strategies: Optional[List[Tuple[Any, TradingStrategyConfig]]] = None
    name: Optional[str] = None
    # data_catalog_path: Optional[str] = None
