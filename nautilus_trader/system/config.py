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

from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.common import Environment
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.common.config import LoggingConfig
from nautilus_trader.common.config import MessageBusConfig
from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import OrderEmulatorConfig
from nautilus_trader.common.config import PositiveFloat
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.execution.config import ImportableExecAlgorithmConfig
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.persistence.config import DataCatalogConfig
from nautilus_trader.persistence.config import StreamingConfig
from nautilus_trader.risk.config import RiskEngineConfig
from nautilus_trader.trading.config import ImportableControllerConfig
from nautilus_trader.trading.strategy import ImportableStrategyConfig


class NautilusKernelConfig(NautilusConfig, frozen=True):
    """
    Configuration for a ``NautilusKernel`` core system instance.

    Parameters
    ----------
    environment : Environment { ``BACKTEST``, ``SANDBOX``, ``LIVE`` }
        The kernel environment context.
    trader_id : TraderId
        The trader ID for the kernel (must be a name and ID tag separated by a hyphen).
    cache : CacheConfig, optional
        The cache configuration.
    message_bus : MessageBusConfig, optional
        The message bus configuration.
    data_engine : DataEngineConfig, optional
        The live data engine configuration.
    risk_engine : RiskEngineConfig, optional
        The live risk engine configuration.
    exec_engine : ExecEngineConfig, optional
        The live execution engine configuration.
    emulator : OrderEmulatorConfig, optional
        The order emulator configuration.
    streaming : StreamingConfig, optional
        The configuration for streaming to feather files.
    catalog : DataCatalogConfig, optional
        The data catalog config.
    actors : list[ImportableActorConfig]
        The actor configurations for the kernel.
    strategies : list[ImportableStrategyConfig]
        The strategy configurations for the kernel.
    exec_algorithms : list[ImportableExecAlgorithmConfig]
        The execution algorithm configurations for the kernel.
    controller : ImportableControllerConfig, optional
        The trader controller for the kernel.
    load_state : bool, default True
        If trading strategy state should be loaded from the database on start.
    save_state : bool, default True
        If trading strategy state should be saved to the database on stop.
    loop_debug : bool, default False
        If the asyncio event loop should be in debug mode.
    logging : LoggingConfig, optional
        The logging config for the kernel.
    snapshot_orders : bool, default False
        If order state snapshot lists should be persisted.
        Snapshots will be taken at every order state update (when events are applied).
    snapshot_positions : bool, default False
        If position state snapshot lists should be persisted.
        Snapshots will be taken at position opened, changed and closed (when events are applied).
        To include the unrealized PnL in the snapshot then quotes for the positions instrument must
        be available in the cache.
    snapshot_positions_interval : PositiveFloat, optional
        The interval (seconds) at which additional position state snapshots are persisted.
        If ``None`` then no additional snapshots will be taken.
        To include the unrealized PnL in the snapshot then quotes for the positions instrument must
        be available in the cache.
    timeout_connection : PositiveFloat (seconds)
        The timeout for all clients to connect and initialize.
    timeout_reconciliation : PositiveFloat (seconds)
        The timeout for execution state to reconcile.
    timeout_portfolio : PositiveFloat (seconds)
        The timeout for portfolio to initialize margins and unrealized PnLs.
    timeout_disconnection : PositiveFloat (seconds)
        The timeout for all engine clients to disconnect.
    timeout_post_stop : PositiveFloat (seconds)
        The timeout after stopping the node to await residual events before final shutdown.

    """

    environment: Environment
    trader_id: TraderId
    instance_id: UUID4 | None = None
    cache: CacheConfig | None = None
    message_bus: MessageBusConfig | None = None
    data_engine: DataEngineConfig | None = None
    risk_engine: RiskEngineConfig | None = None
    exec_engine: ExecEngineConfig | None = None
    emulator: OrderEmulatorConfig | None = None
    streaming: StreamingConfig | None = None
    catalog: DataCatalogConfig | None = None
    actors: list[ImportableActorConfig] = []
    strategies: list[ImportableStrategyConfig] = []
    exec_algorithms: list[ImportableExecAlgorithmConfig] = []
    controller: ImportableControllerConfig | None = None
    load_state: bool = False
    save_state: bool = False
    loop_debug: bool = False
    logging: LoggingConfig | None = None
    snapshot_orders: bool = False
    snapshot_positions: bool = False
    snapshot_positions_interval: PositiveFloat | None = None
    timeout_connection: PositiveFloat = 10.0
    timeout_reconciliation: PositiveFloat = 10.0
    timeout_portfolio: PositiveFloat = 10.0
    timeout_disconnection: PositiveFloat = 10.0
    timeout_post_stop: PositiveFloat = 10.0
