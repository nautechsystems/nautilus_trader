# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import math
from datetime import timedelta
from decimal import ROUND_DOWN
from decimal import Decimal

from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.events import TimeEvent
from nautilus_trader.config import ExecAlgorithmConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.execution.algorithm import ExecAlgorithm
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order


class VWAPExecAlgorithmConfig(ExecAlgorithmConfig, frozen=True):
    """
    Configuration for ``VWAPExecAlgorithm`` instances.

    This configuration class defines the necessary parameters for a Volume-Weighted Average Price
    (VWAP) execution algorithm, which aims to execute orders weighted by a volume profile over
    a specified time horizon, at regular intervals.

    Parameters
    ----------
    exec_algorithm_id : ExecAlgorithmId
        The execution algorithm ID (will override default which is the class name).

    """

    exec_algorithm_id: ExecAlgorithmId | None = ExecAlgorithmId("VWAP")


def _default_volume_profile(n: int) -> list[float]:
    if n <= 1:
        return [1.0]
    mid = (n - 1) / 2.0
    return [1.0 + abs(i - mid) for i in range(n)]


class VWAPExecAlgorithm(ExecAlgorithm):
    """
    Provides a Volume-Weighted Average Price (VWAP) execution algorithm.

    The VWAP execution algorithm aims to execute orders by distributing them according to a
    volume profile over a specified time horizon. Instead of splitting equally like TWAP, each
    interval's child order size is proportional to the corresponding weight in the volume profile.

    A default U-shaped volume profile is used when no explicit weights are provided, reflecting
    the typical intraday volume pattern observed in most liquid markets (higher volume at open
    and close, lower volume midday).

    The algorithm will immediately submit the first order, with the final order submitted being
    the primary order at the end of the horizon period.

    Parameters
    ----------
    config : VWAPExecAlgorithmConfig, optional
        The configuration for the instance.

    """

    def __init__(self, config: VWAPExecAlgorithmConfig | None = None) -> None:
        if config is None:
            config = VWAPExecAlgorithmConfig()
        super().__init__(config)

        self._scheduled_sizes: dict[ClientOrderId, list[Quantity]] = {}

    def on_start(self) -> None:
        pass

    def on_stop(self) -> None:
        self.clock.cancel_timers()

    def on_reset(self) -> None:
        self._scheduled_sizes.clear()

    def on_save(self) -> dict[str, bytes]:
        return {}

    def on_load(self, state: dict[str, bytes]) -> None:
        pass

    def round_decimal_down(self, amount: Decimal, precision: int) -> Decimal:
        return amount.quantize(Decimal(f"1e-{precision}"), rounding=ROUND_DOWN)

    def on_order(self, order: Order) -> None:
        PyCondition.not_in(
            order.client_order_id,
            self._scheduled_sizes,
            "order.client_order_id",
            "self._scheduled_sizes",
        )
        self.log.info(repr(order), LogColor.CYAN)

        if order.order_type != OrderType.MARKET:
            self.log.error(
                f"Cannot execute order: only implemented for market orders, {order.order_type=}",
            )
            return

        instrument = self.cache.instrument(order.instrument_id)
        if not instrument:
            self.log.error(
                f"Cannot execute order: instrument {order.instrument_id} not found",
            )
            return

        exec_params = order.exec_algorithm_params
        if not exec_params:
            self.log.error(
                f"Cannot execute order: "
                f"`exec_algorithm_params` not found for primary order {order!r}",
            )
            return

        horizon_secs = exec_params.get("horizon_secs")
        if not horizon_secs:
            self.log.error(
                f"Cannot execute order: "
                f"`horizon_secs` not found in `exec_algorithm_params` {exec_params}",
            )
            return

        interval_secs = exec_params.get("interval_secs")
        if not interval_secs:
            self.log.error(
                f"Cannot execute order: "
                f"`interval_secs` not found in `exec_algorithm_params` {exec_params}",
            )
            return

        if horizon_secs < interval_secs:
            self.log.error(
                f"Cannot execute order: {horizon_secs=} was less than {interval_secs=}",
            )
            return

        num_intervals: int = math.floor(horizon_secs / interval_secs)

        raw_weights = exec_params.get("volume_weights")
        if raw_weights:
            if len(raw_weights) != num_intervals:
                self.log.error(
                    f"Cannot execute order: volume_weights length {len(raw_weights)} "
                    f"does not match {num_intervals=}",
                )
                return
            if any(w < 0 for w in raw_weights):
                self.log.error("Cannot execute order: volume_weights must be non-negative")
                return
            weights = [float(w) for w in raw_weights]
        else:
            weights = _default_volume_profile(num_intervals)

        weight_sum = sum(weights)
        if weight_sum <= 0:
            self.log.error("Cannot execute order: volume_weights sum must be positive")
            return

        total_qty = order.quantity.as_decimal()
        raw_sizes = [(Decimal(str(w / weight_sum)) * total_qty) for w in weights]

        floored_sizes = [
            self.round_decimal_down(s, instrument.size_precision) for s in raw_sizes
        ]

        allocated = sum(floored_sizes)
        remainder = total_qty - allocated

        scheduled_sizes: list[Quantity] = [instrument.make_qty(s) for s in floored_sizes]

        if remainder > 0:
            scheduled_sizes.append(instrument.make_qty(remainder))

        if any(
            s < instrument.size_increment
            or (instrument.min_quantity and s < instrument.min_quantity)
            for s in scheduled_sizes
        ):
            self.log.warning(
                f"Interval size below minimum, submitting for entire size: {order.quantity=}",
            )
            self.submit_order(order)
            return

        assert sum(scheduled_sizes) == order.quantity
        self.log.info(f"VWAP execution size schedule: {scheduled_sizes}", LogColor.BLUE)

        self._scheduled_sizes[order.client_order_id] = scheduled_sizes
        first_qty: Quantity = scheduled_sizes.pop(0)

        spawned_order: MarketOrder = self.spawn_market(
            primary=order,
            quantity=first_qty,
            time_in_force=order.time_in_force,
            reduce_only=order.is_reduce_only,
            tags=order.tags,
        )

        self.submit_order(spawned_order)

        self.clock.set_timer(
            name=order.client_order_id.value,
            interval=timedelta(seconds=interval_secs),
            callback=self.on_time_event,
        )
        self.log.info(
            f"Started VWAP execution for {order.client_order_id}: "
            f"{horizon_secs=}, {interval_secs=}, intervals={num_intervals}",
            LogColor.BLUE,
        )

    def on_time_event(self, event: TimeEvent) -> None:
        self.log.info(repr(event), LogColor.CYAN)

        exec_spawn_id = ClientOrderId(event.name)

        primary: Order = self.cache.order(exec_spawn_id)
        if not primary:
            self.log.error(f"Cannot find primary order for {exec_spawn_id=}")
            return

        if primary.is_closed:
            self.complete_sequence(primary.client_order_id)
            return

        instrument: Instrument = self.cache.instrument(primary.instrument_id)
        if not instrument:
            self.log.error(
                f"Cannot execute order: instrument {primary.instrument_id} not found",
            )
            return

        scheduled_sizes = self._scheduled_sizes.get(exec_spawn_id)
        if scheduled_sizes is None:
            self.log.error(f"Cannot find scheduled sizes for {exec_spawn_id=}")
            return

        if not scheduled_sizes:
            self.log.warning(f"No more size to execute for {exec_spawn_id=}")
            return

        quantity: Quantity = instrument.make_qty(scheduled_sizes.pop(0))
        if not scheduled_sizes:
            self.submit_order(primary)
            self.complete_sequence(primary.client_order_id)
            return

        spawned_order: MarketOrder = self.spawn_market(
            primary=primary,
            quantity=quantity,
            time_in_force=primary.time_in_force,
            reduce_only=primary.is_reduce_only,
            tags=primary.tags,
        )

        self.submit_order(spawned_order)

    def complete_sequence(self, exec_spawn_id: ClientOrderId) -> None:
        if exec_spawn_id.value in self.clock.timer_names:
            self.clock.cancel_timer(exec_spawn_id.value)
        self._scheduled_sizes.pop(exec_spawn_id, None)
        self.log.info(f"Completed VWAP execution for {exec_spawn_id}", LogColor.BLUE)
