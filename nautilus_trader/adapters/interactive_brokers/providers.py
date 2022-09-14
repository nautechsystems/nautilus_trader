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

import asyncio
import datetime as dt
import json
from typing import Dict, List, Optional

import ib_insync
import numpy as np
import pandas as pd
from ib_insync import Contract
from ib_insync import ContractDetails
from ib_insync import Future

from nautilus_trader.adapters.betfair.util import one
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId
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
            config=config,
        )

        self._client = client
        self._host = host
        self._port = port
        self._client_id = client_id
        self.config = config
        self.contract_details: Dict[str, ContractDetails] = {}
        self.contract_id_to_instrument_id: Dict[int, InstrumentId] = {}

    async def load_all_async(self, filters: Optional[Dict] = None) -> None:
        for f in self._parse_filters(filters=filters or {}):
            filt = dict(f)
            await self.load(**filt)

    @staticmethod
    def _one_not_both(a, b):
        return a or b and not (a and b)

    @staticmethod
    def _parse_contract(**kwargs) -> Contract:
        sec_type = kwargs.pop("secType", None)
        return Contract(secType=sec_type, **kwargs)

    @staticmethod
    def _parse_filters(filters):
        if "filters" in filters:
            return filters["filters"]
        elif filters is None:
            return []
        return tuple(filters.items())

    async def load_ids_async(
        self,
        instrument_ids: List[InstrumentId],
        filters: Optional[Dict] = None,
    ) -> None:
        assert self._one_not_both(instrument_ids, filters)
        for filt in self._parse_filters(filters):
            await self.load(**dict(filt or {}))

    async def load_async(self, instrument_id: InstrumentId, filters: Optional[Dict] = None):
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def get_contract_details(
        self,
        contract: Contract,
        build_futures_chain=False,
        build_options_chain=False,
        option_kwargs: Optional[str] = None,
    ) -> List[ContractDetails]:
        if build_futures_chain:
            return []
        elif build_options_chain:
            return await self.get_option_chain_details(
                underlying=contract, **(json.loads(option_kwargs) or {})
            )
        else:
            # Regular contract
            return await self._client.reqContractDetailsAsync(contract=contract)

    async def get_future_chain_details(
        self,
        symbol: str,
        exchange: Optional[str] = None,
        currency: Optional[str] = None,
        **kwargs,
    ) -> List[ContractDetails]:
        futures = self._client.reqContractDetails(
            Future(
                symbol=symbol,
                exchange=exchange,
                currency=currency,
                **kwargs,
            )
        )
        return futures

    async def get_option_chain_details(
        self,
        underlying: Contract,
        min_expiry: Optional[dt.date] = None,
        max_expiry: Optional[dt.date] = None,
        min_strike: Optional[float] = None,
        max_strike: Optional[float] = None,
        kind: Optional[str] = None,
        exchange: Optional[str] = None,
    ) -> List[ContractDetails]:
        chains = await self._client.reqSecDefOptParamsAsync(
            underlying.symbol, "", underlying.secType, underlying.conId
        )

        chain = one(chains)

        strikes = [
            strike
            for strike in chain.strikes
            if (min_strike or -np.inf) <= strike <= (max_strike or np.inf)
        ]
        expirations = sorted(
            exp
            for exp in chain.expirations
            if (pd.Timestamp(min_expiry or pd.Timestamp.min) <= pd.Timestamp(exp))
            and (pd.Timestamp(exp) <= pd.Timestamp(max_expiry or pd.Timestamp.max))
        )
        rights = [kind] if kind is not None else ["P", "C"]

        contracts = [
            ib_insync.Option(
                underlying.symbol,
                expiration,
                strike,
                right,
                exchange or "SMART",
            )
            for right in rights
            for expiration in expirations
            for strike in strikes
        ]
        qualified = await self._client.qualifyContractsAsync(*contracts)
        details = await asyncio.gather(
            *[self._client.reqContractDetailsAsync(contract=c) for c in qualified]
        )
        return [x for d in details for x in d]

    async def load(self, build_options_chain=False, option_kwargs=None, **kwargs):
        """
        Search and load the instrument for the given symbol, exchange and (optional) kwargs.

        Parameters
        ----------
        build_options_chain: bool (default: False)
            Search for full option chain
        option_kwargs: str (default: False)
            JSON string for options filtering, available fields: min_expiry, max_expiry, min_strike, max_strike, kind
        kwargs: **kwargs
            Optional extra kwargs to search for, examples:
                secType, conId, symbol, lastTradeDateOrContractMonth, strike, right, multiplier, exchange,
                primaryExchange, currency, localSymbol, tradingClass, includeExpired, secIdType, secId,
                comboLegsDescrip, comboLegs,  deltaNeutralContract
        """
        self._log.debug(f"Attempting to find instrument for {kwargs=}")
        contract = self._parse_contract(**kwargs)
        self._log.debug(f"Parsed {contract=}")
        qualified = await self._client.qualifyContractsAsync(contract)
        qualified = one(qualified)
        self._log.debug(f"Qualified {contract=}")
        contract_details: List[ContractDetails] = await self.get_contract_details(
            qualified, build_options_chain=build_options_chain, option_kwargs=option_kwargs
        )
        if not contract_details:
            raise ValueError(f"No contract details found for the given kwargs ({kwargs})")
        self._log.debug(f"Got {contract_details=}")

        for details in contract_details:
            self._log.debug(f"Attempting to create instrument from {details}")
            instrument: Instrument = parse_instrument(
                contract_details=details,
            )
            self._log.info(f"Adding {instrument=} from IB instrument provider")
            self.add(instrument)
            self.contract_details[instrument.id.value] = details
            self.contract_id_to_instrument_id[details.contract.conId] = instrument.id
