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

from decimal import Decimal
from typing import Any, Dict, Optional

from nautilus_trader.backtest.node import BacktestNode


try:
    import hyperopt
except ImportError:
    # hyperopt is an optional extra, which is only required when running `hyperopt_search()`.
    hyperopt = None

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.identifiers import InstrumentId


class HyperoptBacktestNode(BacktestNode):
    """
    Provides a specific node for `hyperopt` backtest runs.

    Distributed Asynchronous Hyper-parameter Optimization.

    Parameters
    ----------
    base_config : BacktestRunConfig
        The base backtest run config to build from.
    """

    def __init__(self, base_config: BacktestRunConfig):
        super().__init__(configs=[base_config])

        self.config: BacktestRunConfig = base_config

        self.strategy_path: Optional[str] = None
        self.config_path: Optional[str] = None
        self.strategy_config: Optional[StrategyConfig] = None
        self.instrument_id: Optional[InstrumentId] = None
        self.bar_type: Optional[BarType] = None
        self.trade_size: Optional[Decimal] = None

    def set_strategy_config(
        self,
        strategy_path: str,
        config_path: str,
    ) -> None:
        """
        Set strategy parameters which can be passed to the hyperopt objective.

        Parameters
        ----------
        strategy_path : str
            The path to the strategy.
        config_path : str
            The path to the strategy config.

        """
        self.strategy_path = strategy_path
        self.config_path = config_path

    def hyperopt_search(
        self,
        params: Dict[str, Any],
        minimum_positions: int = 50,
        max_evals: int = 50,
    ) -> Dict[str, Any]:
        """
        Run with hyperopt to optimize strategy parameters.

        Parameters
        ----------
        params : Dict[str, Any]
            The set of strategy parameters to optimize.
        minimum_positions: int, default 50
            The minimum number of positions to accept a gradient.
        max_evals : int, default 50
            The maximum number of evaluations for the optimization problem.

        Returns
        -------
        dict[str, Any]
            The optimized strategy parameters.

        Raises
        ------
        ImportError
            If hyperopt is not available.

        """
        if hyperopt is None:
            raise ImportError(
                "The hyperopt package is not installed. "
                "Please install via pip or poetry install -E hyperopt",
            )

        logger = Logger(clock=LiveClock(), level_stdout=LogLevel.INFO)
        logger_adapter = LoggerAdapter(component_name="HYPEROPT_LOGGER", logger=logger)

        def objective(args):
            logger_adapter.info(f"Searching with {args}")

            config = ImportableStrategyConfig(
                strategy_path=self.strategy_path,
                config_path=self.config_path,
                config=args,
            )

            local_config: BacktestRunConfig = self.config
            local_config.engine.strategies = [config]

            local_config.check()

            try:
                result = self._run(
                    run_config_id=local_config.id,
                    engine_config=local_config.engine,
                    venue_configs=local_config.venues,
                    data_configs=local_config.data,
                    batch_size_bytes=local_config.batch_size_bytes,
                )

                base_currency = self.config.venues[0].base_currency
                # logger_adapter.info(f"{result.stats_pnls[base_currency]}")
                pnl_pct = result.stats_pnls[base_currency]["PnL%"]
                profit_factor = result.stats_returns["Profit Factor"]
                logger_adapter.info(f"OBJECTIVE: {1/pnl_pct}")
                # win_rate = result.stats_pnls['USDT']['Win Rate']

                if (
                    (1 / profit_factor) == 0
                    or profit_factor <= 0
                    or result.total_positions < minimum_positions
                ):
                    ret = {"status": hyperopt.STATUS_FAIL}
                else:
                    ret = {"status": hyperopt.STATUS_OK, "loss": (1 / profit_factor)}

            except Exception as e:
                ret = {"status": hyperopt.STATUS_FAIL}
                logger_adapter.error(f"Bankruptcy : {e} ")
            return ret

        trials = hyperopt.Trials()

        return hyperopt.fmin(
            fn=objective,
            space=params,
            algo=hyperopt.tpe.suggest,
            trials=trials,
            max_evals=max_evals,
        )
