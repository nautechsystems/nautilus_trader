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
Test configs behavior.
"""

from nautilus_trader import network


def test_network_exposes_transport_backend_config_only() -> None:
    """
    Test network exposes transport backend config only.
    """
    public_names = {name for name in vars(network) if not name.startswith("_")}

    assert public_names == {"TransportBackend"}
    assert network.TransportBackend.TUNGSTENITE != network.TransportBackend.SOCKUDO
