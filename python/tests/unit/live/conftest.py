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
Capture adapter diagnostics independently of process-wide logger initialization.
"""

from unittest.mock import Mock

import pytest

from nautilus_trader import common
from nautilus_trader.common import Logger
from nautilus_trader.live import providers


@pytest.fixture
def native_log(monkeypatch) -> Mock:
    """
    Capture calls using the native Logger interface.
    """
    logger = Mock(spec=Logger)
    constructor = Mock(return_value=logger)
    monkeypatch.setattr(common, "Logger", constructor)
    monkeypatch.setattr(providers, "Logger", constructor)
    return logger
