# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.trading import ExecutionAlgorithmConfig


def test_add_native_exec_algorithm_rejects_unknown_type():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    config = ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("TWAP-UNKNOWN-TYPE"))

    with pytest.raises(TypeError, match="Unsupported native exec algorithm type: VwapAlgorithm"):
        engine.add_native_exec_algorithm("VwapAlgorithm", config)

    engine.dispose()


def test_add_native_exec_algorithm_requires_exec_algorithm_id():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))

    with pytest.raises(ValueError, match="TwapAlgorithm config requires `exec_algorithm_id`"):
        engine.add_native_exec_algorithm("TwapAlgorithm", ExecutionAlgorithmConfig())

    engine.dispose()


def test_add_native_exec_algorithm_rejects_duplicate_registration():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    config = ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("TWAP-DUPLICATE"))
    engine.add_native_exec_algorithm("TwapAlgorithm", config)

    with pytest.raises(RuntimeError, match="'TWAP-DUPLICATE' is already registered"):
        engine.add_native_exec_algorithm("TwapAlgorithm", config)

    engine.dispose()
