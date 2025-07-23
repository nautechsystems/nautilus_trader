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

import copy

import pandas as pd
from ibapi.contract import ContractDetails

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails
from nautilus_trader.adapters.interactive_brokers.common import dict_to_contract_details
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import VENUE_MEMBERS
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import instrument_id_to_ib_contract
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import resolve_path
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument


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

        # Configuration
        self._load_contracts_on_start = (
            set(config.load_contracts) if config.load_contracts is not None else None
        )
        self._min_expiry_days = config.min_expiry_days
        self._max_expiry_days = config.max_expiry_days
        self._build_options_chain = config.build_options_chain
        self._build_futures_chain = config.build_futures_chain
        self._cache_validity_days = config.cache_validity_days
        self._convert_exchange_to_mic_venue = config.convert_exchange_to_mic_venue
        self._symbol_to_mic_venue = config.symbol_to_mic_venue
        # TODO: If cache_validity_days > 0 and Catalog is provided

        self._client = client
        self.config = config
        self.contract_details: dict[InstrumentId, IBContractDetails] = {}
        self.contract_id_to_instrument_id: dict[int, InstrumentId] = {}
        self.contract: dict[InstrumentId, IBContract] = {}

    async def initialize(self, reload: bool = False) -> None:
        await super().initialize(reload)

        # Trigger contract loading only if `load_ids_on_start` is False and `load_contracts_on_start` is True
        if not self._load_ids_on_start and self._load_contracts_on_start:
            self._loaded = False
            self._loading = True
            await self.load_all_async()  # Load all instruments passed as config at startup
            self._loading = False
            self._loaded = True

    async def get_instrument(self, contract_id: int) -> Instrument:
        instrument_id = self.contract_id_to_instrument_id.get(contract_id)

        if not instrument_id:
            await self.load_async(IBContract(conId=contract_id))
            instrument_id = self.contract_id_to_instrument_id.get(contract_id)

        instrument = self.find(instrument_id)

        return instrument

    async def instrument_id_to_ib_contract(
        self,
        instrument_id: InstrumentId,
    ) -> IBContract | None:
        venue = instrument_id.venue.value
        possible_exchanges = VENUE_MEMBERS.get(venue, [venue])

        if len(possible_exchanges) == 1:
            return instrument_id_to_ib_contract(
                instrument_id,
                possible_exchanges[0],
                self.config.symbology_method,
            )
        elif await self.fetch_instrument_id(instrument_id):
            return self.contract[instrument_id]
        else:
            return None

    async def instrument_id_to_ib_contract_details(
        self,
        instrument_id: InstrumentId,
    ) -> IBContractDetails | None:
        if await self.fetch_instrument_id(instrument_id):
            return self.contract_details[instrument_id]

        return None

    def get_price_magnifier(self, instrument_id: InstrumentId) -> int:
        """
        Get the price magnifier for an instrument from its contract details.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier.

        Returns
        -------
        int
            The price magnifier, defaults to 1 if not found.

        """
        contract_details = self.contract_details.get(instrument_id)

        if contract_details:
            return contract_details.priceMagnifier

        return 1

    async def load_all_async(self, filters: dict | None = None) -> None:
        start_instrument_ids = [
            (InstrumentId.from_str(i) if isinstance(i, str) else i) for i in self._load_ids_on_start
        ]

        start_ib_contracts = [
            (IBContract(**c) if isinstance(c, dict) else c) for c in self._load_contracts_on_start
        ]

        await self.load_ids_with_return_async(start_instrument_ids + start_ib_contracts)

    async def load_ids_with_return_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
        force_instrument_update: bool = False,
    ) -> list[InstrumentId]:
        """
        Load instruments for the given IDs and return the instrument IDs of successfully
        loaded instruments.

        This is the original implementation.

        """
        loaded_instrument_ids = []

        for instrument_id in instrument_ids:
            loaded_ids = await self.load_with_return_async(
                instrument_id,
                filters,
                force_instrument_update=force_instrument_update,
            )

            if loaded_ids:
                loaded_instrument_ids.extend(loaded_ids)

        return loaded_instrument_ids

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load instruments for the given IDs (base class interface).
        """
        # Call the auxiliary function that maintains your working logic
        await self.load_ids_with_return_async(
            instrument_ids,
            filters,
            force_instrument_update=False,
        )

    async def load_with_return_async(
        self,
        instrument_id: InstrumentId | IBContract,
        filters: dict | None = None,
        force_instrument_update: bool = False,
    ) -> list[InstrumentId] | None:
        """
        Search and load the instrument for the given IBContract.

        This is the original implementation that returns values.

        """
        contract_details: list | None = None

        if isinstance(instrument_id, InstrumentId):
            venue = instrument_id.venue.value

            if await self.fetch_instrument_id(instrument_id, force_instrument_update):
                return [instrument_id]  # Return the instrument ID if successfully fetched
            else:
                return None
        elif isinstance(instrument_id, IBContract):
            contract = instrument_id
            contract_details = await self.get_contract_details(contract)

            if contract_details:
                full_contract = contract_details[0].contract
                venue = self.determine_venue_from_contract(full_contract)
        else:
            self._log.error(f"Expected InstrumentId or IBContract, received {instrument_id}")
            return None

        if contract_details:
            return self._process_contract_details(contract_details, venue, force_instrument_update)
        else:
            self._log.error(
                f"Unable to resolve contract details for {instrument_id!r}. "
                f"If you believe the InstrumentId or IBContract is correct, please verify its tradability "
                f"in TWS (Trader Workstation) for Interactive Brokers.",
            )
            return None

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        """
        Load the instrument for the given instrument ID (base class interface).
        """
        # Call the auxiliary function that maintains your working logic
        await self.load_with_return_async(instrument_id, filters, force_instrument_update=False)

    async def fetch_instrument_id(
        self,
        instrument_id: InstrumentId,
        force_instrument_update: bool = False,
    ) -> bool:
        if instrument_id in self.contract:
            return True

        venue = instrument_id.venue.value

        # We try to quickly build the contract details if they are already present in an instrument
        if (
            instrument := self._client._cache.instrument(instrument_id)
        ) and not force_instrument_update:
            if instrument.info and instrument.info.get("contract"):
                converted_contract_details = dict_to_contract_details(instrument.info)
                processed_ids = self._process_contract_details([converted_contract_details], venue)

                return bool(processed_ids)  # Return True if any instruments were processed

        # VENUE_MEMBERS associates a MIC venue to several possible IB exchanges
        possible_exchanges = VENUE_MEMBERS.get(venue, [venue])

        try:
            for exchange in possible_exchanges:
                contract = instrument_id_to_ib_contract(
                    instrument_id=instrument_id,
                    exchange=exchange,
                    symbology_method=self.config.symbology_method,
                )

                self._log.info(f"Attempting to find instrument for {contract=}")
                contract_details: list = await self.get_contract_details(contract)

                if contract_details:
                    processed_ids = self._process_contract_details(
                        contract_details,
                        venue,
                        force_instrument_update,
                    )

                    return bool(processed_ids)  # Return True if any instruments were processed
        except ValueError as e:
            self._log.error(str(e))

        return False

    async def get_contract_details(
        self,
        contract: IBContract,
    ) -> list[ContractDetails]:
        try:
            details = await self._client.get_contract_details(contract=contract)

            if not details:
                self._log.debug(f"No contract details returned for {contract}")
                return []

            [qualified] = details
            self._log.info(
                f"Contract qualified for {qualified.contract.localSymbol}."
                f"{qualified.contract.primaryExchange or qualified.contract.exchange} "
                f"with ConId={qualified.contract.conId}",
            )
            self._log.debug(f"Got {details=}")
        except ValueError as e:
            self._log.debug(f"No contract details found for the given kwargs {contract}, {e}")
            return []

        utc_now = pd.Timestamp.utcnow()
        min_expiry_days = contract.min_expiry_days or self._min_expiry_days or 0
        max_expiry_days = contract.max_expiry_days or self._max_expiry_days or 90

        min_expiry = utc_now + pd.Timedelta(days=min_expiry_days)
        max_expiry = utc_now + pd.Timedelta(days=max_expiry_days)

        if (
            contract.secType == "CONTFUT"
            and (contract.build_futures_chain or contract.build_options_chain)
        ) or (self._build_futures_chain or self._build_options_chain):
            # Return Underlying contract details with Future Chains
            details = await self.get_future_chain_details(qualified.contract)

        if (
            contract.secType in ["STK", "CONTFUT", "FUT", "IND"] and contract.build_options_chain
        ) or self._build_options_chain:
            # Return Underlying contract details with Option Chains, including for the Future Chains if apply
            for detail in set(details):
                if contract.lastTradeDateOrContractMonth:
                    option_contracts_detail = await self.get_option_chain_details_by_expiry(
                        underlying=detail.contract,
                        last_trading_date=contract.lastTradeDateOrContractMonth,
                        exchange=contract.options_chain_exchange or contract.exchange,
                    )
                else:
                    option_contracts_detail = await self.get_option_chain_details_by_range(
                        underlying=detail.contract,
                        min_expiry=min_expiry,
                        max_expiry=max_expiry,
                        exchange=contract.options_chain_exchange or contract.exchange,
                    )
                details.extend(option_contracts_detail)

        return details

    async def get_future_chain_details(self, underlying: IBContract) -> list[ContractDetails]:
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

    async def get_option_chain_details_by_range(
        self,
        underlying: IBContract,
        min_expiry: pd.Timestamp,
        max_expiry: pd.Timestamp,
        exchange: str | None = None,
    ) -> list[ContractDetails]:
        chains = await self._client.get_option_chains(underlying)

        if not chains:
            self._log.warning(
                f"No option chains available for {underlying.symbol}.{underlying.exchange}",
            )
            return []

        filtered_chains = [chain for chain in chains if chain[0] == (exchange or "SMART")]
        details = []

        for chain in filtered_chains:
            expirations = sorted(
                exp for exp in chain[1] if (min_expiry <= pd.Timestamp(exp, tz="UTC") <= max_expiry)
            )

            for expiration in expirations:
                option_contracts_detail = await self.get_option_chain_details_by_expiry(
                    underlying=underlying,
                    last_trading_date=expiration,
                    exchange=exchange,
                )
                details.extend(option_contracts_detail)

        return details

    async def get_option_chain_details_by_expiry(
        self,
        underlying: IBContract,
        last_trading_date: str,
        exchange: str,
    ) -> list[ContractDetails]:
        option_details_result = await self._client.get_contract_details(
            IBContract(
                secType=("FOP" if underlying.secType == "FUT" else "OPT"),
                symbol=underlying.symbol,
                lastTradeDateOrContractMonth=last_trading_date,
                exchange=exchange,
            ),
        )

        if not option_details_result:
            self._log.warning(
                f"No option contracts found for {underlying.symbol} expiring on {last_trading_date}",
            )
            return []

        # Handle both single list and nested list cases
        if len(option_details_result) == 1 and isinstance(option_details_result[0], list):
            option_details = option_details_result[0]
        else:
            option_details = option_details_result  # type: ignore[assignment]

        if option_details is None:
            self._log.warning(
                f"Option details is None for {underlying.symbol} expiring on {last_trading_date}",
            )
            return []

        option_details = [d for d in option_details if d.underConId == underlying.conId]  # type: ignore[assignment]
        self._log.info(
            f"Received {len(option_details)} Option Contracts for "
            f"{underlying.symbol}.{underlying.primaryExchange or underlying.exchange} expiring on {last_trading_date}",
        )
        self._log.debug(f"Got {option_details=}")

        return option_details

    def determine_venue_from_contract(self, contract: IBContract) -> str:
        """
        Determine the venue for a contract using the instrument provider configuration
        logic.

        Parameters
        ----------
        contract : IBContract
            The contract to determine the venue for.

        Returns
        -------
        str
            The determined venue.

        """
        # Use the exchange from the contract
        exchange = contract.primaryExchange if contract.exchange == "SMART" else contract.exchange
        venue = None

        if self._convert_exchange_to_mic_venue:
            # Check symbol-specific venue mapping first
            if self._symbol_to_mic_venue:
                for symbol_prefix, symbol_venue in self._symbol_to_mic_venue.items():
                    if contract.symbol.startswith(symbol_prefix):
                        venue = symbol_venue
                        break

            # If no symbol-specific mapping found, use VENUE_MEMBERS mapping
            if not venue:
                for venue_member, exchanges in VENUE_MEMBERS.items():
                    if exchange in exchanges:
                        venue = venue_member
                        break

        # Fall back to using the exchange as venue
        if not venue:
            venue = exchange

        return venue

    def _process_contract_details(
        self,
        contract_details: list[ContractDetails],
        venue: str,
        force_instrument_update: bool = False,
    ) -> list[InstrumentId]:
        """
        Process contract details and return the instrument IDs of successfully processed
        contracts.

        Parameters
        ----------
        contract_details : list[ContractDetails]
            The contract details to process.
        venue : str
            The venue for the contracts.
        force_instrument_update : bool, optional
            Whether to force update existing instruments.

        Returns
        -------
        list[InstrumentId]
            The instrument IDs of successfully processed contracts.

        """
        processed_instrument_ids = []

        for details in copy.deepcopy(contract_details):
            details.contract = IBContract(**details.contract.__dict__)
            details = IBContractDetails(**details.__dict__)
            self._log.debug(f"Attempting to create instrument from {details}")

            try:
                instrument: Instrument = parse_instrument(
                    details,
                    venue,
                    self.config.symbology_method,
                )
            except ValueError as e:
                self._log.error(f"{self.config.symbology_method=} failed to parse {details=}, {e}")
                continue

            if self.config.filter_callable is not None:
                filter_callable = resolve_path(self.config.filter_callable)

                if not filter_callable(instrument):
                    continue

            self._log.info(f"Adding {instrument=} from InteractiveBrokersInstrumentProvider")

            self.add(instrument)

            if not self._client._cache.instrument(instrument.id) or force_instrument_update:
                self._client._cache.add_instrument(instrument)

            self.contract[instrument.id] = details.contract
            self.contract_details[instrument.id] = details
            self.contract_id_to_instrument_id[details.contract.conId] = instrument.id

            # Add to the list of successfully processed instrument IDs
            processed_instrument_ids.append(instrument.id)

        return processed_instrument_ids
