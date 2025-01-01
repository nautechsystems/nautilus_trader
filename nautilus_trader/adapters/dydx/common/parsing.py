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
"""
Define common methods for parsing messages from dYdX.
"""

from nautilus_trader.adapters.dydx.common.enums import DYDXCandlesResolution
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.model.data import BarType


def get_interval_from_bar_type(bar_type: BarType) -> DYDXCandlesResolution:
    """
    Convert a nautilus bar type to a dYdX candles resolution enum.
    """
    enum_parser = DYDXEnumParser()
    return enum_parser.parse_dydx_kline(bar_type)
