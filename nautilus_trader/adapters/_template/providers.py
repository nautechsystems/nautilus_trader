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

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue


TEMPLATE_VENUE = Venue("TEMPLATE")


class TemplateInstrumentProvider(InstrumentProvider):
    async def load_all_async(self):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def load_all(self):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def load(self, instrument_id: InstrumentId, details: dict):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")
