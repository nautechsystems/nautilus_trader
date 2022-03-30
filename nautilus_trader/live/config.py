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
from typing import Dict, FrozenSet, List, Optional

import pydantic
from pydantic import Field
from pydantic import PositiveFloat
from pydantic import PositiveInt
from pydantic import validator

from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.common.config import ImportableConfig
from nautilus_trader.common.config import InstrumentProviderConfig
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.infrastructure.config import CacheDatabaseConfig
from nautilus_trader.persistence.config import PersistenceConfig
from nautilus_trader.risk.config import RiskEngineConfig
from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.config import TradingStrategyConfig


class LiveDataEngineConfig(DataEngineConfig):
    """
    Configuration for ``LiveDataEngine`` instances.
    """

    qsize: PositiveInt = 10000


class LiveRiskEngineConfig(RiskEngineConfig):
    """
    Configuration for ``LiveRiskEngine`` instances.
    """

    qsize: PositiveInt = 10000


class LiveExecEngineConfig(ExecEngineConfig):
    """
    Configuration for ``LiveExecEngine`` instances.

    Parameters
    ----------
    reconciliation_auto : bool
        If reconciliation should automatically generate events to align state.
    reconciliation_lookback_mins : int, optional
        The maximum lookback minutes to reconcile state for. If None then will
        use the maximum lookback available from the venues.
    qsize : PositiveInt
        The queue size for the engines internal queue buffers.
    """

    reconciliation_auto: bool = True
    reconciliation_lookback_mins: Optional[PositiveInt] = None
    qsize: PositiveInt = 10000


class RoutingConfig(pydantic.BaseModel):
    """
    Configuration for live client message routing.

    default : bool
        If the client should be registered as the default routing client
        (when a specific venue routing cannot be found).
    venues : List[str], optional
        The venues to register for routing.
    """

    default: bool = False
    venues: Optional[FrozenSet[str]] = None

    def __hash__(self):  # make hashable BaseModel subclass
        return hash((type(self),) + tuple(self.__dict__.values()))


class LiveDataClientConfig(pydantic.BaseModel):
    """
    Configuration for ``LiveDataClient`` instances.

    Parameters
    ----------
    instrument_provider : InstrumentProviderConfig
        The clients instrument provider configuration.
    routing : RoutingConfig
        The clients message routing config.
    """

    instrument_provider: InstrumentProviderConfig = InstrumentProviderConfig()
    routing: RoutingConfig = RoutingConfig()


class LiveExecClientConfig(pydantic.BaseModel):
    """
    Configuration for ``LiveExecutionClient`` instances.

    Parameters
    ----------
    instrument_provider : InstrumentProviderConfig
        The clients instrument provider configuration.
    routing : RoutingConfig
        The clients message routing config.
    """

    instrument_provider: InstrumentProviderConfig = InstrumentProviderConfig()
    routing: RoutingConfig = RoutingConfig()


class ImportableLiveExecClientConfig(pydantic.BaseModel):
    """
    Represents an importable data client configuration.

    Parameters
    ----------
    path : str
        The fully qualified name of the module.
    config : str
        A JSON string representing the configuration options
    """

    path: str
    config: str

    # @classmethod
    # def parse_obj(cls, *args, **kwargs):
    #     """Overloaded so we can load the proper config class"""
    #     result: ImportableStrategyConfig = super().parse_obj(*args, **kwargs)
    #     mod = importlib.import_module(result.module)
    #     actual_cls = getattr(mod, result.cls)
    #     config = parse_obj_as(actual_cls, args[0]["config"])
    #     return result.copy(update={"config": config})


class ImportableLiveDataClientConfig(pydantic.BaseModel):
    """
    Represents an importable execution client configuration.

    Parameters
    ----------
    path : str
        The fully qualified name of the module.
    config : str
        A JSON string representing the configuration options
    """

    path: str
    config: str

    # @classmethod
    # def parse_obj(cls, *args, **kwargs):
    #     """Overloaded so we can load the proper config class"""
    #     result: ImportableStrategyConfig = super().parse_obj(*args, **kwargs)
    #     mod = importlib.import_module(result.module)
    #     actual_cls = getattr(mod, result.cls)
    #     config = parse_obj_as(actual_cls, args[0]["config"])
    #     return result.copy(update={"config": config})


class TradingNodeConfig(pydantic.BaseModel):
    """
    Configuration for ``TradingNode`` instances.

    Parameters
    ----------
    trader_id : str, default "TRADER-000"
        The trader ID for the node (must be a name and ID tag separated by a hyphen)
    log_level : str, default "INFO"
        The stdout log level for the node.
    cache : CacheConfig, optional
        The cache configuration.
    cache_database : CacheDatabaseConfig, optional
        The cache database configuration.
    data_engine : LiveDataEngineConfig, optional
        The live data engine configuration.
    risk_engine : LiveRiskEngineConfig, optional
        The live risk engine configuration.
    exec_engine : LiveExecEngineConfig, optional
        The live execution engine configuration.
    loop_debug : bool, default False
        If the asyncio event loop should be in debug mode.
    load_strategy_state : bool, default True
        If trading strategy state should be loaded from the database on start.
    save_strategy_state : bool, default True
        If trading strategy state should be saved to the database on stop.
    timeout_connection : PositiveFloat (seconds)
        The timeout for all clients to connect and initialize.
    timeout_reconciliation : PositiveFloat (seconds)
        The timeout for execution state to reconcile.
    timeout_portfolio : PositiveFloat (seconds)
        The timeout for portfolio to initialize margins and unrealized PnLs.
    timeout_disconnection : PositiveFloat (seconds)
        The timeout for all engine clients to disconnect.
    check_residuals_delay : PositiveFloat (seconds)
        The delay after stopping the node to check residual state before final shutdown.
    data_clients : dict[str, LiveDataClientConfig], optional
        The data client configurations.
    exec_clients : dict[str, LiveExecClientConfig], optional
        The execution client configurations.
    persistence : LivePersistenceConfig, optional
        The config for enabling persistence via feather files
    """

    trader_id: str = "TRADER-000"
    log_level: str = "INFO"
    cache: Optional[CacheConfig] = None
    cache_database: Optional[CacheDatabaseConfig] = None
    data_engine: Optional[LiveDataEngineConfig] = None
    risk_engine: Optional[LiveRiskEngineConfig] = None
    exec_engine: Optional[LiveExecEngineConfig] = None
    strategies: List[ImportableStrategyConfig] = Field(default_factory=list)
    loop_debug: bool = False
    load_strategy_state: bool = True
    save_strategy_state: bool = True
    timeout_connection: PositiveFloat = 10.0
    timeout_reconciliation: PositiveFloat = 10.0
    timeout_portfolio: PositiveFloat = 10.0
    timeout_disconnection: PositiveFloat = 10.0
    check_residuals_delay: PositiveFloat = 10.0
    data_clients: Dict[str, LiveDataClientConfig] = {}
    exec_clients: Dict[str, LiveExecClientConfig] = {}
    persistence: Optional[PersistenceConfig] = None

    @validator("strategies", pre=True)
    def validate_strategies(cls, v):
        """Resolve any TradingStrategyConfigs"""

        def resolve(config):
            if ImportableConfig.is_importable(config):
                cfg = ImportableConfig.create(config, config_type=TradingStrategyConfig)
                return ImportableStrategyConfig(path=config["factory_path"], config=cfg)
            return config

        strategies = [resolve(config) for config in v]
        return strategies

    @validator("data_clients", pre=True)
    def validate_importable_data_clients(cls, v):
        """Resolve any ImportableLiveExec/DataClientConfig into"""

        def resolve(config):
            if ImportableConfig.is_importable(config):
                return ImportableConfig.create(config, config_type=LiveDataClientConfig)
            return config

        data_clients = {name: resolve(config) for name, config in v.items()}
        return data_clients

    @validator("exec_clients", pre=True)
    def validate_importable_exec_clients(cls, v):
        """Resolve any ImportableLiveExec/DataClientConfig into"""

        def resolve(config):
            if ImportableConfig.is_importable(config):
                return ImportableConfig.create(config, config_type=LiveExecClientConfig)
            return config

        exec_clients = {name: resolve(config) for name, config in v.items()}
        return exec_clients
