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

import orjson

from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.account_type cimport AccountTypeParser
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.objects cimport AccountBalance


cdef class AccountState(Event):
    """
    Represents an event which includes information on the state of the account.
    """

    def __init__(
        self,
        AccountId account_id not None,
        AccountType account_type,
        Currency base_currency,
        bint reported,
        list balances not None,
        dict info not None,
        UUID event_id not None,
        int64_t ts_updated_ns,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``AccountState`` class.

        Parameters
        ----------
        account_id : AccountId
            The account ID.
        account_type : AccountId
            The account type for the event.
        base_currency : Currency, optional
            The account base currency. Use None for multi-currency accounts.
        reported : bool
            If the state is reported from the exchange (otherwise system calculated).
        balances : list[AccountBalance]
            The account balances
        info : dict [str, object]
            The additional implementation specific account information.
        event_id : UUID
            The event ID.
        ts_updated_ns : int64
            The UNIX timestamp (nanoseconds) when the account was updated.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        """
        Condition.not_empty(balances, "balances")
        super().__init__(event_id, timestamp_ns)

        self.account_id = account_id
        self.account_type = account_type
        self.base_currency = base_currency
        self.balances = balances
        self.is_reported = reported
        self.info = info
        self.ts_updated_ns = ts_updated_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"account_id={self.account_id.value}, "
                f"account_type={AccountTypeParser.to_str(self.account_type)}, "
                f"base_currency={self.base_currency}, "
                f"is_reported={self.is_reported}, "
                f"balances=[{', '.join([str(b) for b in self.balances])}], "
                f"event_id={self.id})")

    @staticmethod
    cdef AccountState from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str base_str = values["base_currency"]
        return AccountState(
            account_id=AccountId.from_str_c(values["account_id"]),
            account_type=AccountTypeParser.from_str(values["account_type"]),
            base_currency=Currency.from_str_c(base_str) if base_str is not None else None,
            reported=values["reported"],
            balances=[AccountBalance.from_dict(b) for b in orjson.loads(values["balances"])],
            info=orjson.loads(values["info"]),
            event_id=UUID.from_str_c(values["event_id"]),
            ts_updated_ns=values["ts_updated_ns"],
            timestamp_ns=values["timestamp_ns"],
        )

    @staticmethod
    cdef dict to_dict_c(AccountState obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "AccountState",
            "account_id": obj.account_id.value,
            "account_type": AccountTypeParser.to_str(obj.account_type),
            "base_currency": obj.base_currency.code if obj.base_currency else None,
            "balances": orjson.dumps([b.to_dict() for b in obj.balances]),
            "reported": obj.is_reported,
            "info": orjson.dumps(obj.info),
            "event_id": obj.id.value,
            "ts_updated_ns": obj.ts_updated_ns,
            "timestamp_ns": obj.timestamp_ns,
        }

    @staticmethod
    def from_dict(dict values):
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
