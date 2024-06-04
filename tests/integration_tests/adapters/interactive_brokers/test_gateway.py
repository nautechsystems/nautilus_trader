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

import pytest
from docker.models.containers import ContainerCollection

from nautilus_trader.adapters.interactive_brokers.gateway import DockerizedIBGateway


pytestmark = pytest.mark.skip(reason="Skip due currently flaky mocks")


def test_gateway_start_no_container(mocker):
    # Arrange
    mock_docker = mocker.patch.object(ContainerCollection, "run")
    gateway = DockerizedIBGateway(username="test", password="test")

    # Act
    gateway.start(wait=None)

    # Assert
    expected = {
        "image": "ghcr.io/unusualalpha/ib-gateway",
        "name": "nautilus-ib-gateway",
        "detach": True,
        "ports": {"4001": "4001", "4002": "4002", "5900": "5900"},
        "platform": "amd64",
        "environment": {
            "TWS_USERID": "test",
            "TWS_PASSWORD": "test",
            "TRADING_MODE": "paper",
            "READ_ONLY_API": "yes",
        },
    }
    result = mock_docker.call_args.kwargs
    assert result == expected
