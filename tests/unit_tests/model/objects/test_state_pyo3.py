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

from nautilus_trader.core.nautilus_pyo3 import AccountState
from nautilus_trader.test_kit.rust.events_pyo3 import TestEventsProviderPyo3


################################################################################
# Account state
################################################################################


def test_cash_account_state():
    cash_account_state = TestEventsProviderPyo3.cash_account_state()
    result_dict = cash_account_state.to_dict()
    assert cash_account_state == AccountState.from_dict(result_dict)
    assert result_dict == {
        "type": "AccountState",
        "account_id": "SIM-000",
        "account_type": "CASH",
        "base_currency": "USD",
        "balances": [
            {
                "type": "AccountBalance",
                "free": "1500000.00",
                "locked": "25000.00",
                "total": "1525000.00",
                "currency": "USD",
            },
        ],
        "event_id": "91762096-b188-49ea-8562-8d8a4cc22ff2",
        "margins": [],
        "reported": True,
        "info": {},
        "ts_init": 0,
        "ts_event": 0,
    }


def test_margin_account_state():
    margin_account_state = TestEventsProviderPyo3.margin_account_state()
    result_dict = margin_account_state.to_dict()
    assert margin_account_state == AccountState.from_dict(result_dict)
    assert result_dict == {
        "type": "AccountState",
        "account_id": "SIM-000",
        "account_type": "MARGIN",
        "base_currency": "USD",
        "balances": [
            {
                "type": "AccountBalance",
                "free": "1500000.00",
                "locked": "25000.00",
                "total": "1525000.00",
                "currency": "USD",
            },
        ],
        "margins": [
            {
                "type": "MarginBalance",
                "instrument_id": "AUD/USD.SIM",
                "initial": "1.00",
                "maintenance": "1.00",
                "currency": "USD",
            },
        ],
        "event_id": "91762096-b188-49ea-8562-8d8a4cc22ff2",
        "reported": True,
        "info": {},
        "ts_init": 0,
        "ts_event": 0,
    }
