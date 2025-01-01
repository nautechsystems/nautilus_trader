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

from nautilus_trader.core.nautilus_pyo3 import CashAccount
from nautilus_trader.core.nautilus_pyo3 import MarginAccount
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Position
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.test_kit.rust.events_pyo3 import TestEventsProviderPyo3
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3
from nautilus_trader.test_kit.rust.orders_pyo3 import TestOrderProviderPyo3


class TestAccountingProviderPyo3:
    @staticmethod
    def margin_account() -> MarginAccount:
        return MarginAccount(
            event=TestEventsProviderPyo3.margin_account_state(),
            calculate_account_state=False,
        )

    @staticmethod
    def cash_account() -> CashAccount:
        return CashAccount(
            event=TestEventsProviderPyo3.cash_account_state(),
            calculate_account_state=False,
        )

    @staticmethod
    def cash_account_million_usd() -> CashAccount:
        return CashAccount(
            event=TestEventsProviderPyo3.cash_account_state_million_usd(),
            calculate_account_state=False,
        )

    @staticmethod
    def cash_account_multi() -> CashAccount:
        return CashAccount(
            event=TestEventsProviderPyo3.cash_account_state_multi(),
            calculate_account_state=False,
        )

    @staticmethod
    def long_position() -> Position:
        order = TestOrderProviderPyo3.market_order(
            instrument_id=TestIdProviderPyo3.audusd_id(),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
        )
        instrument = TestInstrumentProviderPyo3.audusd_sim()
        order_filled = TestEventsProviderPyo3.order_filled(
            instrument=instrument,
            order=order,
            position_id=TestIdProviderPyo3.position_id(),
            last_px=Price.from_str("1.00001"),
        )
        return Position(instrument=instrument, fill=order_filled)

    @staticmethod
    def short_position() -> Position:
        order = TestOrderProviderPyo3.market_order(
            instrument_id=TestIdProviderPyo3.audusd_id(),
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
        )
        instrument = TestInstrumentProviderPyo3.audusd_sim()
        order_filled = TestEventsProviderPyo3.order_filled(
            instrument=instrument,
            order=order,
            position_id=TestIdProviderPyo3.position_id(),
            last_px=Price.from_str("1.00001"),
        )
        return Position(instrument=TestInstrumentProviderPyo3.audusd_sim(), fill=order_filled)
