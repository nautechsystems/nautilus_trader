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

from nautilus_trader.adapters.kraken.config import KrakenExecClientConfig


def test_kraken_exec_client_config_defaults_use_ws_trade_true() -> None:
    cfg = KrakenExecClientConfig()
    assert cfg.use_ws_trade is True
    assert cfg.ws_request_timeout_secs == 5


def test_kraken_exec_client_config_can_disable_ws_trade() -> None:
    cfg = KrakenExecClientConfig(use_ws_trade=False, ws_request_timeout_secs=10)
    assert cfg.use_ws_trade is False
    assert cfg.ws_request_timeout_secs == 10
