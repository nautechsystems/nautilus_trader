from __future__ import annotations

import sys
from dataclasses import dataclass
from datetime import timedelta
from typing import Any

from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId


if __name__ == "flux.execution.nautilus_adapter":
    sys.modules.setdefault(
        "nautilus_trader.flux.execution.nautilus_adapter",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.execution.nautilus_adapter":
    sys.modules.setdefault("flux.execution.nautilus_adapter", sys.modules[__name__])


def _required_text(value: str, field_name: str) -> str:
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


@dataclass(frozen=True, slots=True)
class ManagedOrderBinding:
    instrument_id: InstrumentId
    client_order_id: ClientOrderId | None = None
    venue_order_id: VenueOrderId | None = None

    def __post_init__(self) -> None:
        if self.client_order_id is None and self.venue_order_id is None:
            raise ValueError("managed order bindings require a client or venue order identifier")

    def matches(
        self,
        *,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId | None,
        venue_order_id: VenueOrderId | None,
    ) -> bool:
        if instrument_id != self.instrument_id:
            return False
        if self.client_order_id is not None and client_order_id == self.client_order_id:
            return True
        if self.venue_order_id is not None and venue_order_id == self.venue_order_id:
            return True
        return False


class ControllerManagedExecutionClientAdapter(LiveExecutionClient):
    def __init__(
        self,
        *,
        client: LiveExecutionClient,
        controller_scope_id: str,
        managed_instrument_ids: set[InstrumentId] | frozenset[InstrumentId],
        tracked_orders: tuple[ManagedOrderBinding, ...] | list[ManagedOrderBinding],
    ) -> None:
        self._client = client
        self.controller_scope_id = _required_text(controller_scope_id, "controller_scope_id")
        tracked = tuple(tracked_orders)
        managed = set(managed_instrument_ids)
        managed.update(binding.instrument_id for binding in tracked)
        self._managed_instrument_ids = frozenset(managed)
        self._tracked_orders = tracked

        super().__init__(
            loop=client._loop,
            client_id=client.id,
            venue=client.venue,
            oms_type=client.oms_type,
            account_type=client.account_type,
            base_currency=client.base_currency,
            instrument_provider=client._instrument_provider,
            msgbus=client._msgbus,
            cache=client._cache,
            clock=client._clock,
            config=getattr(client, "config", None),
        )
        if client.account_id is not None:
            self._set_account_id(client.account_id)

        if self._managed_instrument_ids:
            self.supports_startup_historical_order_status_reports = False
        else:
            self.supports_startup_historical_order_status_reports = getattr(
                client,
                "supports_startup_historical_order_status_reports",
                True,
            )

    def __getattr__(self, name: str) -> Any:
        return getattr(self._client, name)

    def connect(self) -> None:
        self._client.connect()

    def disconnect(self) -> None:
        self._client.disconnect()

    def _start(self) -> None:
        self._client.start()

    def _stop(self) -> None:
        self._client.stop()

    def _reset(self) -> None:
        self._client.reset()

    def _dispose(self) -> None:
        self._client.dispose()

    def submit_order(self, command: SubmitOrder) -> None:
        self._client.submit_order(command)

    def submit_order_list(self, command: SubmitOrderList) -> None:
        self._client.submit_order_list(command)

    def modify_order(self, command: ModifyOrder) -> None:
        self._client.modify_order(command)

    def cancel_order(self, command: CancelOrder) -> None:
        self._client.cancel_order(command)

    def cancel_all_orders(self, command: CancelAllOrders) -> None:
        self._client.cancel_all_orders(command)

    def batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        self._client.batch_cancel_orders(command)

    def query_account(self, command: QueryAccount) -> None:
        self._client.query_account(command)

    def query_order(self, command: QueryOrder) -> None:
        self._client.query_order(command)

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        report = await self._client.generate_order_status_report(command)
        if report is None:
            return None
        if self._is_visible_order_lineage(
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
        ):
            return report
        return None

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        reports = await self._client.generate_order_status_reports(command)
        return [report for report in reports if self._is_visible_order_report(report)]

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        fills = await self._client.generate_fill_reports(command)
        return [fill for fill in fills if self._is_visible_fill_report(fill)]

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        return await self._client.generate_position_status_reports(command)

    async def generate_mass_status(
        self,
        lookback_mins: int | None = None,
    ) -> ExecutionMassStatus | None:
        if not self._managed_instrument_ids:
            return await self._client.generate_mass_status(lookback_mins)

        self.reconciliation_active = True
        mass_status = ExecutionMassStatus(
            client_id=self.id,
            account_id=self.account_id,
            venue=self.venue,
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        since = None
        if lookback_mins is not None:
            since = self._clock.utc_now() - timedelta(minutes=lookback_mins)

        try:
            for instrument_id in sorted(self._managed_instrument_ids, key=str):
                order_reports = await self._client.generate_order_status_reports(
                    GenerateOrderStatusReports(
                        instrument_id=instrument_id,
                        start=since,
                        end=None,
                        open_only=True,
                        command_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                    ),
                )
                fill_reports = await self._client.generate_fill_reports(
                    GenerateFillReports(
                        instrument_id=instrument_id,
                        venue_order_id=None,
                        start=since,
                        end=None,
                        command_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                    ),
                )
                position_reports = await self._client.generate_position_status_reports(
                    GeneratePositionStatusReports(
                        instrument_id=instrument_id,
                        start=None,
                        end=None,
                        command_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                    ),
                )
                mass_status.add_order_reports(
                    reports=[
                        report
                        for report in order_reports
                        if self._is_visible_order_report(report)
                    ],
                )
                mass_status.add_fill_reports(
                    reports=[
                        fill
                        for fill in fill_reports
                        if self._is_visible_fill_report(fill)
                    ],
                )
                mass_status.add_position_reports(reports=position_reports)
        finally:
            self.reconciliation_active = False

        return mass_status

    def _is_visible_order_report(self, report: OrderStatusReport) -> bool:
        return self._is_visible_order_lineage(
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
        )

    def _is_visible_fill_report(self, report: FillReport) -> bool:
        return self._is_visible_order_lineage(
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
        )

    def _is_visible_order_lineage(
        self,
        *,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId | None,
        venue_order_id: VenueOrderId | None,
    ) -> bool:
        if instrument_id not in self._managed_instrument_ids:
            return True
        return any(
            binding.matches(
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
            )
            for binding in self._tracked_orders
        )


__all__ = [
    "ControllerManagedExecutionClientAdapter",
    "ManagedOrderBinding",
]
