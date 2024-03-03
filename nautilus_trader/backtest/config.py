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

from __future__ import annotations

import sys
from typing import Any

import pandas as pd

from nautilus_trader.common import Environment
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import resolve_path
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.model.data import Bar
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.risk.config import RiskEngineConfig
from nautilus_trader.system.config import NautilusKernelConfig


def parse_filters_expr(s: str | None):
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
        return eval(code, {}, allowed_names)  # noqa

    return safer_eval(s)  # Only allow use of the field object


class BacktestVenueConfig(NautilusConfig, frozen=True):
    """
    Represents a venue configuration for one specific backtest engine.
    """

    name: str
    oms_type: str
    account_type: str
    starting_balances: list[str]
    base_currency: str | None = None
    default_leverage: float = 1.0
    leverages: dict[str, float] | None = None
    book_type: str = "L1_MBP"
    routing: bool = False
    frozen_account: bool = False
    bar_execution: bool = True
    reject_stop_orders: bool = True
    support_gtd_orders: bool = True
    support_contingent_orders: bool = True
    use_position_ids: bool = True
    use_random_ids: bool = False
    use_reduce_only: bool = True
    # fill_model: FillModel | None = None  # TODO(cs): Implement
    modules: list[ImportableActorConfig] | None = None


class BacktestDataConfig(NautilusConfig, frozen=True):
    """
    Represents the data configuration for one specific backtest run.
    """

    catalog_path: str
    data_cls: str
    catalog_fs_protocol: str | None = None
    catalog_fs_storage_options: dict | None = None
    instrument_id: InstrumentId | None = None
    start_time: str | int | None = None
    end_time: str | int | None = None
    filter_expr: str | None = None
    client_id: str | None = None
    metadata: dict | None = None
    bar_spec: str | None = None
    batch_size: int | None = 10_000

    @property
    def data_type(self) -> type:
        """
        Return a `type` for the specified `data_cls` for the configuration.

        Returns
        -------
        type

        """
        if isinstance(self.data_cls, str):
            return resolve_path(self.data_cls)
        else:
            return self.data_cls

    @property
    def query(self) -> dict[str, Any]:
        """
        Return a catalog query object for the configuration.

        Returns
        -------
        dict[str, Any]

        """
        if self.data_cls is Bar and self.bar_spec:
            bar_type = f"{self.instrument_id}-{self.bar_spec}-EXTERNAL"
            filter_expr: str | None = f'field("bar_type") == "{bar_type}"'
        else:
            filter_expr = self.filter_expr

        return {
            "data_cls": self.data_type,
            "instrument_ids": [self.instrument_id] if self.instrument_id else None,
            "start": self.start_time,
            "end": self.end_time,
            "filter_expr": parse_filters_expr(filter_expr),
            "metadata": self.metadata,
        }

    @property
    def start_time_nanos(self) -> int:
        """
        Return the data configuration start time in UNIX nanoseconds.

        Will be zero if no `start_time` was specified.

        Returns
        -------
        int

        """
        if self.start_time is None:
            return 0
        return dt_to_unix_nanos(self.start_time)

    @property
    def end_time_nanos(self) -> int:
        """
        Return the data configuration end time in UNIX nanoseconds.

        Will be sys.maxsize if no `end_time` was specified.

        Returns
        -------
        int

        """
        if self.end_time is None:
            return sys.maxsize
        return dt_to_unix_nanos(self.end_time)


class BacktestEngineConfig(NautilusKernelConfig, frozen=True):
    """
    Configuration for ``BacktestEngine`` instances.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the node (must be a name and ID tag separated by a hyphen).
    log_level : str, default "INFO"
        The stdout log level for the node.
    loop_debug : bool, default False
        If the asyncio event loop should be in debug mode.
    cache : CacheConfig, optional
        The cache configuration.
    data_engine : DataEngineConfig, optional
        The live data engine configuration.
    risk_engine : RiskEngineConfig, optional
        The live risk engine configuration.
    exec_engine : ExecEngineConfig, optional
        The live execution engine configuration.
    streaming : StreamingConfig, optional
        The configuration for streaming to feather files.
    strategies : list[ImportableStrategyConfig]
        The strategy configurations for the kernel.
    actors : list[ImportableActorConfig]
        The actor configurations for the kernel.
    exec_algorithms : list[ImportableExecAlgorithmConfig]
        The execution algorithm configurations for the kernel.
    controller : ImportableControllerConfig, optional
        The trader controller for the kernel.
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
    trader_id: TraderId = TraderId("BACKTESTER-001")
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
    venues : list[BacktestVenueConfig]
        The venue configurations for the backtest run.
        A valid configuration must include at least one venue config.
    data : list[BacktestDataConfig]
        The data configurations for the backtest run.
        A valid configuration must include at least one data config.
    engine : BacktestEngineConfig
        The backtest engine configuration (the core system kernel).
    batch_size_bytes : optional
        The batch block size in bytes (will then run in streaming mode).

    """

    venues: list[BacktestVenueConfig]
    data: list[BacktestDataConfig]
    engine: BacktestEngineConfig | None = None
    batch_size_bytes: int | None = None


class SimulationModuleConfig(ActorConfig, frozen=True):
    """
    Configuration for ``SimulationModule`` instances.
    """


class FXRolloverInterestConfig(SimulationModuleConfig, frozen=True):
    """
    Provides an FX rollover interest simulation module.

    Parameters
    ----------
    rate_data : pd.DataFrame
        The interest rate data for the internal rollover interest calculator.

    """

    rate_data: pd.DataFrame  # TODO(cs): This could probably just become JSON data
