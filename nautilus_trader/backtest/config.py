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

from __future__ import annotations

import sys
from typing import Any

import msgspec
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
from nautilus_trader.model.data import BarType
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
        return None

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

    Parameters
    ----------
    name : str
        The name of the venue.
    oms_type : str
        The order management system type for the exchange. If ``HEDGING`` will
        generate new position IDs.
    account_type : str
        The account type for the exchange.
    starting_balances : list[Money | str]
        The starting account balances (specify one for a single asset account).
    base_currency : Currency | str, optional
        The account base currency for the exchange. Use ``None`` for multi-currency accounts.
    default_leverage : float, optional
        The account default leverage (for margin accounts).
    leverages : dict[str, float], optional
        The instrument specific leverage configuration (for margin accounts).
    book_type : str
        The default order book type.
    routing : bool, default False
        If multi-venue routing should be enabled for the execution client.
    frozen_account : bool, default False
        If the account for this exchange is frozen (balances will not change).
    reject_stop_orders : bool, default True
        If stop orders are rejected on submission if trigger price is in the market.
    support_gtd_orders : bool, default True
        If orders with GTD time in force will be supported by the venue.
    support_contingent_orders : bool, default True
        If contingent orders will be supported/respected by the venue.
        If False, then it's expected the strategy will be managing any contingent orders.
    use_position_ids : bool, default True
        If venue position IDs will be generated on order fills.
    use_random_ids : bool, default False
        If all venue generated identifiers will be random UUID4's.
    use_reduce_only : bool, default True
        If the `reduce_only` execution instruction on orders will be honored.
    bar_execution : bool, default True
        If bars should be processed by the matching engine(s) (and move the market).
    bar_adaptive_high_low_ordering : bool, default False
        Determines whether the processing order of bar prices is adaptive based on a heuristic.
        This setting is only relevant when `bar_execution` is True.
        If False, bar prices are always processed in the fixed order: Open, High, Low, Close.
        If True, the processing order adapts with the heuristic:
        - If High is closer to Open than Low then the processing order is Open, High, Low, Close.
        - If Low is closer to Open than High then the processing order is Open, Low, High, Close.
    trade_execution : bool, default False
        If trades should be processed by the matching engine(s) (and move the market).

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
    reject_stop_orders: bool = True
    support_gtd_orders: bool = True
    support_contingent_orders: bool = True
    use_position_ids: bool = True
    use_random_ids: bool = False
    use_reduce_only: bool = True
    bar_execution: bool = True
    bar_adaptive_high_low_ordering: bool = False
    trade_execution: bool = False
    # fill_model: FillModel | None = None  # TODO: Implement
    modules: list[ImportableActorConfig] | None = None


class BacktestDataConfig(NautilusConfig, frozen=True):
    """
    Represents the data configuration for one specific backtest run.

    Parameters
    ----------
    catalog_path : str
        The path to the data catalog.
    data_cls : str
        The data type for the configuration.
    catalog_fs_protocol : str, optional
        The `fsspec` filesystem protocol for the catalog.
    catalog_fs_storage_options : dict, optional
        The `fsspec` storage options.
    instrument_id : InstrumentId | str, optional
        The instrument ID for the data configuration.
    start_time : str or int, optional
        The start time for the data configuration.
        Can be an ISO 8601 format datetime string, or UNIX nanoseconds integer.
    end_time : str or int, optional
        The end time for the data configuration.
        Can be an ISO 8601 format datetime string, or UNIX nanoseconds integer.
    filter_expr : str, optional
        The additional filter expressions for the data catalog query.
    client_id : str, optional
        The client ID for the data configuration.
    metadata : dict, optional
        The metadata for the data catalog query.
    bar_spec : BarSpecification | str, optional
        The bar specification for the data catalog query.
    instrument_ids : list[InstrumentId | str], optional
        The instrument IDs for the data catalog query.
        Can be used if instrument_id is not specified.
        If bar_spec is specified an equivalent list of bar_types will be constructed.
    bar_types : list[BarType | str], optional
        The bar types for the data catalog query.
        Can be used if instrument_id is not specified.

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
    instrument_ids: list[str] | None = None
    bar_types: list[str] | None = None

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
    def query(self) -> dict[str, Any]:  # noqa: C901
        """
        Return a catalog query object for the configuration.

        Returns
        -------
        dict[str, Any]

        """
        filter_expr: str | None = None

        if self.data_cls is Bar:
            used_bar_types = []

            if self.bar_types is None and self.instrument_ids is None:
                assert self.instrument_id, "No `instrument_id` for Bar data config"
                assert self.bar_spec, "No `bar_spec` for Bar data config"

            if self.instrument_id is not None and self.bar_spec is not None:
                bar_type = f"{self.instrument_id}-{self.bar_spec}-EXTERNAL"
                used_bar_types = [bar_type]
            elif self.bar_types is not None:
                used_bar_types = self.bar_types
            elif self.instrument_ids is not None and self.bar_spec is not None:
                for instrument_id in self.instrument_ids:
                    used_bar_types.append(f"{instrument_id}-{self.bar_spec}-EXTERNAL")

            if len(used_bar_types) > 0:
                filter_expr = f'(field("bar_type") == "{used_bar_types[0]}")'

            for bar_type in used_bar_types[1:]:
                filter_expr = f'{filter_expr} | (field("bar_type") == "{bar_type}")'
        else:
            filter_expr = self.filter_expr

        used_instrument_ids = None

        if self.instrument_id is not None:
            used_instrument_ids = [self.instrument_id]
        elif self.instrument_ids is not None:
            used_instrument_ids = self.instrument_ids
        elif self.bar_types is not None:
            bar_types: list[BarType] = [
                BarType.from_str(bar_type) if type(bar_type) is str else bar_type
                for bar_type in self.bar_types
            ]
            used_instrument_ids = [bar_type.instrument_id for bar_type in bar_types]

        return {
            "data_cls": self.data_type,
            "instrument_ids": used_instrument_ids,
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
    trader_id: TraderId = "BACKTESTER-001"
    data_engine: DataEngineConfig = DataEngineConfig()
    risk_engine: RiskEngineConfig = RiskEngineConfig()
    exec_engine: ExecEngineConfig = ExecEngineConfig()
    run_analysis: bool = True

    def __post_init__(self):
        if isinstance(self.trader_id, str):
            msgspec.structs.force_setattr(self, "trader_id", TraderId(self.trader_id))


class BacktestRunConfig(NautilusConfig, frozen=True):
    """
    Represents the configuration for one specific backtest run.

    This includes a backtest engine with its actors and strategies, with the
    external inputs of venues and data.

    Parameters
    ----------
    venues : list[BacktestVenueConfig]
        The venue configurations for the backtest run.
    data : list[BacktestDataConfig]
        The data configurations for the backtest run.
    engine : BacktestEngineConfig
        The backtest engine configuration (the core system kernel).
    chunk_size : int, optional
        The number of data points to process in each chunk during streaming mode.
        If `None`, the backtest will run without streaming, loading all data at once.
    dispose_on_completion : bool, default True
        If the backtest engine should be disposed on completion of the run.
        If True, then will drop data and all state.
        If False, then will *only* drop data.
    start : datetime or str or int, optional
        The start datetime (UTC) for the backtest run.
        If ``None`` engine runs from the start of the data.
    end : datetime or str or int, optional
        The end datetime (UTC) for the backtest run.
        If ``None`` engine runs to the end of the data.

    Notes
    -----
    A valid backtest run configuration must include:
      - At least one `venues` config.
      - At least one `data` config.

    """

    venues: list[BacktestVenueConfig]
    data: list[BacktestDataConfig]
    engine: BacktestEngineConfig | None = None
    chunk_size: int | None = None
    dispose_on_completion: bool = True
    start: str | int | None = None
    end: str | int | None = None


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

    rate_data: pd.DataFrame  # TODO: This could probably just become JSON data
