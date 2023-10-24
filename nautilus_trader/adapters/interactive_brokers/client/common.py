# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from collections.abc import Callable
from decimal import Decimal
from typing import Annotated, Any, NamedTuple

import msgspec

# fmt: off
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId


# fmt: on


class AccountOrderRef(NamedTuple):
    account: str
    order_id: str


class IBPosition(NamedTuple):
    account: str
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

    def __hash__(self):
        return hash((self.req_id, self.name))


class Subscription(msgspec.Struct, frozen=True):
    """
    Container for Subscription details.
    """

    req_id: Annotated[int, msgspec.Meta(gt=0)]
    name: str | tuple
    handle: Callable
    cancel: Callable
    last: Any

    def __hash__(self):
        return hash((self.req_id, self.name))


class Base:
    """
    Base class to maintain Request Id mapping for subscriptions and data requests.
    """

    def __init__(self):
        self._req_id_to_name: dict[int, str | tuple] = {}  # type: ignore
        self._req_id_to_handle: dict[int, Callable] = {}  # type: ignore
        self._req_id_to_cancel: dict[int, Callable] = {}  # type: ignore

    def __repr__(self):
        return f"{self.__class__.__name__}:\n{[self.get(req_id=k) for k in self._req_id_to_name]!r}"

    def _name_to_req_id(self, name: Any):
        try:
            return list(self._req_id_to_name.keys())[
                list(self._req_id_to_name.values()).index(name)
            ]
        except ValueError:
            pass

    def _validation_check(self, req_id: int, name: Any):
        if req_id in self._req_id_to_name:
            raise KeyError(
                f"Duplicate entry not allowed for {req_id=}, found {self.get(req_id=req_id)}",
            )
        elif name in self._req_id_to_name.values():
            raise KeyError(f"Duplicate entry not allowed for {name=}, found {self.get(name=name)}")

    def remove(
        self,
        req_id: int | None = None,
        name: InstrumentId | (BarType | str) | None = None,
    ):
        if not req_id:
            req_id = self._name_to_req_id(name)
        for d in [x for x in list(dir(self)) if x.startswith("_req_id_to_")]:
            getattr(self, d).pop(req_id, None)

    def get_all(self):
        result = []
        for req_id in self._req_id_to_name:
            result.append(self.get(req_id=req_id))
        return result

    def get(
        self,
        req_id: int | None = None,
        name: str | tuple | None = None,
    ):
        raise NotImplementedError("method must be implemented in the subclass")


class Subscriptions(Base):
    """
    Container for holding the Subscriptions.
    """

    def __init__(self):
        super().__init__()
        self._req_id_to_last: dict[int, Any] = {}  # type: ignore

    def add(
        self,
        req_id: int,
        name: str | tuple,
        handle: Callable,
        cancel: Callable = lambda: None,
    ):
        self._validation_check(req_id=req_id, name=name)
        self._req_id_to_name[req_id] = name
        self._req_id_to_handle[req_id] = handle
        self._req_id_to_cancel[req_id] = cancel
        self._req_id_to_last[req_id] = None
        return self.get(req_id=req_id)

    def get(
        self,
        req_id: int | None = None,
        name: str | tuple | None = None,
    ):
        if not req_id:
            req_id = self._name_to_req_id(name)
        if not req_id or not (name := self._req_id_to_name.get(req_id, None)):
            return
        return Subscription(
            req_id=req_id,
            name=name,
            last=self._req_id_to_last[req_id],
            handle=self._req_id_to_handle[req_id],
            cancel=self._req_id_to_cancel[req_id],
        )

    def update_last(self, req_id: int, value: Any):
        self._req_id_to_last[req_id] = value


class Requests(Base):
    """
    Container for holding the data Requests.
    """

    def __init__(self):
        super().__init__()
        self._req_id_to_future: dict[int, asyncio.Future] = {}  # type: ignore
        self._req_id_to_result: dict[int, Any] = {}  # type: ignore

    def get_futures(self):
        return self._req_id_to_future.values()

    def add(
        self,
        req_id: int,
        name: str | tuple,
        handle: Callable,
        cancel: Callable = lambda: None,
    ):
        self._validation_check(req_id=req_id, name=name)
        self._req_id_to_name[req_id] = name
        self._req_id_to_handle[req_id] = handle
        self._req_id_to_cancel[req_id] = cancel
        self._req_id_to_future[req_id] = asyncio.Future()
        self._req_id_to_result[req_id] = []
        return self.get(req_id=req_id)

    def get(
        self,
        req_id: int | None = None,
        name: str | tuple | None = None,
    ):
        if not req_id:
            req_id = self._name_to_req_id(name)
        if not req_id or not (name := self._req_id_to_name.get(req_id, None)):
            return
        return Request(
            req_id=req_id,
            name=name,
            handle=self._req_id_to_handle[req_id],
            cancel=self._req_id_to_cancel[req_id],
            future=self._req_id_to_future[req_id],
            result=self._req_id_to_result[req_id],
        )
