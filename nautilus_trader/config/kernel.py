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

from typing import List, Optional, Union

import pydantic
from pydantic import Field

from nautilus_trader.config.components import CacheConfig
from nautilus_trader.config.components import CacheDatabaseConfig
from nautilus_trader.config.components import ImportableActorConfig
from nautilus_trader.config.components import ImportableStrategyConfig
from nautilus_trader.config.engines import DataEngineConfig
from nautilus_trader.config.engines import ExecEngineConfig
from nautilus_trader.config.engines import RiskEngineConfig
from nautilus_trader.config.live import LiveDataEngineConfig
from nautilus_trader.config.live import LiveExecEngineConfig
from nautilus_trader.config.live import LiveRiskEngineConfig
from nautilus_trader.config.persistence import PersistenceConfig
from nautilus_trader.system.kernel import Environment


class NautilusKernelConfig(pydantic.BaseModel):
    """
    Configuration for core system ``NautilusKernel`` instances.

    Parameters
    ----------
    environment : str
        The kernel environment context.
    trader_id : str
        The trader ID for the node (must be a name and ID tag separated by a hyphen).
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
    persistence : PersistenceConfig, optional
        The configuration for enabling persistence via feather files.
    data_clients : dict[str, LiveDataClientConfig], optional
        The data client configurations.
    exec_clients : dict[str, LiveExecClientConfig], optional
        The execution client configurations.
    actors : List[ImportableActorConfig]
        The actor configurations for the kernel.
    strategies : List[ImportableStrategyConfig]
        The strategy configurations for the kernel.
    load_state : bool, default True
        If trading strategy state should be loaded from the database on start.
    save_state : bool, default True
        If trading strategy state should be saved to the database on stop.
    loop_debug : bool, default False
        If the asyncio event loop should be in debug mode.
    log_level : str, default "INFO"
        The stdout log level for the node.
    bypass_logging : bool, default False
        If logging to stdout should be bypassed.
    """

    environment: Environment
    trader_id: str
    cache: Optional[CacheConfig] = None
    cache_database: Optional[CacheDatabaseConfig] = None
    data_engine: Union[DataEngineConfig, LiveDataEngineConfig]
    risk_engine: Union[RiskEngineConfig, LiveRiskEngineConfig]
    exec_engine: Union[ExecEngineConfig, LiveExecEngineConfig]
    persistence: Optional[PersistenceConfig] = None
    actors: List[ImportableActorConfig] = Field(default_factory=list)
    strategies: List[ImportableStrategyConfig] = Field(default_factory=list)
    load_state: bool = False
    save_state: bool = False
    loop_debug: bool = False
    log_level: str = "INFO"
    bypass_logging: bool = False
