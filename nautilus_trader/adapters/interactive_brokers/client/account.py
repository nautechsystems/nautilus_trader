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
from decimal import Decimal

# fmt: off
from ibapi.account_summary_tags import AccountSummaryTags
from ibapi.utils import current_fn_name
from ibapi.wrapper import EWrapper

from nautilus_trader.adapters.interactive_brokers.client.common import IBPosition
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.common.enums import LogColor
from nautilus_trader.model.position import Position


# fmt: on


class InteractiveBrokersAccountManager(EWrapper):
    """
    Manages IB accounts for the InteractiveBrokersClient. It extends the EWrapper
    interface, handling various account and position related requests and responses.

    Parameters
    ----------
    client : InteractiveBrokersClient
        The client instance that will be used to communicate with the TWS API.

    """

    def __init__(self, client):
        self._client = client
        self._eclient = client._eclient
        self._log = client._log

        self.account_ids: set[str] = set()

    def accounts(self) -> set[str]:
        """
        Return a set of account identifiers managed by this instance.

        Returns
        -------
        str

        """
        return self.account_ids.copy()

    def subscribe_account_summary(self) -> None:
        """
        Subscribe to the account summary for all accounts. It sends a request to
        Interactive Brokers to retrieve account summary information.

        Returns
        -------
        None

        """
        name = "accountSummary"
        if not (subscription := self._client.subscriptions.get(name=name)):
            req_id = self._client.next_req_id()
            subscription = self._client.subscriptions.add(
                req_id=req_id,
                name=name,
                handle=functools.partial(
                    self._eclient.reqAccountSummary,
                    reqId=req_id,
                    groupName="All",
                    tags=AccountSummaryTags.AllTags,
                ),
                cancel=functools.partial(
                    self._eclient.cancelAccountSummary,
                    reqId=req_id,
                ),
            )
        # Allow fetching all tags upon request even if already subscribed
        subscription.handle()

    def unsubscribe_account_summary(self, account_id: str) -> None:
        """
        Unsubscribe from the account summary for the specified account. This method is
        not implemented.

        Parameters
        ----------
        account_id : str
            The identifier of the account to unsubscribe from.

        Returns
        -------
        None

        """
        raise NotImplementedError

    def position(
        self,
        account_id: str,
        contract: IBContract,
        position: Decimal,
        avg_cost: float,
    ) -> None:
        """
        Process position data for an account.

        Parameters
        ----------
        account_id : str
            The account identifier
        contract : IBContract
            The contract details for the position.
        position : Decimal
            The quantity of the position.
        avg_cost : float
            The average cost of the position.

        Returns
        -------
        None

        """
        self._client.logAnswer(current_fn_name(), vars())
        if request := self._client.requests.get(name="OpenPositions"):
            request.result.append(IBPosition(account_id, contract, position, avg_cost))

    async def get_positions(self, account_id: str) -> Position:
        """
        Fetch open positions for a specified account.

        Parameters
        ----------
        account_id: str
            The account identifier for which to fetch positions.

        Returns
        -------
        None

        Returns
        -------
            A list of Position objects representing open positions for the specified account.

        """
        self._log.debug(f"Requesting Open Positions for {account_id}")
        name = "OpenPositions"
        if not (request := self._client.requests.get(name=name)):
            request = self._client.requests.add(
                req_id=self._client.next_req_id(),
                name=name,
                handle=self._eclient.reqPositions,
            )
            request.handle()
            all_positions = await self._client.await_request(request, 30)
        else:
            all_positions = await self._client.await_request(request, 30)
        positions = []
        for position in all_positions:
            if position.account == account_id:
                positions.append(position)
        return positions

    # -- EWrapper overrides -----------------------------------------------------------------------
    def accountSummary(
        self,
        req_id: int,
        account_id: str,
        tag: str,
        value: str,
        currency: str,
    ) -> None:
        """
        Receive account information.
        """
        self._client.logAnswer(current_fn_name(), vars())
        name = f"accountSummary-{account_id}"
        if handler := self._client.event_subscriptions.get(name, None):
            handler(tag, value, currency)

    def managedAccounts(self, accounts_list: str) -> None:
        """
        Receive a comma-separated string with the managed account ids.

        Occurs automatically on initial API client connection.

        """
        self._client.logAnswer(current_fn_name(), vars())
        self.account_ids = {a for a in accounts_list.split(",") if a}
        if (
            self._client.order_manager.next_valid_order_id >= 0
            and not self._client.is_ib_ready.is_set()
        ):
            self._log.info("`is_ib_ready` set by managedAccounts", LogColor.BLUE)
            self._client.is_ib_ready.set()

    def positionEnd(self) -> None:
        """
        Indicate that all the positions have been transmitted.
        """
        self._client.logAnswer(current_fn_name(), vars())
        if request := self._client.requests.get(name="OpenPositions"):
            self._client.end_request(request.req_id)