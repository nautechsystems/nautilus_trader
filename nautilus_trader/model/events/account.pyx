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

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.functions cimport account_type_from_str
from nautilus_trader.model.functions cimport account_type_to_str
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport MarginBalance


cdef class AccountState(Event):
    """
    Represents an event which includes information on the state of the account.

    Parameters
    ----------
    account_id : AccountId
        The account ID (with the venue).
    account_type : AccountType
        The account type for the event.
    base_currency : Currency, optional
        The account base currency. Use None for multi-currency accounts.
    reported : bool
        If the state is reported from the exchange (otherwise system calculated).
    balances : list[AccountBalance]
        The account balances.
    margins : list[MarginBalance]
        The margin balances (can be empty).
    info : dict [str, object]
        The additional implementation specific account information.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the account state event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    Raises
    ------
    ValueError
        If `balances` is empty.
    """

    def __init__(
        self,
        AccountId account_id not None,
        AccountType account_type,
        Currency base_currency,
        bint reported,
        list balances not None,
        list margins not None,  # Can be empty
        dict info not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        Condition.not_empty(balances, "balances")

        self.account_id = account_id
        self.account_type = account_type
        self.base_currency = base_currency
        self.balances = balances
        self.margins = margins
        self.is_reported = reported
        self.info = info

        self._event_id = event_id
        self._ts_event = ts_event
        self._ts_init = ts_init

    def __eq__(self, Event other) -> bool:
        return self._event_id == other.id

    def __hash__(self) -> int:
        return hash(self._event_id)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"account_id={self.account_id.to_str()}, "
            f"account_type={account_type_to_str(self.account_type)}, "
            f"base_currency={self.base_currency}, "
            f"is_reported={self.is_reported}, "
            f"balances=[{', '.join([str(b) for b in self.balances])}], "
            f"margins=[{', '.join([str(m) for m in self.margins])}], "
            f"event_id={self._event_id.to_str()})"
        )

    @property
    def id(self) -> UUID4:
        """
        The event message identifier.

        Returns
        -------
        UUID4

        """
        return self._event_id

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the event occurred.

        Returns
        -------
        int

        """
        return self._ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._ts_init


    @staticmethod
    cdef AccountState from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str base_str = values["base_currency"]
        return AccountState(
            account_id=AccountId(values["account_id"]),
            account_type=account_type_from_str(values["account_type"]),
            base_currency=Currency.from_str_c(base_str) if base_str is not None else None,
            reported=values["reported"],
            balances=[AccountBalance.from_dict(b) for b in values["balances"]],
            margins=[MarginBalance.from_dict(m) for m in values["margins"]],
            info=values["info"],
            event_id=UUID4.from_str_c(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(AccountState obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "AccountState",
            "account_id": obj.account_id.to_str(),
            "account_type": account_type_to_str(obj.account_type),
            "base_currency": obj.base_currency.code if obj.base_currency else None,
            "balances": [b.to_dict() for b in obj.balances],
            "margins": [m.to_dict() for m in obj.margins],
            "reported": obj.is_reported,
            "info": obj.info,
            "event_id": obj._event_id.to_str(),
            "ts_event": obj._ts_event,
            "ts_init": obj._ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> AccountState:
        """
        Return an account state event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        AccountState

        """
        return AccountState.from_dict_c(values)

    @staticmethod
    def to_dict(AccountState obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return AccountState.to_dict_c(obj)
