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

from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.common import Environment
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import NonNegativeInt
from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.common.config import resolve_config_path
from nautilus_trader.common.config import resolve_path
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.live.config import LiveDataClientConfig
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.risk.config import RiskEngineConfig
from nautilus_trader.system.config import NautilusKernelConfig


def parse_filters_expr(s: str | None):
    """
    Parse a pyarrow.dataset filter expression from a string.

    >>> parse_filters_expr('field("Currency") == "CHF"')
    <pyarrow.dataset.Expression (Currency == "CHF")>

    >>> parse_filters_expr("print('hello')")

    >>> parse_filters_expr("None")

    """
    import re

    from pyarrow.dataset import field

    if not s:
        return None

    # Normalise single-quoted filters so our regex only has to reason about
    # the double-quoted form produced by Nautilus itself. If the expression
    # already contains double quotes we leave it unchanged to avoid corrupting
    # mixed quoting scenarios.
    if "'" in s and '"' not in s:
        s = s.replace("'", '"')

    # Security: Only allow very specific PyArrow field expressions
    # Pattern matches: field("name") == "value", field("name") != "value", etc.
    # Optional opening/closing parentheses are allowed around each comparison so
    # we can safely compose expressions such as
    #     (field("Currency") == "CHF") | (field("Symbol") == "USD")
    # Supported grammar (regex-validated):
    #     [ '(' ] field("name") <op> "literal" [ ')' ] ( ( '|' | '&' ) ... )*
    safe_pattern = (
        r"^(\()?"
        r'field\("[^"]+"\)\s*[!=<>]+\s*"[^"]*"'
        r"(\))?"
        r'(\s*[|&]\s*(\()?field\("[^"]+"\)\s*[!=<>]+\s*"[^"]*"(\))?)*$'
    )

    if not re.match(safe_pattern, s.strip()):
        raise ValueError(
            f"Filter expression '{s}' is not allowed. Only field() comparisons are permitted.",
        )

    try:
        # For now, rely on the regex validation above to guarantee safety and
        # evaluate the expression in a minimal global namespace that only exposes
        # the `field` helper. Built-ins are intentionally left untouched because
        # PyArrow requires access to them (for example it imports `decimal` under
        # the hood). Stripping them leads to a hard crash inside the C++ layer
        # of Arrow. The expression is still safe because the regex prevents any
        # reference other than the allowed `field(...)` comparisons.
        allowed_globals = {"field": field}
        return eval(s, allowed_globals, {})  # noqa: S307

    except Exception as e:
        raise ValueError(f"Failed to parse filter expression '{s}': {e}")


class BacktestVenueConfig(NautilusConfig, frozen=True):
    """
    Represents a venue configuration for one specific backtest engine.

    Parameters
    ----------
    name : str
        The name of the venue.
    oms_type : OmsType | str
        The order management system type for the exchange. If ``HEDGING`` will
        generate new position IDs.
    account_type : AccountType | str
        The account type for the exchange.
    starting_balances : list[Money | str]
        The starting account balances (specify one for a single asset account).
    base_currency : Currency | str, optional
        The account base currency for the exchange. Use ``None`` for multi-currency accounts.
    default_leverage : float, optional
        The account default leverage (for margin accounts).
    leverages : dict[str, float], optional
        The instrument specific leverage configuration (for margin accounts).
    margin_model : MarginModelConfig, optional
        The margin calculation model configuration. Default 'leveraged'.
    modules : list[ImportableActorConfig], optional
        The simulation modules for the venue.
    fill_model : ImportableFillModelConfig, optional
        The fill model for the venue.
    latency_model : ImportableLatencyModelConfig, optional
        The latency model for the venue.
    fee_model : ImportableFeeModelConfig, optional
        The fee model for the venue.
    book_type : str, default 'L1_MBP'
        The default order book type.
    routing : bool, default False
        If multi-venue routing should be enabled for the execution client.
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
    allow_cash_borrowing : bool, default False
        If borrowing is allowed for cash accounts (negative balances).
    frozen_account : bool, default False
        If the account for this exchange is frozen (balances will not change).

    """

    name: str
    oms_type: OmsType | str
    account_type: AccountType | str
    starting_balances: list[str]
    base_currency: str | None = None
    default_leverage: float = 1.0
    leverages: dict[str, float] | None = None
    margin_model: MarginModelConfig | None = None
    modules: list[ImportableActorConfig] | None = None
    fill_model: ImportableFillModelConfig | None = None
    latency_model: ImportableLatencyModelConfig | None = None
    fee_model: ImportableFeeModelConfig | None = None
    book_type: BookType | str = "L1_MBP"
    routing: bool = False
    reject_stop_orders: bool = True
    support_gtd_orders: bool = True
    support_contingent_orders: bool = True
    use_position_ids: bool = True
    use_random_ids: bool = False
    use_reduce_only: bool = True
    bar_execution: bool = True
    bar_adaptive_high_low_ordering: bool = False
    trade_execution: bool = False
    allow_cash_borrowing: bool = False
    frozen_account: bool = False


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
    catalog_fs_rust_storage_options : dict, optional
        The `fsspec` storage options for the Rust backend.
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
    metadata : dict or callable, optional
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
    catalog_fs_rust_storage_options: dict | None = None
    instrument_id: InstrumentId | None = None
    start_time: str | int | None = None
    end_time: str | int | None = None
    filter_expr: str | None = None
    client_id: str | None = None
    metadata: dict | Any | None = None
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

        used_identifiers = None

        if self.instrument_id is not None:
            used_identifiers = [self.instrument_id]
        elif self.instrument_ids is not None:
            used_identifiers = self.instrument_ids
        elif self.bar_types is not None:
            bar_types: list[BarType] = [
                BarType.from_str(bar_type) if type(bar_type) is str else bar_type
                for bar_type in self.bar_types
            ]
            used_identifiers = [bar_type.instrument_id for bar_type in bar_types]

        return {
            "data_cls": self.data_type,
            "identifiers": used_identifiers,
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
    cache: CacheConfig | None = CacheConfig(drop_instruments_on_reset=False)
    data_engine: DataEngineConfig | None = DataEngineConfig()
    risk_engine: RiskEngineConfig | None = RiskEngineConfig()
    exec_engine: ExecEngineConfig | None = ExecEngineConfig()
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
    raise_exception : bool, default False
        If exceptions during an engine build or run should be raised to interrupt the nodes process.
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
    data_clients : dict[str, type[LiveDataClientConfig]], optional
        The data clients configuration for the backtest run.

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
    raise_exception: bool = False
    dispose_on_completion: bool = True
    start: str | int | None = None
    end: str | int | None = None
    data_clients: dict[str, type[LiveDataClientConfig]] | None = None


class SimulationModuleConfig(ActorConfig, frozen=True):
    """
    Configuration for ``SimulationModule`` instances.
    """


class FillModelConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``FillModel`` instances.

    Parameters
    ----------
    prob_fill_on_limit : float, default 1.0
        The probability of limit order filling if the market rests on its price.
    prob_fill_on_stop : float, default 1.0
        The probability of stop orders filling if the market rests on its price.
    prob_slippage : float, default 0.0
        The probability of order fill prices slipping by one tick.
    random_seed : int, optional
        The random seed (if None then no random seed).

    """

    prob_fill_on_limit: float = 1.0
    prob_fill_on_stop: float = 1.0
    prob_slippage: float = 0.0
    random_seed: int | None = None


class ImportableFillModelConfig(NautilusConfig, frozen=True):
    """
    Configuration for a fill model instance.

    Parameters
    ----------
    fill_model_path : str
        The fully qualified name of the fill model class.
    config_path : str
        The fully qualified name of the config class.
    config : dict[str, Any]
        The fill model configuration.

    """

    fill_model_path: str
    config_path: str
    config: dict[str, Any]


class FillModelFactory:
    """
    Provides fill model creation from importable configurations.
    """

    @staticmethod
    def create(config: ImportableFillModelConfig):
        """
        Create a fill model from the given configuration.

        Parameters
        ----------
        config : ImportableFillModelConfig
            The configuration for the building step.

        Returns
        -------
        FillModel

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableFillModelConfig`.

        """
        PyCondition.type(config, ImportableFillModelConfig, "config")
        fill_model_cls = resolve_path(config.fill_model_path)
        config_cls = resolve_config_path(config.config_path)
        json = msgspec.json.encode(config.config, enc_hook=msgspec_encoding_hook)
        config_obj = config_cls.parse(json)
        return fill_model_cls(config=config_obj)


class LatencyModelConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``LatencyModel`` instances.

    Parameters
    ----------
    base_latency_nanos : int, default 1_000_000_000
        The base latency (nanoseconds) for the model.
    insert_latency_nanos : int, default 0
        The order insert latency (nanoseconds) for the model.
    update_latency_nanos : int, default 0
        The order update latency (nanoseconds) for the model.
    cancel_latency_nanos : int, default 0
        The order cancel latency (nanoseconds) for the model.

    """

    base_latency_nanos: NonNegativeInt = 1_000_000_000  # 1 millisecond in nanoseconds
    insert_latency_nanos: NonNegativeInt = 0
    update_latency_nanos: NonNegativeInt = 0
    cancel_latency_nanos: NonNegativeInt = 0


class ImportableLatencyModelConfig(NautilusConfig, frozen=True):
    """
    Configuration for a latency model instance.

    Parameters
    ----------
    latency_model_path : str
        The fully qualified name of the latency model class.
    config_path : str
        The fully qualified name of the config class.
    config : dict[str, Any]
        The latency model configuration.

    """

    latency_model_path: str
    config_path: str
    config: dict[str, Any]


class LatencyModelFactory:
    """
    Provides latency model creation from importable configurations.
    """

    @staticmethod
    def create(config: ImportableLatencyModelConfig):
        """
        Create a latency model from the given configuration.

        Parameters
        ----------
        config : ImportableLatencyModelConfig
            The configuration for the building step.

        Returns
        -------
        LatencyModel

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableLatencyModelConfig`.

        """
        PyCondition.type(config, ImportableLatencyModelConfig, "config")
        latency_model_cls = resolve_path(config.latency_model_path)
        config_cls = resolve_config_path(config.config_path)
        json = msgspec.json.encode(config.config, enc_hook=msgspec_encoding_hook)
        config_obj = config_cls.parse(json)
        return latency_model_cls(config=config_obj)


class FeeModelConfig(NautilusConfig, frozen=True):
    """
    Base configuration for ``FeeModel`` instances.
    """


class MakerTakerFeeModelConfig(FeeModelConfig, frozen=True):
    """
    Configuration for ``MakerTakerFeeModel`` instances.

    This fee model uses the maker/taker fees defined on the instrument.

    """


class FixedFeeModelConfig(FeeModelConfig, frozen=True):
    """
    Configuration for ``FixedFeeModel`` instances.

    Parameters
    ----------
    commission : Money | str
        The fixed commission amount for trades.
    charge_commission_once : bool, default True
        Whether to charge the commission once per order or per fill.

    """

    commission: str
    charge_commission_once: bool = True


class PerContractFeeModelConfig(FeeModelConfig, frozen=True):
    """
    Configuration for ``PerContractFeeModel`` instances.

    Parameters
    ----------
    commission : Money | str
        The commission amount per contract.

    """

    commission: str


class ImportableFeeModelConfig(NautilusConfig, frozen=True):
    """
    Configuration for a fee model instance.

    Parameters
    ----------
    fee_model_path : str
        The fully qualified name of the fee model class.
    config_path : str
        The fully qualified name of the config class.
    config : dict[str, Any]
        The fee model configuration.

    """

    fee_model_path: str
    config_path: str
    config: dict[str, Any]


class FeeModelFactory:
    """
    Provides fee model creation from importable configurations.
    """

    @staticmethod
    def create(config: ImportableFeeModelConfig):
        """
        Create a fee model from the given configuration.

        Parameters
        ----------
        config : ImportableFeeModelConfig
            The configuration for the building step.

        Returns
        -------
        FeeModel

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableFeeModelConfig`.

        """
        PyCondition.type(config, ImportableFeeModelConfig, "config")
        fee_model_cls = resolve_path(config.fee_model_path)
        config_cls = resolve_config_path(config.config_path)
        json = msgspec.json.encode(config.config, enc_hook=msgspec_encoding_hook)
        config_obj = config_cls.parse(json)
        return fee_model_cls(config=config_obj)


class FXRolloverInterestConfig(SimulationModuleConfig, frozen=True):
    """
    Provides an FX rollover interest simulation module.

    Parameters
    ----------
    rate_data : pd.DataFrame
        The interest rate data for the internal rollover interest calculator.

    """

    rate_data: pd.DataFrame  # TODO: This could probably just become JSON data


class MarginModelConfig(NautilusConfig, frozen=True):
    """
    Configuration for margin calculation models.

    Parameters
    ----------
    model_type : str, default 'leveraged'
        The type of margin model to use. Options:
        - "standard": Fixed percentages without leverage division (traditional brokers)
        - "leveraged": Margin requirements reduced by leverage (current Nautilus behavior)
        - Custom class path for custom models
    config : dict, optional
        Additional configuration parameters for custom models.

    """

    model_type: str = "leveraged"
    config: dict = {}


class MarginModelFactory:
    """
    Provides margin model creation from configurations.
    """

    @staticmethod
    def create(config: MarginModelConfig):
        """
        Create a margin model from the given configuration.

        Parameters
        ----------
        config : MarginModelConfig
            The configuration for the margin model.

        Returns
        -------
        MarginModel
            The created margin model instance.

        Raises
        ------
        ValueError
            If the model type is unknown or invalid.

        """
        from nautilus_trader.backtest.models import LeveragedMarginModel
        from nautilus_trader.backtest.models import StandardMarginModel

        model_type = config.model_type.lower()

        if model_type == "standard":
            return StandardMarginModel()
        elif model_type == "leveraged":
            return LeveragedMarginModel()
        else:
            # Try to import custom model
            try:
                from nautilus_trader.common.config import resolve_path

                model_cls = resolve_path(config.model_type)
                return model_cls(config)
            except Exception as e:
                raise ValueError(
                    f"Unknown `MarginModel` type '{config.model_type}'. "
                    f"Supported types: 'standard', 'leveraged', "
                    f"or a fully qualified class path. Error: {e}",
                ) from e
