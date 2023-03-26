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

from typing import Any, Optional

from nautilus_trader.config import ExecAlgorithmConfig
from nautilus_trader.config import ImportableExecAlgorithmConfig

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class ExecAlgorithm(Actor):
    """
    The base class for all execution algorithms.

    This class allows traders to implement their own customized execution algorithms.

    Parameters
    ----------
    config : ExecAlgorithmConfig, optional
        The execution algorithm configuration.

    Raises
    ------
    TypeError
        If `config` is not of type `ExecAlgorithmConfig`.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, config: Optional[ExecAlgorithmConfig] = None):
        if config is None:
            config = ExecAlgorithmConfig()
        Condition.type(config, ExecAlgorithmConfig, "config")

        super().__init__()
        # Assign Execution Algorithm ID after base class initialized
        self.id = type(self).__name__ if config.exec_algorithm_id is None else config.exec_algorithm_id

        # Configuration
        self.config = config

        # Public components
        self.clock = self._clock
        self.cache = None          # Initialized when registered
        self.portfolio = None      # Initialized when registered
        self.order_factory = None  # Initialized when registered

    def to_importable_config(self) -> ImportableExecAlgorithmConfig:
        """
        Returns an importable configuration for this execution algorithm.

        Returns
        -------
        ImportableExecAlgorithmConfig

        """
        return ImportableExecAlgorithmConfig(
            exec_algorithm_path=self.fully_qualified_name(),
            config_path=self.config.fully_qualified_name(),
            config=self.config.dict(),
        )

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register(
        self,
        TraderId trader_id,
        PortfolioFacade portfolio,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
        Logger logger,
    ):
        """
        Register the execution algorithm with a trader.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID for the execution algorithm.
        portfolio : PortfolioFacade
            The read-only portfolio for the execution algorithm.
        msgbus : MessageBus
            The message bus for the execution algorithm.
        cache : CacheFacade
            The read-only cache for the execution algorithm.
        clock : Clock
            The clock for the execution algorithm.
        logger : Logger
            The logger for the execution algorithm.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(trader_id, "trader_id")
        Condition.not_none(portfolio, "portfolio")
        Condition.not_none(msgbus, "msgbus")
        Condition.not_none(cache, "cache")
        Condition.not_none(clock, "clock")
        Condition.not_none(logger, "logger")

        self.register_base(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self.portfolio = portfolio  # Assigned as PortfolioFacade

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.id,
            clock=self.clock,
        )

        # Required subscriptions
        # self._msgbus.subscribe(topic=f"events.order.{self.id}", handler=self.handle_event)
        # self._msgbus.subscribe(topic=f"events.position.{self.id}", handler=self.handle_event)
