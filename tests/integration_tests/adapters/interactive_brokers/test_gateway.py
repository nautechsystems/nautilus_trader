# # -------------------------------------------------------------------------------------------------
# #  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
# #  https://nautechsystems.io
# #
# #  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
# #  You may not use this file except in compliance with the License.
# #  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# #
# #  Unless required by applicable law or agreed to in writing, software
# #  distributed under the License is distributed on an "AS IS" BASIS,
# #  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# #  See the License for the specific language governing permissions and
# #  limitations under the License.
# # -------------------------------------------------------------------------------------------------
#
# from unittest import mock
# from unittest.mock import MagicMock
# from unittest.mock import call
#
# import pytest
#
# from nautilus_trader.adapters.interactive_brokers.gateway import InteractiveBrokersGateway
# from tests import TESTS_PACKAGE_ROOT
#
#
# TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/ib/responses/"
#
#
# class TestIBGateway:
#     @pytest.mark.skip(reason="local test")
#     def test_gateway_start_no_container(self):
#         mock.patch("nautilus_trader.adapters.interactive_brokers.gateway.docker")
#         self.gateway = InteractiveBrokersGateway(username="test", password="test")  # noqa: S106
#         self.gateway._docker = MagicMock()
#
#         # Arrange, Act
#         self.gateway.start(wait=None)
#
#         # Assert
#         expected = call.containers.run(
#             image="ghcr.io/unusualalpha/ib-gateway",
#             name="nautilus-ib-gateway",
#             detach=True,
#             ports={"4001": "4001", "4002": "4002", "5900": "5900"},
#             platform="amd64",
#             environment={"TWSUSERID": "test", "TWSPASSWORD": "test", "TRADING_MODE": "paper"},
#         )
#         result = self.gateway._docker.method_calls[-1]
#         assert result == expected
