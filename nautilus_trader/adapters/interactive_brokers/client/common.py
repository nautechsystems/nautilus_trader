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

import asyncio
import functools
from abc import ABC
from abc import abstractmethod
from collections.abc import Callable
from decimal import Decimal
from typing import Annotated, Any, NamedTuple

import msgspec
from ibapi.client import EClient
from ibapi.commission_report import CommissionReport
from ibapi.common import BarData
from ibapi.execution import Execution

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import MessageBus
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId


class AccountOrderRef(NamedTuple):
    account_id: str
    order_id: str


class IBPosition(NamedTuple):
    account_id: str
    contract: IBContract
    quantity: Decimal
    avg_cost: float


class Request(msgspec.Struct, frozen=True):
    """
    Container for Data request details.
    """

    req_id: Annotated[int, msgspec.Meta(gt=0)]
    name: str | tuple
    handle: Callable
    cancel: Callable
    future: asyncio.Future
    result: list[Any]

    def __hash__(self) -> int:
        return hash((self.req_id, self.name))


class Subscription(msgspec.Struct, frozen=True):
    """
    Container for Subscription details.
    """

    req_id: Annotated[int, msgspec.Meta(gt=0)]
    name: str | tuple
    handle: functools.partial | Callable
    cancel: Callable
    last: Any

    def __hash__(self) -> int:
        return hash((self.req_id, self.name))


class Base(ABC):
    """
    Abstract base class to maintain Request Id mapping for subscriptions and data
    requests.
    """

    def __init__(self) -> None:
        self._req_id_to_name: dict[int, str | tuple] = {}
        self._req_id_to_handle: dict[int, Callable] = {}
        self._req_id_to_cancel: dict[int, Callable] = {}

    def __repr__(self) -> str:
        return f"{self.__class__.__name__}:\n{[self.get(req_id=k) for k in self._req_id_to_name]!r}"

    def _name_to_req_id(self, name: Any) -> int | None:
        """
        Map a given name to its corresponding request ID.

        Parameters
        ----------
        name : Any
            The name to find the corresponding request ID for.

        Returns
        -------
        str

        """
        for req_id, req_name in self._req_id_to_name.items():
            if req_name == name:
                return req_id

        return None

    def _validation_check(self, req_id: int, name: Any) -> None:
        """
        Validate that the provided request ID and name are not already in use.

        Parameters
        ----------
        req_id : int
            The request ID to validate.
        name : Any
            The name to validate.

        Raises
        ------
        KeyError
            If the request ID or name is already in use.

        """
        if req_id in self._req_id_to_name:
            existing = self.get(req_id=req_id)
            raise KeyError(f"Duplicate entry for {req_id=} not allowed, existing entry: {existing}")
        if name in self._req_id_to_name.values():
            existing = self.get(name=name)
            raise KeyError(f"Duplicate entry for {name=} not allowed, existing entry: {existing}")

    def add_req_id(
        self,
        req_id: int,
        name: str | tuple,
        handle: Callable,
        cancel: Callable,
    ) -> None:
        """
        Add a new request ID along with associated name, handle, and cancel callback to
        the mappings.

        Parameters
        ----------
        req_id : int
            The request ID to add.
        name : str | tuple
            The name associated with the request ID.
        handle : Callable
            The handler function for the request.
        cancel : Callable
            The cancel callback function for the request.

        """
        self._validation_check(req_id, name)
        self._req_id_to_name[req_id] = name
        self._req_id_to_handle[req_id] = handle
        self._req_id_to_cancel[req_id] = cancel

    def remove_req_id(self, req_id: int) -> None:
        """
        Remove a request ID and its associated mappings from the class.

        Parameters
        ----------
        req_id : int
            The request ID to remove.

        """
        self._req_id_to_name.pop(req_id, None)
        self._req_id_to_handle.pop(req_id, None)
        self._req_id_to_cancel.pop(req_id, None)

    def remove(
        self,
        req_id: int | None = None,
        name: InstrumentId | (BarType | str) | None = None,
    ) -> None:
        """
        Remove a request ID and its associated mappings, identified either by request ID
        or name.

        Parameters
        ----------
        req_id : int, optional
            The request ID to remove. If None, name is used to determine the request ID.
        name : InstrumentId | (BarType | str), optional
            The name associated with the request ID.

        """
        if req_id is None:
            req_id = self._name_to_req_id(name)

            if req_id is None:
                return  # If no matching req_id is found, exit the method

        self._req_id_to_name.pop(req_id, None)
        self._req_id_to_handle.pop(req_id, None)
        self._req_id_to_cancel.pop(req_id, None)

    def get_all(self) -> list[Request | Subscription]:
        """
        Retrieve all stored mappings as a list of their respective request or
        subscription objects.

        Returns
        -------
        list[Request | Subscription]

        """
        result: list = []

        for req_id in self._req_id_to_name:
            result.append(self.get(req_id=req_id))

        return result

    @abstractmethod
    def get(
        self,
        req_id: int | None = None,
        name: str | tuple | None = None,
    ) -> Request | Subscription | None:
        """
        Abstract method to retrieve a Request or Subscription object based on the
        request ID or name.

        Parameters
        ----------
        req_id : int
            The request ID of the object to retrieve. If None, name is used.
        name : str | tuple, optional
            The name associated with the request ID.

        Returns
        -------
        Request | Subscription | ``None``

        """


class Subscriptions(Base):
    """
    Manages and stores Subscriptions which are identified and accessed using request
    IDs.
    """

    def __init__(self) -> None:
        super().__init__()
        self._req_id_to_last: dict[int, Any] = {}

    def add(
        self,
        req_id: int,
        name: str | tuple,
        handle: Callable,
        cancel: Callable = lambda: None,
    ) -> Subscription | None:
        """
        Add a new subscription with the given request ID, name, handle, and optional
        cancel callback. This method stores the subscription details and initializes its
        'last' value to None. If a subscription with the given request ID already
        exists, it is overwritten.

        Parameters
        ----------
        req_id : int
            The request ID for the new subscription.
        name : str | tuple
            The name associated with the subscription.
        handle : Callable
            The handler function for the subscription.
        cancel : Callable, optional
            The cancel callback function for the subscription. Defaults to a no-op lambda.

        Returns
        -------
        Subscription | ``None``

        """
        super().add_req_id(req_id, name, handle, cancel)
        self._req_id_to_last[req_id] = None

        return self.get(req_id=req_id)

    def remove(self, req_id: int | None = None, name: str | tuple | None = None) -> None:
        """
        Remove a subscription identified by either its request ID or name. If the
        subscription is identified by name, the corresponding request ID is first
        determined. If neither req_id nor name is provided, or if the specified
        subscription is not found, no action is taken.

        Parameters
        ----------
        req_id : int, optional
            The request ID of the subscription to remove. If None, name is used.
        name : str | tuple, optional
            The name of the subscription to remove.

        """
        if not req_id:
            req_id = self._name_to_req_id(name)

        if req_id:
            super().remove_req_id(req_id)
            self._req_id_to_last.pop(req_id, None)

    def get(
        self,
        req_id: int | None = None,
        name: str | tuple | None = None,
    ) -> Subscription | None:
        """
        Retrieve a Subscription based on the request ID or name.

        Parameters
        ----------
        req_id : int, optional
            The request ID of the subscription to retrieve. If None, name is used.
        name : str | tuple, optional
            The name associated with the request ID.

        Returns
        -------
        Subscription | ``None``

        """
        if not req_id:
            req_id = self._name_to_req_id(name)

        if not req_id or not (name := self._req_id_to_name.get(req_id, None)):
            return None

        return Subscription(
            req_id=req_id,
            name=name,
            last=self._req_id_to_last[req_id],
            handle=self._req_id_to_handle[req_id],
            cancel=self._req_id_to_cancel[req_id],
        )

    def update_last(self, req_id: int, value: Any) -> None:
        """
        Update the 'last' value for a given subscription.

        Parameters
        ----------
        req_id : int
            The request ID of the subscription to update.
        value : Any
            The new value to set as the 'last' value for the subscription.

        """
        self._req_id_to_last[req_id] = value


class Requests(Base):
    """
    Manages and stores data requests, inheriting common functionalities from the Base
    class.

    Requests are identified and accessed using request IDs.

    """

    def __init__(self) -> None:
        super().__init__()
        self._req_id_to_future: dict[int, asyncio.Future] = {}
        self._req_id_to_result: dict[int, Any] = {}

    def get_futures(self) -> list[asyncio.Future]:
        """
        Retrieve all asyncio Futures associated with the stored requests.

        Returns
        -------
        list[asyncio.Future]

        """
        return list(self._req_id_to_future.values())

    def add(
        self,
        req_id: int,
        name: str | tuple,
        handle: Callable,
        cancel: Callable = lambda: None,
    ) -> Request | None:
        """
        Add a new data request with the specified request ID, name, handle, and an
        optional cancel callback. This method stores the data request details and
        initializes its future and result. If a data request with the given request ID
        already exists, it is overwritten.

        Parameters
        ----------
        req_id : int
            The request ID for the new data request.
        name : str | tuple
            The name associated with the data request.
        handle : Callable
            The handler function for the data request.
        cancel : Callable, optional
            The cancel callback function for the data request. Defaults to a no-op lambda.

        Returns
        -------
        Request | ``None``

        """
        super().add_req_id(req_id, name, handle, cancel)
        self._req_id_to_future[req_id] = asyncio.Future()
        self._req_id_to_result[req_id] = []

        return self.get(req_id=req_id)

    def remove(self, req_id: int | None = None, name: str | tuple | None = None) -> None:
        """
        Remove a data request identified by either its request ID or name. This method
        removes the data request details from the internal storage. If the data request
        is identified by name, the corresponding request ID is first determined. If
        neither req_id nor name is provided, or if the specified data request is not
        found, no action is taken.

        Parameters
        ----------
        req_id : int, optional
            The request ID of the data request to remove. If None, name is used.
        name : str | tuple, optional
            The name of the data request to remove.

        """
        if not req_id:
            req_id = self._name_to_req_id(name)

        if req_id:
            super().remove_req_id(req_id)
            self._req_id_to_future.pop(req_id, None)
            self._req_id_to_result.pop(req_id, None)

    def get(
        self,
        req_id: int | None = None,
        name: str | tuple | None = None,
    ) -> Request | None:
        """
        Retrieve a Request based on the request ID or name.

        Parameters
        ----------
        req_id : int, optional
            The request ID of the request to retrieve. If None, name is used.
        name : str | tuple, optional
            The name associated with the request ID.

        Returns
        -------
        Request | ``None``

        """
        if not req_id:
            req_id = self._name_to_req_id(name)

        if not req_id or not (name := self._req_id_to_name.get(req_id, None)):
            return None

        return Request(
            req_id=req_id,
            name=name,
            handle=self._req_id_to_handle[req_id],
            cancel=self._req_id_to_cancel[req_id],
            future=self._req_id_to_future[req_id],
            result=self._req_id_to_result[req_id],
        )


class BaseMixin:
    """
    Provide type hints for InteractiveBrokerClient Mixins.
    """

    # Client
    is_running: bool
    _loop: asyncio.AbstractEventLoop
    _log: Logger
    _cache: Cache
    _clock: LiveClock
    _msgbus: MessageBus
    _host: str
    _port: int
    _client_id: int
    _requests: Requests
    _instrument_provider: (
        Any  # InteractiveBrokersInstrumentProvider | None - Will be set by data/execution client
    )
    _subscriptions: Subscriptions
    _event_subscriptions: dict[str, Callable]
    _eclient: EClient
    _is_ib_connected: asyncio.Event
    _start: Callable
    _startup: Callable
    _reset: Callable
    _stop: Callable
    _resume: Callable
    _degrade: Callable
    _end_request: Callable
    _await_request: Callable
    _next_req_id: Callable
    _resubscribe_all: Callable
    _create_task: Callable
    logAnswer: Callable

    # Account
    accounts: Callable

    # Connection
    _reconnect_attempts: int
    _reconnect_delay: int
    _max_reconnect_attempts: int
    _indefinite_reconnect: bool
    _last_disconnection_ns: int | None

    # MarketData
    _bar_type_to_last_bar: dict[str, BarData | None]
    _bar_timeout_tasks: dict[str, Any]  # asyncio.Task
    _order_id_to_order_ref: dict[int, AccountOrderRef]

    # Order
    _next_valid_order_id: int
    _exec_id_details: dict[
        str,
        dict[str, Execution | (CommissionReport | str)],
    ]


class IBKRBookLevel(msgspec.Struct, frozen=True):
    """
    Single price level in the order book.

    Attributes
    ----------
    price : float
        Price at this level.
    size : Decimal
        Total size/quantity at this price.
    side : OrderSide
        Side of the order at this price.
    market_maker : str
        Market maker identifier providing this quote.

    """

    price: float
    size: Decimal
    side: OrderSide
    market_maker: str
