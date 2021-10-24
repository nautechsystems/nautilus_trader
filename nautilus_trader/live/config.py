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

from typing import Any, Dict, Optional

import pydantic
from pydantic import PositiveFloat
from pydantic import PositiveInt

from nautilus_trader.cache.cache import CacheConfig
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.infrastructure.cache import CacheDatabaseConfig
from nautilus_trader.persistence.config import PersistenceConfig
from nautilus_trader.risk.config import RiskEngineConfig


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
    """

    qsize: PositiveInt = 10000


class TradingNodeConfig(pydantic.BaseModel):
    """
    Configuration for ``TradingNode`` instances.

    trader_id : str, default="TRADER-000"
        The trader ID for the node (must be a name and ID tag separated by a hyphen)
    log_level : str, default="INFO"
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
    loop_debug : bool, default=False
        If the asyncio event loop should be in debug mode.
    load_strategy_state : bool, default=True
        If trading strategy state should be loaded from the database on start.
    save_strategy_state : bool, default=True
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
    data_clients : dict[str, dict[str, Any]], optional
        The data client configurations.
    exec_clients : dict[str, dict[str, Any]], optional
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
    loop_debug: bool = False
    load_strategy_state: bool = True
    save_strategy_state: bool = True
    timeout_connection: PositiveFloat = 10.0
    timeout_reconciliation: PositiveFloat = 10.0
    timeout_portfolio: PositiveFloat = 10.0
    timeout_disconnection: PositiveFloat = 10.0
    check_residuals_delay: PositiveFloat = 10.0
    data_clients: Dict[str, Dict[str, Any]] = {}
    exec_clients: Dict[str, Dict[str, Any]] = {}
    persistence: Optional[PersistenceConfig] = None
