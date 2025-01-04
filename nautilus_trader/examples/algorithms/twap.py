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


class TWAPExecAlgorithmConfig(ExecAlgorithmConfig, frozen=True):
    """
    Configuration for ``TWAPExecAlgorithm`` instances.

    This configuration class defines the necessary parameters for a Time-Weighted Average Price
    (TWAP) execution algorithm, which aims to execute orders evenly spread over a specified
    time horizon, at regular intervals.

    Parameters
    ----------
    exec_algorithm_id : ExecAlgorithmId
        The execution algorithm ID (will override default which is the class name).

    """

    exec_algorithm_id: ExecAlgorithmId | None = ExecAlgorithmId("TWAP")


class TWAPExecAlgorithm(ExecAlgorithm):
    """
    Provides a Time-Weighted Average Price (TWAP) execution algorithm.

    The TWAP execution algorithm aims to execute orders by evenly spreading them over a specified
    time horizon. The algorithm receives a primary order representing the total size and direction
    then splits this by spawning smaller child orders, which are then executed at regular intervals
    throughout the time horizon.

    This helps to reduce the impact of the full size of the primary order on the market, by
    minimizing the concentration of trade size at any given time.

    The algorithm will immediately submit the first order, with the final order submitted being the
    primary order at the end of the horizon period.

    Parameters
    ----------
    config : TWAPExecAlgorithmConfig, optional
        The configuration for the instance.

    """

    def __init__(self, config: TWAPExecAlgorithmConfig | None = None) -> None:
        if config is None:
            config = TWAPExecAlgorithmConfig()
        super().__init__(config)

        self._scheduled_sizes: dict[ClientOrderId, list[Quantity]] = {}

    def on_start(self) -> None:
        """
        Actions to be performed when the algorithm component is started.
        """
        # Optionally implement

    def on_stop(self) -> None:
        """
        Actions to be performed when the algorithm component is stopped.
        """
        self.clock.cancel_timers()

    def on_reset(self) -> None:
        """
        Actions to be performed when the algorithm component is reset.
        """
        self._scheduled_sizes.clear()

    def on_save(self) -> dict[str, bytes]:
        """
        Actions to be performed when the algorithm component is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        """
        return {}  # Optionally implement

    def on_load(self, state: dict[str, bytes]) -> None:
        """
        Actions to be performed when the algorithm component is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict[str, bytes]
            The algorithm component state dictionary.

        """
        # Optionally implement

    def round_decimal_down(self, amount: Decimal, precision: int) -> Decimal:
        return amount.quantize(Decimal(f"1e-{precision}"), rounding=ROUND_DOWN)

    def on_order(self, order: Order) -> None:
        """
        Actions to be performed when running and receives an order.

        Parameters
        ----------
        order : Order
            The order to be handled.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
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

        # Validate execution parameters
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

        # Calculate the number of intervals
        num_intervals: int = math.floor(horizon_secs / interval_secs)

        # Divide the order quantity evenly and determine any remainder
        quotient = order.quantity.as_decimal() / num_intervals
        floored_quotient = self.round_decimal_down(quotient, instrument.size_precision)
        qty_quotient = instrument.make_qty(floored_quotient)
        qty_per_interval = instrument.make_qty(qty_quotient)
        qty_remainder = order.quantity.as_decimal() - (floored_quotient * num_intervals)

        if (
            qty_per_interval == order.quantity
            or qty_per_interval < instrument.size_increment
            or (instrument.min_quantity and qty_per_interval < instrument.min_quantity)
        ):
            # Immediately submit first order for entire size
            self.log.warning(f"Submitting for entire size {qty_per_interval=}, {order.quantity=}")
            self.submit_order(order)
            return  # Done

        scheduled_sizes: list[Quantity] = [qty_per_interval] * num_intervals
        if qty_remainder:
            scheduled_sizes.append(instrument.make_qty(qty_remainder))

        assert sum(scheduled_sizes) == order.quantity
        self.log.info(f"Order execution size schedule: {scheduled_sizes}", LogColor.BLUE)

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

        # Set up timer
        self.clock.set_timer(
            name=order.client_order_id.value,
            interval=timedelta(seconds=interval_secs),
            callback=self.on_time_event,
        )
        self.log.info(
            f"Started TWAP execution for {order.client_order_id}: "
            f"{horizon_secs=}, {interval_secs=}",
            LogColor.BLUE,
        )

    def on_time_event(self, event: TimeEvent) -> None:
        """
        Actions to be performed when the algorithm receives a time event.

        Parameters
        ----------
        event : TimeEvent
            The time event received.

        """
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
        if not scheduled_sizes:  # Final quantity
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
        """
        Complete an execution sequence.

        Parameters
        ----------
        exec_spawn_id : ClientOrderId
            The execution spawn ID to complete.

        """
        if exec_spawn_id.value in self.clock.timer_names:
            self.clock.cancel_timer(exec_spawn_id.value)
        self._scheduled_sizes.pop(exec_spawn_id, None)
        self.log.info(f"Completed TWAP execution for {exec_spawn_id}", LogColor.BLUE)
