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

from nautilus_trader.execution import ExecutionEngineConfig
from nautilus_trader.execution import OrderEmulatorConfig


def test_execution_engine_config_defaults():
    config = ExecutionEngineConfig()
    assert config.load_cache is True
    assert config.manage_own_order_books is False
    assert config.snapshot_orders is False
    assert config.snapshot_positions is False
    assert config.snapshot_positions_interval_secs is None
    assert config.allow_overfills is False
    assert config.purge_from_database is False
    assert config.debug is False


def test_execution_engine_config_with_overrides():
    config = ExecutionEngineConfig(
        load_cache=False,
        manage_own_order_books=True,
        snapshot_orders=True,
        snapshot_positions=True,
        snapshot_positions_interval_secs=5.0,
        allow_overfills=True,
        purge_from_database=True,
        debug=True,
    )
    assert config.load_cache is False
    assert config.manage_own_order_books is True
    assert config.snapshot_orders is True
    assert config.snapshot_positions is True
    assert config.snapshot_positions_interval_secs == 5.0
    assert config.allow_overfills is True
    assert config.purge_from_database is True
    assert config.debug is True


def test_execution_engine_config_repr():
    config = ExecutionEngineConfig()
    assert "ExecutionEngineConfig" in repr(config)


def test_order_emulator_config_defaults():
    config = OrderEmulatorConfig()
    assert config.debug is False


def test_order_emulator_config_debug_enabled():
    config = OrderEmulatorConfig(debug=True)
    assert config.debug is True


def test_order_emulator_config_repr():
    config = OrderEmulatorConfig()
    assert "OrderEmulatorConfig" in repr(config)
