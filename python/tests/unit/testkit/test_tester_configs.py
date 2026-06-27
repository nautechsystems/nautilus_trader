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

from nautilus_trader.testkit import ExecTesterConfig


def test_exec_tester_config_defaults_to_hyphenated_client_order_ids() -> None:
    config = ExecTesterConfig()

    # Inherited StrategyConfig default keeps hyphens (the core default)
    assert "use_hyphens_in_client_order_ids: true" in repr(config)


def test_exec_tester_config_disables_hyphens_in_client_order_ids() -> None:
    config = ExecTesterConfig(use_hyphens_in_client_order_ids=False)

    # Venues such as OKX reject hyphenated clOrdId; the option must reach the base config
    assert "use_hyphens_in_client_order_ids: false" in repr(config)
