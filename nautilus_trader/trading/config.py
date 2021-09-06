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

import importlib.util
import sys
from importlib.machinery import ModuleSpec
from types import ModuleType
from typing import Optional

import pydantic

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.trading.strategy import TradingStrategy
from nautilus_trader.trading.strategy import TradingStrategyConfig


class StrategyFactory:
    """
    Provides strategy creation from importable configurations.
    """

    @staticmethod
    def create(config: "ImportableStrategyConfig") -> TradingStrategy:
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
            If callback is not of type `ImportableStrategyConfig`.

        """
        PyCondition.type(config, ImportableStrategyConfig, "config")
        if (config.path is None or config.path.isspace()) and (
            config.source is None or config.source.isspace()
        ):
            raise ValueError("both `source` and `path` were None")

        # TODO(cs): Implement importing in various ways
        if config.source is not None:
            spec: ModuleSpec = importlib.util.spec_from_loader(config.module, loader=None)
            module: ModuleType = importlib.util.module_from_spec(spec)

            exec(config.source, module.__dict__)  # noqa
            sys.modules[config.module] = module

        # spec.loader.exec_module(module)
        else:
            pass


class ImportableStrategyConfig(pydantic.BaseModel):
    """
    Represents the trading strategy configuration for one specific backtest run.

    name : str
        The fully-qualified name of the module.
    path : str
        The path to the source code.

    """

    module: str
    cls: str
    path: Optional[str]
    source: Optional[bytes]
    config: Optional[TradingStrategyConfig]
