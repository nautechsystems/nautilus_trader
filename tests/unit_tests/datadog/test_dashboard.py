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

from nautilus_trader.datadog.dashboard import _dashboard_api_url
from nautilus_trader.datadog.dashboard import load_dashboard


class TestDatadogDashboard:
    def test_load_dashboard_returns_builtin_dashboard(self):
        # Arrange, Act
        dashboard = load_dashboard()

        # Assert
        assert dashboard["title"] == "Nautilus Trading Ops"
        assert dashboard["layout_type"] == "ordered"
        assert len(dashboard["widgets"]) > 0

    def test_load_dashboard_returns_dev_dashboard(self):
        # Arrange, Act
        dashboard = load_dashboard(name="dev")

        # Assert
        assert dashboard["title"] == "Nautilus Trading Ops - Dev"
        assert dashboard["template_variables"][0]["default"] == "dev"

    def test_dashboard_api_url_supports_update_endpoint(self):
        # Arrange, Act
        url = _dashboard_api_url("us5.datadoghq.com", "abc-def-ghi")

        # Assert
        assert url == "https://api.us5.datadoghq.com/api/v1/dashboard/abc-def-ghi"
