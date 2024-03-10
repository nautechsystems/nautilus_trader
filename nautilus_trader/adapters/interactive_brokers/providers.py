# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import copy

import pandas as pd
from ibapi.contract import ContractDetails

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import instrument_id_to_ib_contract
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import resolve_path
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument


# fmt: on


class InteractiveBrokersInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects through Interactive Brokers.
    """

    def __init__(
        self,
        client: InteractiveBrokersClient,
        config: InteractiveBrokersInstrumentProviderConfig,
    ) -> None:
        """
        Initialize a new instance of the ``InteractiveBrokersInstrumentProvider`` class.

        Parameters
        ----------
        client : InteractiveBrokersClient
            The Interactive Brokers client.
        config : InteractiveBrokersInstrumentProviderConfig
            The instrument provider config

        """
        super().__init__(config=config)

        # Settings
        self._load_contracts_on_start = (
            set(config.load_contracts) if config.load_contracts is not None else None
        )
        self._min_expiry_days = config.min_expiry_days
        self._max_expiry_days = config.max_expiry_days
        self._build_options_chain = config.build_options_chain
        self._build_futures_chain = config.build_futures_chain
        self._cache_validity_days = config.cache_validity_days
        # TODO: If cache_validity_days > 0 and Catalog is provided

        self._client = client
        self.config = config
        self.contract_details: dict[str, IBContractDetails] = {}
        self.contract_id_to_instrument_id: dict[int, InstrumentId] = {}

    async def load_all_async(self, filters: dict | None = None) -> None:
        await self.load_ids_async([])

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        # Parse and load InstrumentIds
        if self._load_ids_on_start:
            for instrument_id in [
                (InstrumentId.from_str(i) if isinstance(i, str) else i)
                for i in self._load_ids_on_start
            ]:
                await self.load_async(instrument_id)
        # Load IBContracts
        if self._load_contracts_on_start:
            for contract in [
                (IBContract(**c) if isinstance(c, dict) else c)
                for c in self._load_contracts_on_start
            ]:
                await self.load_async(contract)

    async def get_contract_details(
        self,
        contract: IBContract,
    ) -> list[ContractDetails]:
        try:
            details = await self._client.get_contract_details(contract=contract)
            if not details:
                self._log.error(f"No contract details returned for {contract}.")
                return []
            [qualified] = details
            self._log.info(
                f"Contract qualified for {qualified.contract.localSymbol}."
                f"{qualified.contract.primaryExchange or qualified.contract.exchange} "
                f"with ConId={qualified.contract.conId}",
            )
            self._log.debug(f"Got {details=}")
        except ValueError as e:
            self._log.error(f"No contract details found for the given kwargs {contract}, {e}")
            return []
        min_expiry = pd.Timestamp.now() + pd.Timedelta(
            days=(contract.min_expiry_days or self._min_expiry_days or 0),
        )
        max_expiry = pd.Timestamp.now() + pd.Timedelta(
            days=(contract.max_expiry_days or self._max_expiry_days or 90),
        )

        if (
            contract.secType in ["FUT", "CONTFUT"]
            and contract.build_futures_chain
            or self._build_futures_chain
        ):
            # Return Underlying contract details with Future Chains
            details = await self.get_future_chain_details(
                underlying=qualified.contract,
                min_expiry=min_expiry,
                max_expiry=max_expiry,
            )
        elif contract.secType == "CONTFUT":
            # Get Active Month's Future
            details = await self._client.get_contract_details(
                IBContract(
                    secType="FUT",
                    localSymbol=qualified.contract.localSymbol,
                    exchange=qualified.contract.exchange,
                    tradingClass=qualified.contract.tradingClass,
                ),
            )
            self._log.debug(f"Got {details=}")

        if (
            contract.secType in ["STK", "FUT"]
            and contract.build_options_chain
            or self._build_options_chain
        ):
            # Return Underlying contract details with Option Chains, including for the Future Chains if apply
            for detail in set(details):
                details.extend(
                    await self.get_option_chain_details(
                        underlying=detail.contract,
                        min_expiry=min_expiry,
                        max_expiry=max_expiry,
                        last_trading_date=contract.lastTradeDateOrContractMonth,
                    ),
                )
        return details

    async def get_future_chain_details(
        self,
        underlying: IBContract,
        min_expiry: pd.Timestamp,
        max_expiry: pd.Timestamp,
    ) -> list[ContractDetails]:
        self._log.info(f"Building futures chain for {underlying.symbol}.{underlying.exchange}")
        details = await self._client.get_contract_details(
            IBContract(
                secType="FUT",
                symbol=underlying.symbol,
                exchange=underlying.exchange,
                tradingClass=underlying.tradingClass,
                includeExpired=True,
            ),
        )
        self._log.debug(f"Got {details=}")
        return details

    async def get_option_chain_details(
        self,
        underlying: IBContract,
        min_expiry: pd.Timestamp,
        max_expiry: pd.Timestamp,
        last_trading_date: str,
        exchange: str | None = None,
    ) -> list[ContractDetails]:
        if last_trading_date:
            expirations = [last_trading_date]
        else:
            try:
                chains = await self._client.get_option_chains(underlying)
                [chain] = [chain for chain in chains if chain[0] == (exchange or "SMART")]
            except ValueError as e:
                self._log.error(
                    f"No chain details loaded for the given underlying {underlying}, {e}",
                )
                return []

            expirations = sorted(
                exp for exp in chain[1] if (min_expiry <= pd.Timestamp(exp) <= max_expiry)
            )

        details = []
        for expiration in expirations:
            [option_details] = (
                await self._client.get_contract_details(
                    IBContract(
                        secType="OPT",
                        symbol=underlying.symbol,
                        lastTradeDateOrContractMonth=expiration,
                        exchange=exchange or "SMART",
                    ),
                ),
            )
            option_details = [d for d in option_details if d.underConId == underlying.conId]
            self._log.info(
                f"Received {len(option_details)} Option Contracts for "
                f"{underlying.symbol}.{underlying.primaryExchange or underlying.exchange} expiring on {expiration}",
            )
            self._log.debug(f"Got {option_details=}")
            details.extend(option_details)
        # Finally we need to match the conId with underlying because results may include other securities
        return details

    async def load_async(
        self,
        instrument_id: InstrumentId | IBContract,
        filters: dict | None = None,
    ) -> None:
        """
        Search and load the instrument for the given IBContract. It is important that
        the Contract shall have enough parameters so only one match is returned.

        Parameters
        ----------
        instrument_id : IBContract
            InteractiveBroker's Contract.
        filters : dict, optional
            Not applicable in this case.

        """
        if isinstance(instrument_id, InstrumentId):
            try:
                contract = instrument_id_to_ib_contract(
                    instrument_id=instrument_id,
                    strict_symbology=self.config.strict_symbology,
                )
            except ValueError as e:
                self._log.error(
                    f"{self.config.strict_symbology=} failed to parse {instrument_id=}, {e}",
                )
                return
        elif isinstance(instrument_id, IBContract):
            contract = instrument_id
        else:
            self._log.error(f"Expected InstrumentId or IBContract, received {instrument_id}")
            return

        self._log.debug(f"Attempting to find instrument for {contract=}")
        contract_details: list[ContractDetails]
        if not (contract_details := await self.get_contract_details(contract)):
            return
        for details in copy.deepcopy(contract_details):
            details.contract = IBContract(**details.contract.__dict__)
            details = IBContractDetails(**details.__dict__)
            self._log.debug(f"Attempting to create instrument from {details}")
            try:
                instrument: Instrument = parse_instrument(
                    contract_details=details,
                    strict_symbology=self.config.strict_symbology,
                )
            except ValueError as e:
                self._log.error(f"{self.config.strict_symbology=} failed to parse {details=}, {e}")
                continue
            if self.config.filter_callable is not None:
                filter_callable = resolve_path(self.config.filter_callable)
                if not filter_callable(instrument):
                    continue
            self._log.info(f"Adding {instrument=} from InteractiveBrokersInstrumentProvider")
            self.add(instrument)
            self._client._cache.add_instrument(instrument)
            self.contract_details[instrument.id.value] = details
            self.contract_id_to_instrument_id[details.contract.conId] = instrument.id

    async def find_with_contract_id(self, contract_id: int) -> Instrument:
        instrument_id = self.contract_id_to_instrument_id.get(contract_id)
        if not instrument_id:
            await self.load_async(IBContract(conId=contract_id))
            instrument_id = self.contract_id_to_instrument_id.get(contract_id)
        instrument = self.find(instrument_id)
        return instrument
