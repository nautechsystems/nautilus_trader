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

import functools
from typing import Any

from ibapi.common import SetOfFloat
from ibapi.common import SetOfString
from ibapi.contract import ContractDetails

from nautilus_trader.adapters.interactive_brokers.client.common import BaseMixin
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails


class InteractiveBrokersClientContractMixin(BaseMixin):
    """
    Handles contracts (instruments) for the InteractiveBrokersClient.

    This class provides methods to request contract details, matching contracts, and
    option chains. The InteractiveBrokersInstrumentProvider class uses methods defined
    in this class to request the data it needs.

    """

    async def get_contract_details(self, contract: IBContract) -> list[IBContractDetails] | None:
        """
        Request details for a specific contract.

        Parameters
        ----------
        contract : IBContract
            The contract for which details are requested.

        Returns
        -------
        IBContractDetails | ``None``

        """
        name = str(contract)
        if not (request := self._requests.get(name=name)):
            req_id = self._next_req_id()
            request = self._requests.add(
                req_id=req_id,
                name=name,
                handle=functools.partial(
                    self._eclient.reqContractDetails,
                    reqId=req_id,
                    contract=contract,
                ),
            )
            if not request:
                return None
            request.handle()
            return await self._await_request(request, 10, supress_timeout_warning=True)
        else:
            return await self._await_request(request, 10, supress_timeout_warning=True)

    async def get_matching_contracts(self, pattern: str) -> list[IBContract] | None:
        """
        Request contracts matching a specific pattern.

        Parameters
        ----------
        pattern : str
            The pattern to match for contract symbols.

        Returns
        -------
        list[IBContract] | ``None``

        """
        name = f"MatchingSymbols-{pattern}"
        if not (request := self._requests.get(name=name)):
            req_id = self._next_req_id()
            request = self._requests.add(
                req_id=req_id,
                name=name,
                handle=functools.partial(
                    self._eclient.reqMatchingSymbols,
                    reqId=req_id,
                    pattern=pattern,
                ),
            )
            if not request:
                return None
            request.handle()
            return await self._await_request(request, 20)
        else:
            self._log.info(f"Request already exist for {request}")
            return None

    async def get_option_chains(self, underlying: IBContract) -> Any | None:
        """
        Request option chains for a specific underlying contract.

        Parameters
        ----------
        underlying : IBContract
            The underlying contract for which option chains are requested.

        Returns
        -------
        list[IBContractDetails] | ``None``

        """
        name = f"OptionChains-{underlying!s}"
        if not (request := self._requests.get(name=name)):
            req_id = self._next_req_id()
            request = self._requests.add(
                req_id=req_id,
                name=name,
                handle=functools.partial(
                    self._eclient.reqSecDefOptParams,
                    reqId=req_id,
                    underlyingSymbol=underlying.symbol,
                    futFopExchange=underlying.exchange if underlying.secType == "FUT" else "",
                    underlyingSecType=underlying.secType,
                    underlyingConId=underlying.conId,
                ),
            )
            if not request:
                return None
            request.handle()
            return await self._await_request(request, 20)
        else:
            self._log.info(f"Request already exist for {request}")
            return None

    async def process_contract_details(
        self,
        *,
        req_id: int,
        contract_details: ContractDetails,
    ) -> None:
        """
        Receive the full contract's definitions This method will return all
        contracts matching the requested via EClientSocket::reqContractDetails.
        For example, one can obtain the whole option chain with it.
        """
        if not (request := self._requests.get(req_id=req_id)):
            return
        request.result.append(contract_details)

    async def process_contract_details_end(self, *, req_id: int) -> None:
        """
        After all contracts matching the request were returned, this method will mark
        the end of their reception.
        """
        self._end_request(req_id)

    async def process_security_definition_option_parameter(
        self,
        *,
        req_id: int,
        exchange: str,
        underlying_con_id: int,
        trading_class: str,
        multiplier: str,
        expirations: SetOfString,
        strikes: SetOfFloat,
    ) -> None:
        """
        Return the option chain for an underlying on an exchange specified in
        reqSecDefOptParams There will be multiple callbacks to
        securityDefinitionOptionParameter if multiple exchanges are specified in
        reqSecDefOptParams.
        """
        if request := self._requests.get(req_id=req_id):
            request.result.append((exchange, expirations))

    async def process_security_definition_option_parameter_end(self, *, req_id: int) -> None:
        """
        Call when all callbacks to securityDefinitionOptionParameter are complete.
        """
        self._end_request(req_id)

    async def process_symbol_samples(
        self,
        *,
        req_id: int,
        contract_descriptions: list,
    ) -> None:
        """
        Return an array of sample contract descriptions.
        """
        if request := self._requests.get(req_id=req_id):
            for contract_description in contract_descriptions:
                request.result.append(IBContract(**contract_description.contract.__dict__))
            self._end_request(req_id)
