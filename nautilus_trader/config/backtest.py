# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import hashlib
import importlib
import sys
from decimal import Decimal
from typing import Callable, Optional, Union

import msgspec
import pandas as pd

from nautilus_trader.common import Environment
from nautilus_trader.config.common import DataEngineConfig
from nautilus_trader.config.common import ExecEngineConfig
from nautilus_trader.config.common import ImportableConfig
from nautilus_trader.config.common import NautilusConfig
from nautilus_trader.config.common import NautilusKernelConfig
from nautilus_trader.config.common import RiskEngineConfig
from nautilus_trader.core.datetime import maybe_dt_to_unix_nanos
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.identifiers import ClientId


class BacktestVenueConfig(NautilusConfig, frozen=True):
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
    bar_execution: bool = True
    reject_stop_orders: bool = True
    # fill_model: Optional[FillModel] = None  # TODO(cs): Implement
    modules: Optional[list[ImportableConfig]] = None


class BacktestDataConfig(NautilusConfig, frozen=True):
    """
    Represents the data configuration for one specific backtest run.
    """

    catalog_path: str
    data_cls: str
    catalog_fs_protocol: Optional[str] = None
    catalog_fs_storage_options: Optional[dict] = None
    instrument_id: Optional[str] = None
    start_time: Optional[Union[str, int]] = None
    end_time: Optional[Union[str, int]] = None
    filter_expr: Optional[str] = None
    client_id: Optional[str] = None
    metadata: Optional[dict] = None
    bar_spec: Optional[str] = None
    use_rust: Optional[bool] = False
    batch_size: Optional[int] = 10_000

    @property
    def data_type(self):
        if isinstance(self.data_cls, str):
            mod_path, cls_name = self.data_cls.rsplit(":", maxsplit=1)
            mod = importlib.import_module(mod_path)
            return getattr(mod, cls_name)
        else:
            return self.data_cls

    @property
    def query(self):
        if self.data_cls is Bar and self.bar_spec:
            bar_type = f"{self.instrument_id}-{self.bar_spec}-EXTERNAL"
            filter_expr = f'field("bar_type") == "{bar_type}"'
        else:
            filter_expr = self.filter_expr

        return {
            "cls": self.data_type,
            "instrument_ids": [self.instrument_id] if self.instrument_id else None,
            "start": self.start_time,
            "end": self.end_time,
            "filter_expr": parse_filters_expr(filter_expr),
            "as_nautilus": True,
            "metadata": self.metadata,
            "use_rust": self.use_rust,
        }

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
        as_nautilus: bool = True,
    ):
        query = self.query
        query.update(
            {
                "start": start_time or query["start"],
                "end": end_time or query["end"],
                "as_nautilus": as_nautilus,
            },
        )

        catalog = self.catalog()
        instruments = catalog.instruments(
            instrument_ids=[self.instrument_id] if self.instrument_id else None,
            as_nautilus=True,
        )
        if not instruments:
            return {"data": [], "instrument": None}
        data = catalog.query(**query)
        return {
            "type": query["cls"],
            "data": data,
            "instrument": instruments[0] if self.instrument_id else None,
            "client_id": ClientId(self.client_id) if self.client_id else None,
        }


class BacktestEngineConfig(NautilusKernelConfig, frozen=True):
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


class BacktestRunConfig(NautilusConfig, frozen=True):
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
        return tokenize_config(self.dict())


def parse_filters_expr(s: Optional[str]):
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
        return eval(code, {}, allowed_names)

    return safer_eval(s)  # Only allow use of the field object


CUSTOM_ENCODINGS: dict[type, Callable] = {
    pd.DataFrame: lambda x: x.to_json(),
}


def json_encoder(x):
    if isinstance(x, (str, Decimal)):
        return str(x)
    elif isinstance(x, type) and hasattr(x, "fully_qualified_name"):
        return x.fully_qualified_name()
    elif type(x) in CUSTOM_ENCODINGS:
        func = CUSTOM_ENCODINGS[type(x)]
        return func(x)
    raise TypeError(f"Objects of type {type(x)} are not supported")


def register_json_encoding(type_: type, encoder: Callable) -> None:
    global CUSTOM_ENCODINGS
    CUSTOM_ENCODINGS[type_] = encoder


def tokenize_config(obj: dict) -> str:
    value: bytes = msgspec.json.encode(obj, enc_hook=json_encoder)
    return hashlib.sha256(value).hexdigest()
