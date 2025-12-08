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

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig


class LighterInstrumentProvider(InstrumentProvider):
    """
    Placeholder instrument provider for Lighter perpetuals.

    The full implementation will be added in PR1 once the orderBooks REST client is available.
    """

    def __init__(
        self,
        client: object,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config or InstrumentProviderConfig())
        self._client = client

    async def load_all_async(self, filters: dict | None = None) -> None:  # pragma: no cover - stub
        raise NotImplementedError("Instrument discovery will be implemented in PR1.")

    async def load_ids_async(  # pragma: no cover - stub
        self,
        instrument_ids: list,
        filters: dict | None = None,
    ) -> None:
        raise NotImplementedError("Instrument discovery will be implemented in PR1.")

    async def load_async(self, instrument_id, filters: dict | None = None) -> None:  # pragma: no cover - stub
        raise NotImplementedError("Instrument discovery will be implemented in PR1.")
