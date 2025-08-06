from typing import Any

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from stubs.accounting.accounts.base import Account
from stubs.cache.cache import Cache
from stubs.common.component import Clock
from stubs.common.component import Component
from stubs.common.component import MessageBus
from stubs.execution.messages import BatchCancelOrders
from stubs.execution.messages import CancelAllOrders
from stubs.execution.messages import CancelOrder
from stubs.execution.messages import ModifyOrder
from stubs.execution.messages import QueryOrder
from stubs.execution.messages import SubmitOrder
from stubs.execution.messages import SubmitOrderList
from stubs.model.events.account import AccountState
from stubs.model.events.order import OrderEvent
from stubs.model.identifiers import AccountId
from stubs.model.identifiers import ClientId
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import PositionId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import TradeId
from stubs.model.identifiers import Venue
from stubs.model.identifiers import VenueOrderId
from stubs.model.objects import AccountBalance
from stubs.model.objects import Currency
from stubs.model.objects import MarginBalance
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class ExecutionClient(Component):

    trader_id: Any
    venue: Any
    oms_type: Any
    account_id: Any
    account_type: Any
    base_currency: Any
    is_connected: bool

    def __init__(
        self,
        client_id: ClientId,
        venue: Venue | None,
        oms_type: OmsType,
        account_type: AccountType,
        base_currency: Currency | None,
        msgbus: MessageBus,
        cache: Cache,
        clock: Clock,
        config: NautilusConfig | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def _set_connected(self, value: bool = True) -> None: ...
    def _set_account_id(self, account_id: AccountId) -> None: ...
    def get_account(self) -> Account | None: ...
    def submit_order(self, command: SubmitOrder) -> None: ...
    def submit_order_list(self, command: SubmitOrderList) -> None: ...
    def modify_order(self, command: ModifyOrder) -> None: ...
    def cancel_order(self, command: CancelOrder) -> None: ...
    def cancel_all_orders(self, command: CancelAllOrders) -> None: ...
    def batch_cancel_orders(self, command: BatchCancelOrders) -> None: ...
    def query_order(self, command: QueryOrder) -> None: ...
    def generate_account_state(
        self,
        balances: list[AccountBalance],
        margins: list[MarginBalance],
        reported: bool,
        ts_event: int,
        info: dict[str, Any] | None = None,
    ) -> None: ...
    def generate_order_submitted(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        ts_event: int,
    ) -> None: ...
    def generate_order_rejected(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: str,
        ts_event: int,
    ) -> None: ...
    def generate_order_accepted(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> None: ...
    def generate_order_modify_rejected(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: str,
        ts_event: int,
    ) -> None: ...
    def generate_order_cancel_rejected(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: str,
        ts_event: int,
    ) -> None: ...
    def generate_order_updated(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Price,
        trigger_price: Price | None,
        ts_event: int,
        venue_order_id_modified: bool = False,
    ) -> None: ...
    def generate_order_canceled(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> None: ...
    def generate_order_triggered(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> None: ...
    def generate_order_expired(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> None: ...
    def generate_order_filled(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        venue_position_id: PositionId | None,
        trade_id: TradeId,
        order_side: OrderSide,
        order_type: OrderType,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Money,
        liquidity_side: LiquiditySide,
        ts_event: int,
        info: dict[str, Any] | None = None,
    ) -> None: ...
    def _send_account_state(self, account_state: AccountState) -> None: ...
    def _send_order_event(self, event: OrderEvent) -> None: ...
    def _send_mass_status_report(self, report: ExecutionMassStatus) -> None: ...
    def _send_order_status_report(self, report: OrderStatusReport) -> None: ...
    def _send_fill_report(self, report: FillReport) -> None: ...
