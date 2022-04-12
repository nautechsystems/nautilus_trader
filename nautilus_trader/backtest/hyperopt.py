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
from typing import Dict, Optional

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
from nautilus_trader.config.backtest import BacktestRunConfig
from nautilus_trader.config.components import ImportableStrategyConfig
from nautilus_trader.config.components import TradingStrategyConfig
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.identifiers import InstrumentId


class HyperoptBacktestNode(BacktestNode):
    """
    Provides a specific node for `hyperopt` backtest runs.

    Distributed Asynchronous Hyper-parameter Optimization.
    """

    def __init__(self):
        self.path: Optional[str] = None
        self.strategy: Optional[TradingStrategyConfig] = None
        self.instrument_id: Optional[InstrumentId] = None
        self.bar_type: Optional[BarType] = None
        self.trade_size: Optional[Decimal] = None
        self.config: Optional[BacktestRunConfig] = None

    def set_strategy_config(
        self,
        path: str,
        strategy: TradingStrategyConfig,
        instrument_id: InstrumentId,
        bar_type: BarType,
        trade_size: Decimal,
    ) -> None:
        """
        Set strategy parameters which can be passed to the hyperopt objective.

        Parameters
        ----------
        path : str
            The path to the strategy.
        strategy : TradingStrategyConfig
            The strategy config object.
        instrument_id : InstrumentId
            The instrument ID.
        bar_type : BarType
            The type of bar type used.
        trade_size : Decimal
            The trade size to be used.

        """
        self.path = path
        self.strategy = strategy
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size

    def hyperopt_search(self, config, params, minimum_positions=50, max_evals=50) -> Dict:
        """
        Run hyperopt to optimize strategy parameters.

        Parameters
        ----------
        config : BacktestRunConfig
            The configuration for the backtest test.
        params : Dict[str, Any]
            The set of strategy parameters to optimize.
        max_evals : int
            The maximum number of evaluations for the optimization problem.

        Returns
        -------
        Dict
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
        self.config = config

        def objective(args):

            logger_adapter.info(f"{args}")

            strategies = [
                ImportableStrategyConfig(
                    path=self.path,
                    config=self.strategy(
                        instrument_id=self.instrument_id,
                        bar_type=self.bar_type,
                        trade_size=self.trade_size,
                        **args,
                    ),
                ),
            ]

            local_config = self.config
            local_config = local_config.replace(strategies=strategies)

            local_config.check()

            try:
                result = self._run(
                    engine_config=local_config.engine,
                    run_config_id=local_config.id,
                    venue_configs=local_config.venues,
                    data_configs=local_config.data,
                    actor_configs=local_config.actors,
                    strategy_configs=local_config.strategies,
                    persistence=local_config.persistence,
                    batch_size_bytes=local_config.batch_size_bytes,
                    # return_engine=True
                )

                base_currency = self.config.venues[0].base_currency
                # logger_adapter.info(f"{result.stats_pnls[base_currency]}")
                pnl_pct = result.stats_pnls[base_currency]["PnL%"]
                logger_adapter.info(f"OBJECTIVE: {1/pnl_pct}")
                # win_rate = result.stats_pnls['USDT']['Win Rate']

                if (1 / pnl_pct) == 0 or pnl_pct <= 0 \
                    or result.total_positions < minimum_positions:
                    ret = {"status": hyperopt.STATUS_FAIL}
                else:
                    ret = {"status": hyperopt.STATUS_OK, "loss": (1 / pnl_pct)}

            except Exception as e:
                ret = {"status": hyperopt.STATUS_FAIL}
                logger_adapter.error(f"Bankruptcy : {e} ")
            return ret

        trials = hyperopt.Trials()

        return hyperopt.fmin(
            objective, params, algo=hyperopt.tpe.suggest, trials=trials, max_evals=max_evals
        )
