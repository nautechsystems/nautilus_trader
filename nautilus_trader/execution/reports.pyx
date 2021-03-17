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

from nautilus_trader.core.correctness cimport Condition


cdef class ExecutionStateReport:
    """
    Represents a report of execution state by order identifier.
    """

    def __init__(
        self,
        str name not None,
        AccountId account_id not None,
        dict order_states not None,
        dict order_filled not None,
        dict position_states not None,
    ):
        """
        Initializes a new instance of the `ExecutionStateReport` class.

        Parameters
        ----------
        name : str
            The client name for the report.
        account_id : AccountId
            The account identifier for the report.
        order_states : dict[OrderId, OrderState]
            The order states for the venue.
        order_filled : dict[OrderId, OrderEvent]
            The order fill info for the venue.
        position_states : dict[InstrumentId, Decimal]
            The position states for the venue.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        Condition.valid_string(name, "name")

        self.name = name
        self.account_id = account_id
        self.order_states = order_states
        self.order_filled = order_filled
        self.position_states = position_states
