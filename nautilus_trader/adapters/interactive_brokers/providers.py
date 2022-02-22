# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Dict, List, Optional

import ib_insync
from ib_insync import ContractDetails

from nautilus_trader.adapters.betfair.util import one
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.live.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument


class InteractiveBrokersInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects through Interactive Brokers.
    """

    def __init__(
        self,
        client: ib_insync.IB,
        config: InstrumentProviderConfig,
        logger: Logger,
        host: str = "127.0.0.1",
        port: int = 7497,
        client_id: int = 1,
    ):
        """
        Initialize a new instance of the ``InteractiveBrokersInstrumentProvider`` class.

        Parameters
        ----------
        client : ib_insync.IB
            The Interactive Brokers client.
        config : InstrumentProviderConfig
            The instrument provider config
        logger : Logger
            The logger for the instrument provider.
        host : str
            The client host name or IP address.
        port : str
            The client port number.
        client_id : int
            The unique client ID number for the connection.

        """
        super().__init__(
            venue=IB_VENUE,
            logger=logger,
            load_all_on_start=config.load_all,
            load_ids_on_start=set(config.load_ids) if config.load_ids else None,
            filters=set(config.filters) if config.filters else None,
        )

        self._client = client
        self._host = host
        self._port = port
        self._client_id = client_id
        self.contract_details: Dict[InstrumentId, ContractDetails] = {}
        self.contract_id_to_instrument_id: Dict[int, InstrumentId] = {}

    async def load_all_async(self, filters: Optional[Dict] = None) -> None:
        raise NotImplementedError(f"load_all not implemented to {self.__class__.__name__}")

    @staticmethod
    def _one_not_both(a, b):
        return a or b and not (a and b)

    async def load_ids_async(
        self,
        instrument_ids: List[InstrumentId],
        filters: Optional[Dict] = None,
    ) -> None:
        assert self._one_not_both(instrument_ids, filters)
        await self.load(**dict(filters))

    async def load_async(self, instrument_id: InstrumentId, filters: Optional[Dict] = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def load(self, **kwargs):
        """
        Search and load the instrument for the given symbol, exchange and (optional) kwargs

        Parameters
        ----------
        symbol : str
            The symbol to search for
        exchange : str
            The exchange that the symbol trades on
        kwargs: **kwargs
            Optional extra kwargs to search for, examples:
                secType, conId, symbol, lastTradeDateOrContractMonth, strike, right, multiplier, exchange,
                primaryExchange, currency, localSymbol, tradingClass, includeExpired, secIdType, secId,
                comboLegsDescrip, comboLegs,  deltaNeutralContract
        """
        contract = ib_insync.contract.Forex(**kwargs)
        qualified = await self._client.qualifyContractsAsync(contract)
        qualified = one(qualified)
        contract_details: List[ContractDetails] = await self._client.reqContractDetailsAsync(
            contract=qualified
        )
        if not contract_details:
            raise ValueError(f"No contract details found for the given kwargs ({kwargs})")

        for details in contract_details:
            instrument: Instrument = parse_instrument(
                instrument_id=InstrumentId(Symbol(qualified.pair()), Venue(qualified.exchange)),
                contract_details=details,
            )
            self.add(instrument)
            self.contract_details[instrument.id] = details
            self.contract_id_to_instrument_id[details.contract.conId] = instrument.id
