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
from nautilus_trader.core.nautilus_pyo3 import UUID4
from nautilus_trader.core.nautilus_pyo3 import OrderDenied
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3


class TestEventsProviderPyo3:
    @staticmethod
    def order_denied_max_submit_rate() -> OrderDenied:
        uuid = "91762096-b188-49ea-8562-8d8a4cc22ff2"
        return OrderDenied(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.audusd_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            reason="Exceeded MAX_ORDER_SUBMIT_RATE",
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
        )
