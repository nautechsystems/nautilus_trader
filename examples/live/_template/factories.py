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
Construct the adapter clients and instrument provider from node configuration.
"""

from nautilus_trader.live.clients import DataClientFactory
from nautilus_trader.live.clients import ExecutionClientFactory
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountType
from nautilus_trader.model import OmsType

from .constants import VENUE
from .data import TemplateDataClient
from .execution import TemplateExecutionClient
from .providers import TemplateInstrumentProvider


class TemplateDataClientFactory(DataClientFactory):
    """
    Construct clients for the deterministic live scenario.
    """

    @staticmethod
    def create(*, name, config, cache, clock) -> TemplateDataClient:
        """
        Construct a fresh client for the supplied node context.
        """
        provider = TemplateInstrumentProvider(config.instrument_provider)
        return TemplateDataClient(
            name=name,
            config=config,
            cache=cache,
            clock=clock,
            venue=VENUE,
            instrument_provider=provider,
        )


class TemplateExecutionClientFactory(ExecutionClientFactory):
    """
    Construct clients for the deterministic live scenario.
    """

    @staticmethod
    def create(*, name, config, cache, clock, trader_id) -> TemplateExecutionClient:
        """
        Construct a fresh client for the supplied node context.
        """
        return TemplateExecutionClient(
            name=name,
            config=config,
            cache=cache,
            clock=clock,
            trader_id=trader_id,
            venue=VENUE,
            account_id=AccountId("TEMPLATE-001"),
            account_type=AccountType.CASH,
            oms_type=OmsType.NETTING,
        )
