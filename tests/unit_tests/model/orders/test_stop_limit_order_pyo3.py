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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderStatus
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.test_kit.rust.orders_pyo3 import TestOrderProviderPyo3


AUDUSD_SIM = InstrumentId.from_str("AUD/USD.SIM")

stop_limit_order = TestOrderProviderPyo3.stop_limit_order(
    instrument_id=AUDUSD_SIM,
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(100_000),
    price=Price.from_str("1.00000"),
    trigger_price=Price.from_str("1.10010"),
    tags=["ENTRY"],
)


def test_initialize_stop_limit_order():
    assert stop_limit_order.order_type == OrderType.STOP_LIMIT
    assert stop_limit_order.expire_time is None
    assert stop_limit_order.status == OrderStatus.INITIALIZED
    assert stop_limit_order.time_in_force == TimeInForce.GTC
    assert stop_limit_order.has_price
    assert stop_limit_order.has_trigger_price
    assert stop_limit_order.is_passive
    assert not stop_limit_order.is_aggressive
    assert not stop_limit_order.is_closed
    assert (
        str(stop_limit_order)
        == "StopLimitOrder(BUY 100_000 AUD/USD.SIM STOP_LIMIT @ 1.10010-STOP[MID_POINT] 1.00000-LIMIT GTC, status=INITIALIZED, client_order_id=O-20210410-022422-001-001-1, venue_order_id=None, position_id=None, tags=ENTRY)"
    )
    assert (
        repr(stop_limit_order)
        == "StopLimitOrder(BUY 100_000 AUD/USD.SIM STOP_LIMIT @ 1.10010-STOP[MID_POINT] 1.00000-LIMIT GTC, status=INITIALIZED, client_order_id=O-20210410-022422-001-001-1, venue_order_id=None, position_id=None, tags=ENTRY)"
    )


def test_pyo3_cython_conversion():
    order_pyo3_dict = stop_limit_order.to_dict()
    stop_limit_order_cython = StopLimitOrder.from_pyo3(stop_limit_order)
    stop_limit_order_cython_dict = StopLimitOrder.to_dict(stop_limit_order_cython)
    stop_limit_order_pyo3_back = nautilus_pyo3.StopLimitOrder.from_dict(
        stop_limit_order_cython_dict,
    )
    assert order_pyo3_dict == stop_limit_order_cython_dict
    assert stop_limit_order == stop_limit_order_pyo3_back
