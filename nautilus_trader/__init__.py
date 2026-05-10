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
NautilusTrader (https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, with Python serving as the control plane for strategy logic,
configuration, and orchestration.
"""

from pathlib import Path
from typing import Final

from nautilus_trader.core import nautilus_pyo3


__version__ = nautilus_pyo3.NAUTILUS_VERSION

PACKAGE_ROOT: Final[Path] = Path(__file__).resolve().parent.parent
TEST_DATA_DIR: Final[Path] = PACKAGE_ROOT / "tests" / "test_data"

NAUTILUS_USER_AGENT: Final[str] = nautilus_pyo3.NAUTILUS_USER_AGENT
