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

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.trading.strategy import ImportableStrategyConfig
from nautilus_trader.trading.strategy import TradingStrategy


class StrategyBuilder:
    """
    Provides strategy importing and configurable building.
    """

    @staticmethod
    def create(config: ImportableStrategyConfig) -> TradingStrategy:
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

        # TODO(cs): Implement importing in various ways
        spec: ModuleSpec = importlib.util.spec_from_file_location(config.module_name, config.path)
        module: ModuleType = importlib.util.module_from_spec(spec)
        sys.modules[config.module_name] = module
        # spec.loader.exec_module(module)
        pass
