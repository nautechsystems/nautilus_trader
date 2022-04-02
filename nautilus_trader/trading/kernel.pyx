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

import asyncio
import warnings
from asyncio import AbstractEventLoop
from typing import Optional, Union

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport nautilus_header
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport nanos_to_millis
from nautilus_trader.core.time cimport unix_timestamp_ns
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.infrastructure.cache cimport RedisCacheDatabase
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.live.risk_engine cimport LiveRiskEngine
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.portfolio cimport Portfolio
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.serialization.msgpack.serializer cimport MsgPackSerializer
from nautilus_trader.trading.trader cimport Trader

from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.infrastructure.config import CacheDatabaseConfig
from nautilus_trader.live.config import LiveDataEngineConfig
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.risk.config import RiskEngineConfig


try:
    import uvloop

    asyncio.set_event_loop_policy(uvloop.EventLoopPolicy())
    uvloop_version = uvloop.__version__
except ImportError:  # pragma: no cover
    uvloop_version = None
    warnings.warn("uvloop is not available.")


cdef class NautilusKernel:
    """
    Provides the core Nautilus system kernel

    The kernel is common between backtest and live systems.

    Parameters
    ----------
    name : str
        The name for the kernel (will append all logs).
    trader_id : TraderId
        The trader ID for the kernel (must be a name and ID tag separated by a hyphen).
    machine_id : str
        The kernels underlying machine identifier.
    instance_id : UUID4
        The kernels instance identifier.
    clock : Clock
        The clock for the kernel.
    uuid_factory : UUIDFactory
        The UUID factory for the kernel.
    logger : Logger
        The logger for the kernel.
    cache_config : CacheConfig
        The cache configuration for the kernel.
    cache_database_config : CacheDatabaseConfig
        The cache database configuration for the kernel.
    data_config : Union[DataEngineConfig, LiveDataEngineConfig]
        The live data engine configuration for the kernel.
    risk_config : Union[RiskEngineConfig, LiveRiskEngineConfig]
        The risk engine configuration for the kernel.
    exec_config : Union[ExecEngineConfig, LiveExecEngineConfig]
        The execution engine configuration for the kernel.
    loop : AbstractEventLoop, optional
        The event loop for the kernel.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    ValueError
        If `machine_id` is not a valid string.
    TypeError
        If any configuration object is not of the expected type.

    """

    def __init__(
        self,
        str name not None,
        TraderId trader_id not None,
        str machine_id not None,
        UUID4 instance_id not None,
        Clock clock not None,
        UUIDFactory uuid_factory not None,
        Logger logger not None,
        cache_config not None: CacheConfig,
        cache_database_config not None: CacheDatabaseConfig,
        data_config not None: Union[DataEngineConfig, LiveDataEngineConfig],
        risk_config not None: Union[RiskEngineConfig, LiveRiskEngineConfig],
        exec_config not None: Union[ExecEngineConfig, LiveExecEngineConfig],
        loop: Optional[AbstractEventLoop] = None,
    ):
        Condition.valid_string(name, "name")
        Condition.valid_string(machine_id, "machine_id")
        Condition.type(cache_config, CacheConfig, "cache_config")
        Condition.type(cache_database_config, CacheDatabaseConfig, "cache_database_config")
        Condition.true(isinstance(data_config, (DataEngineConfig, LiveDataEngineConfig)), "data_config was unrecognized type", ex_type=TypeError)
        Condition.true(isinstance(risk_config, (RiskEngineConfig, LiveRiskEngineConfig)), "risk_config was unrecognized type", ex_type=TypeError)
        Condition.true(isinstance(exec_config, (ExecEngineConfig, LiveExecEngineConfig)), "exec_config was unrecognized type", ex_type=TypeError)

        # Components
        self.clock = clock
        self.uuid_factory = UUIDFactory()
        self.logger = logger
        self.log = LoggerAdapter(
            component_name=name,
            logger=logger,
        )

        # Identifiers
        self.name = name
        self.trader_id = trader_id
        self.machine_id = machine_id
        self.instance_id = instance_id
        self.ts_created = unix_timestamp_ns()

        nautilus_header(self.log, uvloop_version)
        self.log.info("Building system kernel...")

        if cache_database_config is None or cache_database_config.type == "in-memory":
            cache_db = None
        elif cache_database_config.type == "redis":
            cache_db = RedisCacheDatabase(
                trader_id=self.trader_id,
                logger=self.logger,
                serializer=MsgPackSerializer(timestamps_as_str=True),
                config=cache_database_config,
            )
        else:
            raise ValueError(
                f"The `cache_db_config.type` is unrecognized. "
                f"Please use one of {{\'in-memory\', \'redis\'}}.",
            )

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = Cache(
            database=cache_db,
            logger=logger,
            config=cache_config,
        )

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        ########################################################################
        # Data Engine
        ########################################################################
        if isinstance(data_config, LiveDataEngineConfig):
            self.data_engine = LiveDataEngine(
                loop=loop,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=data_config,
            )
        elif isinstance(data_config, DataEngineConfig):
            self.data_engine = DataEngine(
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=data_config,
            )

        ########################################################################
        # Risk Engine
        ########################################################################
        if isinstance(risk_config, LiveRiskEngineConfig):
            self.risk_engine = LiveRiskEngine(
                loop=loop,
                portfolio=self.portfolio,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=risk_config,
            )
        elif isinstance(risk_config, RiskEngineConfig):
            self.risk_engine = RiskEngine(
                portfolio=self.portfolio,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=risk_config,
            )

        ########################################################################
        # Execution Engine
        ########################################################################
        if isinstance(exec_config, LiveExecEngineConfig):
            self.exec_engine = LiveExecutionEngine(
                loop=loop,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=exec_config,
            )
        elif isinstance(exec_config, ExecEngineConfig):
            self.exec_engine = ExecutionEngine(
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=exec_config,
            )

        if exec_config.load_cache:
            self.exec_engine.load_cache()

        self.trader = Trader(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            portfolio=self.portfolio,
            data_engine=self.data_engine,
            risk_engine=self.risk_engine,
            exec_engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        cdef int64_t build_time_ms = nanos_to_millis(unix_timestamp_ns() - self.ts_created)
        self.log.info(f"Initialized in {build_time_ms}ms.")
