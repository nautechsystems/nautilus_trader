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


from nautilus_trader.core.nautilus_pyo3 import OrderDenied
from nautilus_trader.test_kit.rust.events_pyo3 import TestEventsProviderPyo3


def test_order_denied():
    event = TestEventsProviderPyo3.order_denied_max_submit_rate()
    result_dict = OrderDenied.to_dict(event)
    order_denied = OrderDenied.from_dict(result_dict)
    assert order_denied == event
    assert (
        str(event)
        == "OrderDenied(instrument_id=AUD/USD.SIM, client_order_id=O-20210410-022422-001-001-1, "
        + "reason=Exceeded MAX_ORDER_SUBMIT_RATE)"
    )
    assert (
        repr(event)
        == "OrderDenied(trader_id=TESTER-000, strategy_id=S-001, "
        + "instrument_id=AUD/USD.SIM, client_order_id=O-20210410-022422-001-001-1, "
        + "reason=Exceeded MAX_ORDER_SUBMIT_RATE, event_id=91762096-b188-49ea-8562-8d8a4cc22ff2, ts_init=0)"
    )
