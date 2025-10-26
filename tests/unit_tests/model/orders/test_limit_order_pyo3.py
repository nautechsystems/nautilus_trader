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
from nautilus_trader.core.nautilus_pyo3 import ExecAlgorithmId
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderStatus
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.test_kit.rust.orders_pyo3 import TestOrderProviderPyo3


AUDUSD_SIM = InstrumentId.from_str("AUD/USD.SIM")


def test_initialize_limit_order():
    order = TestOrderProviderPyo3.limit_order(
        instrument_id=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00000"),
        exec_algorithm_id=ExecAlgorithmId("TWAP"),
    )

    # Assert
    assert order.order_type == OrderType.LIMIT
    assert order.expire_time is None  # GTC orders don't have expiry
    assert order.status == OrderStatus.INITIALIZED
    assert order.time_in_force == TimeInForce.GTC
    assert order.has_price
    assert not order.has_trigger_price
    assert order.is_passive
    assert not order.is_open
    assert not order.is_aggressive
    assert not order.is_closed
    assert not order.is_emulated
    assert order.is_active_local
    assert order.is_primary
    assert not order.is_spawned
    assert (
        str(order)
        == "LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, status=INITIALIZED, "
        + "client_order_id=O-20210410-022422-001-001-1, venue_order_id=None, position_id=None, "
        + "exec_algorithm_id=TWAP, exec_spawn_id=O-20210410-022422-001-001-1, tags=None)"
    )
    assert (
        repr(order)
        == "LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, status=INITIALIZED, "
        + "client_order_id=O-20210410-022422-001-001-1, venue_order_id=None, position_id=None, "
        + "exec_algorithm_id=TWAP, exec_spawn_id=O-20210410-022422-001-001-1, tags=None)"
    )


def test_pyo3_cython_conversion():
    limit_order_pyo3 = TestOrderProviderPyo3.limit_order(
        instrument_id=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00000"),
    )
    limit_order_pyo3_dict = limit_order_pyo3.to_dict()
    limit_order_cython = LimitOrder.from_pyo3(limit_order_pyo3)
    limit_order_cython_dict = LimitOrder.to_dict(limit_order_cython)
    limit_order_pyo3_back = nautilus_pyo3.LimitOrder.from_dict(limit_order_cython_dict)
    assert limit_order_pyo3_dict == limit_order_cython_dict
    assert limit_order_pyo3 == limit_order_pyo3_back
