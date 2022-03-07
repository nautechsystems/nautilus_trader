from typing import Optional

from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
from tests.test_kit.stubs.component import TestComponentStubs
from tests.test_kit.stubs.identifiers import TestIdStubs


class TestCommandStubs:
    @staticmethod
    def submit_order_command(order: Order):
        return SubmitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            position_id=TestIdStubs.position_id(),
            order=order,
            command_id=TestIdStubs.uuid(),
            ts_init=TestComponentStubs.clock().timestamp_ns(),
        )

    @staticmethod
    def modify_order_command(
        instrument_id: Optional[InstrumentId] = None,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
        quantity: Optional[Quantity] = None,
        price: Optional[Price] = None,
    ):
        return ModifyOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            client_order_id=client_order_id or TestIdStubs.client_order_id(),
            venue_order_id=venue_order_id or TestIdStubs.venue_order_id(),
            quantity=quantity,
            price=price,
            trigger_price=None,
            command_id=TestIdStubs.uuid(),
            ts_init=TestComponentStubs.clock().timestamp_ns(),
        )

    @staticmethod
    def cancel_order_command(
        instrument_id: Optional[InstrumentId] = None,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ):
        return CancelOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            client_order_id=client_order_id or TestIdStubs.client_order_id(),
            venue_order_id=venue_order_id or TestIdStubs.venue_order_id(),
            command_id=TestIdStubs.uuid(),
            ts_init=TestComponentStubs.clock().timestamp_ns(),
        )
