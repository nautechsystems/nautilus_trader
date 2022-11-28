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

import dataclasses
import importlib
import sys
from datetime import datetime
from typing import Optional, Union

import pandas as pd
from pydantic import validator
from pydantic.fields import ModelField

from nautilus_trader.common import Environment
from nautilus_trader.config.common import DataEngineConfig
from nautilus_trader.config.common import ExecEngineConfig
from nautilus_trader.config.common import NautilusConfig
from nautilus_trader.config.common import NautilusKernelConfig
from nautilus_trader.config.common import RiskEngineConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import maybe_dt_to_unix_nanos
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.persistence.funcs import tokenize


class Partialable(NautilusConfig):
    """
    The abstract base class for all partialable configurations.
    """

    def fields(self) -> dict[str, ModelField]:
        return self.__fields__

    def missing(self):
        return [x for x in self.fields() if getattr(self, x) is None]

    def optional_fields(self):
        for field in self.fields().values():
            # https://stackoverflow.com/questions/56832881/check-if-a-field-is-typing-optional
            if (
                hasattr(field.annotation, "__args__")
                and len(field.annotation.__args__) == 2
                and field.annotation.__args__[-1] is type(None)  # noqa: E721
            ):
                # Check if exactly two arguments exists and one of them are None type
                yield field.name

    def is_partial(self):
        return any(self.missing())

    def check(self, ignore: Optional[dict] = None):
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

    def __repr__(self):  # Adding -> causes error: Module has no attribute "_repr_fn"
        dataclass_repr_func = dataclasses._repr_fn(
            fields=list(self.fields().values()),
            globals=self.__dict__,
        )
        r = dataclass_repr_func(self)
        if self.missing():
            return "Partial-" + r
        return r


class BacktestVenueConfig(Partialable):
    """
    Represents a venue configuration for one specific backtest engine.
    """

    name: str
    oms_type: str
    account_type: str
    starting_balances: list[str]
    base_currency: Optional[str] = None
    default_leverage: float = 1.0
    leverages: Optional[dict[str, float]] = None
    book_type: str = "L1_TBBO"
    routing: bool = False
    frozen_account: bool = False
    reject_stop_orders: bool = True
    # fill_model: Optional[FillModel] = None  # TODO(cs): Implement
    # modules: Optional[list[SimulationModule]] = None  # TODO(cs): Implement

    def __tokenize__(self):
        values = [
            self.name,
            self.oms_type,
            self.account_type,
            self.base_currency,
            ",".join(sorted([b for b in self.starting_balances])),
            self.default_leverage,
            self.leverages,
            self.book_type,
            self.routing,
            self.frozen_account,
            self.reject_stop_orders,
            # self.modules,  # TODO(cs): Implement
        ]
        return tuple(values)


class BacktestDataConfig(Partialable):
    """
    Represents the data configuration for one specific backtest run.
    """

    catalog_path: str
    data_cls: Optional[Union[type, str]] = None
    catalog_fs_protocol: Optional[str] = None
    catalog_fs_storage_options: Optional[dict] = None
    instrument_id: Optional[str] = None
    start_time: Optional[Union[datetime, str, int]] = None
    end_time: Optional[Union[datetime, str, int]] = None
    filter_expr: Optional[str] = None
    client_id: Optional[str] = None
    metadata: Optional[dict] = None

    @validator("data_cls")
    def data_cls_str(cls, v: Union[str, type]):
        if not isinstance(v, str):
            if not hasattr(v, Data.fully_qualified_name.__name__):
                raise TypeError(
                    f"`data_cls` is not a valid `Data` class, was {type(v)}",
                )
            return v.fully_qualified_name()  # type: ignore
        return v

    @property
    def data_type(self):
        mod_path, cls_name = self.data_cls.rsplit(":", maxsplit=1)
        mod = importlib.import_module(mod_path)
        return getattr(mod, cls_name)

    @property
    def query(self):
        return dict(
            cls=self.data_type,
            instrument_ids=[self.instrument_id] if self.instrument_id else None,
            start=self.start_time,
            end=self.end_time,
            filter_expr=self.filter_expr,
            as_nautilus=True,
        )

    @property
    def start_time_nanos(self) -> int:
        if self.start_time is None:
            return 0
        return maybe_dt_to_unix_nanos(pd.Timestamp(self.start_time))

    @property
    def end_time_nanos(self) -> int:
        if self.end_time is None:
            return sys.maxsize
        return maybe_dt_to_unix_nanos(pd.Timestamp(self.end_time))

    def catalog(self):
        from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog

        return ParquetDataCatalog(
            path=self.catalog_path,
            fs_protocol=self.catalog_fs_protocol,
            fs_storage_options=self.catalog_fs_storage_options,
        )

    def load(
        self,
        start_time: Optional[pd.Timestamp] = None,
        end_time: Optional[pd.Timestamp] = None,
    ):
        query = self.query
        query.update(
            {
                "start": start_time or query["start"],
                "end": end_time or query["end"],
                "filter_expr": parse_filters_expr(query.pop("filter_expr", "None")),
                "metadata": self.metadata,
            },
        )

        catalog = self.catalog()
        instruments = catalog.instruments(instrument_ids=self.instrument_id, as_nautilus=True)
        if not instruments:
            return {"data": [], "instrument": None}
        data = catalog.query(**query)
        return {
            "type": query["cls"],
            "data": data,
            "instrument": instruments[0] if self.instrument_id else None,
            "client_id": ClientId(self.client_id) if self.client_id else None,
        }


class BacktestEngineConfig(NautilusKernelConfig):
    """
    Configuration for ``BacktestEngine`` instances.

    Parameters
    ----------
    trader_id : str
        The trader ID for the node (must be a name and ID tag separated by a hyphen).
    log_level : str, default "INFO"
        The stdout log level for the node.
    loop_debug : bool, default False
        If the asyncio event loop should be in debug mode.
    cache : CacheConfig, optional
        The cache configuration.
    cache_database : CacheDatabaseConfig, optional
        The cache database configuration.
    data_engine : DataEngineConfig, optional
        The live data engine configuration.
    risk_engine : RiskEngineConfig, optional
        The live risk engine configuration.
    exec_engine : ExecEngineConfig, optional
        The live execution engine configuration.
    streaming : StreamingConfig, optional
        The configuration for streaming to feather files.
    strategies : list[ImportableStrategyConfig]
        The strategy configurations for the node.
    actors : list[ImportableActorConfig]
        The actor configurations for the node.
    load_state : bool, default True
        If trading strategy state should be loaded from the database on start.
    save_state : bool, default True
        If trading strategy state should be saved to the database on stop.
    bypass_logging : bool, default False
        If logging should be bypassed.
    run_analysis : bool, default True
        If post backtest performance analysis should be run.

    """

    environment: Environment = Environment.BACKTEST
    trader_id: str = "BACKTESTER-001"
    data_engine: DataEngineConfig = DataEngineConfig()
    risk_engine: RiskEngineConfig = RiskEngineConfig()
    exec_engine: ExecEngineConfig = ExecEngineConfig()
    run_analysis: bool = True

    def __tokenize__(self):
        return tuple(self.dict().items())


class BacktestRunConfig(Partialable):
    """
    Represents the configuration for one specific backtest run.

    This includes a backtest engine with its actors and strategies, with the
    external inputs of venues and data.

    Parameters
    ----------
    engine : BacktestEngineConfig, optional
        The backtest engine configuration (represents the core system kernel).
    venues : list[BacktestVenueConfig]
        The venue configurations for the backtest run.
    data : list[BacktestDataConfig]
        The data configurations for the backtest run.
    batch_size_bytes : optional
        The batch block size in bytes (will then run in streaming mode).
    """

    engine: Optional[BacktestEngineConfig] = None
    venues: Optional[list[BacktestVenueConfig]] = None
    data: Optional[list[BacktestDataConfig]] = None
    batch_size_bytes: Optional[int] = None

    @property
    def id(self):
        return tokenize(self)


def parse_filters_expr(s: str):
    # TODO (bm) - could we do this better, probably requires writing our own parser?
    """
    Parse a pyarrow.dataset filter expression from a string.

    >>> parse_filters_expr('field("Currency") == "CHF"')
    <pyarrow.dataset.Expression (Currency == "CHF")>

    >>> parse_filters_expr("print('hello')")

    >>> parse_filters_expr("None")

    """
    from pyarrow.dataset import field

    assert field  # Required for eval

    if not s:
        return

    def safer_eval(input_string):
        allowed_names = {"field": field}
        code = compile(input_string, "<string>", "eval")
        for name in code.co_names:
            if name not in allowed_names:
                raise NameError(f"Use of {name} not allowed")
        return eval(code, {}, allowed_names)  # noqa: S307

    return safer_eval(s)  # Only allow use of the field object
