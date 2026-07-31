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
"""
Static type contract for supported Python v2 authoring workflows.
"""

from decimal import Decimal
from typing import assert_type

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.trading import ExecutionAlgorithm
from nautilus_trader.trading import ExecutionAlgorithmConfig
from nautilus_trader.trading import ImportableExecAlgorithmConfig
from nautilus_trader.trading import StrategyConfig


instrument_id = InstrumentId.from_str("ETHUSDT-PERP.BINANCE")
price = Price.from_str("123.45")
quantity = Quantity.from_str("1.000")
strategy_config = StrategyConfig(strategy_id=StrategyId("MOMENTUM-001"))
exec_config = ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("TWAP-001"))
importable = ImportableExecAlgorithmConfig(
    exec_algorithm_path="strategies.twap:TwapAlgorithm",
    config_path="strategies.twap:TwapConfig",
    config={"exec_algorithm_id": "TWAP-001"},
)
algorithm = ExecutionAlgorithm(exec_config)
engine = BacktestEngine(BacktestEngineConfig())

assert_type(instrument_id, InstrumentId)
assert_type(instrument_id.value, str)
assert_type(price.as_decimal(), Decimal)
assert_type(quantity.as_decimal(), Decimal)
assert_type(strategy_config.strategy_id, StrategyId | None)
assert_type(exec_config.exec_algorithm_id, ExecAlgorithmId | None)
assert_type(importable.exec_algorithm_path, str)
assert_type(algorithm.to_importable_config(), ImportableExecAlgorithmConfig)
assert_type(engine.iteration, int)
