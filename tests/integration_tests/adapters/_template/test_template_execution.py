# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters._template.execution import TemplateLiveExecutionClient
from nautilus_trader.live.execution_client import LiveExecutionClient


pytestmark = pytest.mark.skip(reason="template")


@pytest.fixture()
def execution_client() -> LiveExecutionClient:
    return TemplateLiveExecutionClient()  # type: ignore


def test_connect(execution_client: LiveExecutionClient):
    execution_client.connect()
    assert execution_client.is_connected


def test_disconnect(execution_client: LiveExecutionClient):
    execution_client.connect()
    execution_client.disconnect()
    assert not execution_client.is_connected


def test_submit_order(execution_client: LiveExecutionClient):
    pass


def test_submit_bracket_order(execution_client: LiveExecutionClient):
    pass


def test_modify_order(execution_client: LiveExecutionClient):
    pass


def test_cancel_order(execution_client: LiveExecutionClient):
    pass


def test_generate_order_status_report(execution_client: LiveExecutionClient):
    pass
