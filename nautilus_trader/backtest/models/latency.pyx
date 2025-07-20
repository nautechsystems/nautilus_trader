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

import random

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport NANOSECONDS_IN_MILLISECOND
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.functions cimport liquidity_side_to_str
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class LatencyModel:
    """
    Provides a latency model for simulated exchange message I/O.

    Parameters
    ----------
    base_latency_nanos : int, default 1_000_000_000
        The base latency (nanoseconds) for the model.
    insert_latency_nanos : int, default 0
        The order insert latency (nanoseconds) for the model.
    update_latency_nanos : int, default 0
        The order update latency (nanoseconds) for the model.
    cancel_latency_nanos : int, default 0
        The order cancel latency (nanoseconds) for the model.
    config : FillModelConfig, optional
        The configuration for the model.

    Raises
    ------
    ValueError
        If `base_latency_nanos` is negative (< 0).
    ValueError
        If `insert_latency_nanos` is negative (< 0).
    ValueError
        If `update_latency_nanos` is negative (< 0).
    ValueError
        If `cancel_latency_nanos` is negative (< 0).
    """

    def __init__(
        self,
        uint64_t base_latency_nanos = NANOSECONDS_IN_MILLISECOND,
        uint64_t insert_latency_nanos = 0,
        uint64_t update_latency_nanos = 0,
        uint64_t cancel_latency_nanos = 0,
        config = None,
    ) -> None:
        if config is not None:
            # Initialize from config
            base_latency_nanos = config.base_latency_nanos
            insert_latency_nanos = config.insert_latency_nanos
            update_latency_nanos = config.update_latency_nanos
            cancel_latency_nanos = config.cancel_latency_nanos

        Condition.not_negative_int(base_latency_nanos, "base_latency_nanos")
        Condition.not_negative_int(insert_latency_nanos, "insert_latency_nanos")
        Condition.not_negative_int(update_latency_nanos, "update_latency_nanos")
        Condition.not_negative_int(cancel_latency_nanos, "cancel_latency_nanos")

        self.base_latency_nanos = base_latency_nanos
        self.insert_latency_nanos = base_latency_nanos + insert_latency_nanos
        self.update_latency_nanos = base_latency_nanos + update_latency_nanos
        self.cancel_latency_nanos = base_latency_nanos + cancel_latency_nanos
