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

from nautilus_trader.adapters.interactive_brokers.gateway import DockerizedIBGateway


class _FakeContainer:
    def __init__(self, logs: str) -> None:
        self._logs = logs.encode()

    def logs(self) -> bytes:
        return self._logs


def test_gateway_is_logged_in_requires_completed_login_markers() -> None:
    logs = """
    2026-03-30 19:49:40:767 IBC: Found Gateway main window
    2026-03-30 19:49:40:770 IBC: Invoking config dialog menu
    """

    assert DockerizedIBGateway.is_logged_in(_FakeContainer(logs)) is False


def test_gateway_is_logged_in_when_ibc_reports_login_completed() -> None:
    logs = """
    2026-03-30 19:49:46:408 IBC: Login has completed
    2026-03-30 19:49:46:742 IBC: Configuration tasks completed
    """

    assert DockerizedIBGateway.is_logged_in(_FakeContainer(logs)) is True


def test_gateway_is_logged_in_preserves_legacy_success_markers() -> None:
    logs = "2026-03-30 19:49:46:408 IBC: Login successful"

    assert DockerizedIBGateway.is_logged_in(_FakeContainer(logs)) is True
