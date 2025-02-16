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

from __future__ import annotations

from nautilus_trader.common.config import NautilusConfig


class PortfolioConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``Portfolio`` instances.

    Parameters
    ----------
    bar_updates : bool, default True
        If external bars should be considered for updating unrealized pnls.
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    bar_updates: bool = True
    debug: bool = False
