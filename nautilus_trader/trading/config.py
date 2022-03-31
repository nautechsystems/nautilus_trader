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
from typing import Optional

from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.common.config import resolve_path
from nautilus_trader.core.correctness import PyCondition


class TradingStrategyConfig(ActorConfig):
    """
    The base model for all trading strategy configurations.

    Parameters
    ----------
    component_id : str, optional
        The unique component ID for the strategy. Will become the strategy ID if not None.
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OMSType, optional
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).

    """

    order_id_tag: str = "000"
    oms_type: Optional[str] = None


class ImportableStrategyConfig(ImportableActorConfig):
    """
    Represents a trading strategy configuration for one specific backtest run.

    Parameters
    ----------
    strategy_path : str
        The fully qualified name of the strategy class.
    config_path : str
        The fully qualified name of the config class.
    config : Dict
        The strategy configuration
    """

    strategy_path: str
    config_path: str
    config: dict


class StrategyFactory:
    """
    Provides strategy creation from importable configurations.
    """

    @staticmethod
    def create(config: ImportableStrategyConfig):
        """
        Create a trading strategy from the given configuration.

        Parameters
        ----------
        config : ImportableStrategyConfig
            The configuration for the building step.

        Returns
        -------
        TradingStrategy

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableStrategyConfig`.

        """
        PyCondition.type(config, ImportableStrategyConfig, "config")
        strategy_cls = resolve_path(config.strategy_path)
        config_cls = resolve_path(config.config_path)
        return strategy_cls(config=config_cls(**config.config))
