import asyncio
import datetime as dt
from collections.abc import Callable
from concurrent.futures import Executor
from typing import Any

from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.common.executor import TaskId
from nautilus_trader.model.enums import BookType
from stubs.cache.base import CacheFacade
from stubs.common.component import Clock
from stubs.common.component import Component
from stubs.common.component import MessageBus
from stubs.core.data import Data
from stubs.core.message import Event
from stubs.core.uuid import UUID4
from stubs.data.messages import DataResponse
from stubs.indicators.base.indicator import Indicator
from stubs.model.book import OrderBook
from stubs.model.data import Bar
from stubs.model.data import BarType
from stubs.model.data import DataType
from stubs.model.data import IndexPriceUpdate
from stubs.model.data import InstrumentClose
from stubs.model.data import InstrumentStatus
from stubs.model.data import MarkPriceUpdate
from stubs.model.data import OrderBookDepth10
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick
from stubs.model.greeks import GreeksCalculator
from stubs.model.identifiers import ClientId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Venue
from stubs.model.instruments.base import Instrument
from stubs.model.instruments.synthetic import SyntheticInstrument
from stubs.portfolio.base import PortfolioFacade

class Actor(Component):

    msgbus: MessageBus | None
    cache: CacheFacade | None
    clock: Clock | None
    greeks: GreeksCalculator | None
    log: Any # Logger type from Component
    config: ActorConfig
    trader_id: UUID4 | None
    _log_events: bool
    _log_commands: bool
    _warning_events: set[type]
    _pending_requests: dict[UUID4, Callable[[UUID4], None] | None]
    _pyo3_conversion_types: set[type]
    _signal_classes: dict[str, type]
    _indicators: list[Indicator]
    _indicators_for_quotes: dict[InstrumentId, list[Indicator]]
    _indicators_for_trades: dict[InstrumentId, list[Indicator]]
    _indicators_for_bars: dict[BarType, list[Indicator]]

    def __init__(self, config: ActorConfig | None = None) -> None: ...
    def to_importable_config(self) -> ImportableActorConfig: ...
    def on_save(self) -> dict[str, bytes]: ...
    def on_load(self, state: dict[str, bytes]) -> None: ...
    def on_start(self) -> None: ...
    def on_stop(self) -> None: ...
    def on_resume(self) -> None: ...
    def on_reset(self) -> None: ...
    def on_dispose(self) -> None: ...
    def on_degrade(self) -> None: ...
    def on_fault(self) -> None: ...
    def on_instrument_status(self, data: InstrumentStatus) -> None: ...
    def on_instrument_close(self, update: InstrumentClose) -> None: ...
    def on_instrument(self, instrument: Instrument) -> None: ...
    def on_order_book(self, order_book: OrderBook) -> None: ...
    def on_order_book_deltas(self, deltas) -> None: # pyo3 type hint
        ...
    def on_order_book_depth(self, depth) -> None: ...
    def on_quote_tick(self, tick: QuoteTick) -> None: ...
    def on_trade_tick(self, tick: TradeTick) -> None: ...
    def on_mark_price(self, mark_price: MarkPriceUpdate) -> None: ...
    def on_index_price(self, index_price: IndexPriceUpdate) -> None: ...
    def on_bar(self, bar: Bar) -> None: ...
    def on_data(self, data: Data) -> None: ...
    def on_signal(self, signal) -> None: ...
    def on_historical_data(self, data: Data) -> None: ...
    def on_event(self, event: Event) -> None: ...
    @property
    def registered_indicators(self) -> list[Indicator]: ...
    def indicators_initialized(self) -> bool: ...
    def register_base(
        self,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: CacheFacade,
        clock: Clock,
    ) -> None: ...
    def register_executor(
        self,
        loop: asyncio.AbstractEventLoop,
        executor: Executor,
    ) -> None: ...
    def register_warning_event(self, event: type) -> None: ...
    def deregister_warning_event(self, event: type) -> None: ...
    def register_indicator_for_quote_ticks(self, instrument_id: InstrumentId, indicator: Indicator) -> None: ...
    def register_indicator_for_trade_ticks(self, instrument_id: InstrumentId, indicator: Indicator) -> None: ...
    def register_indicator_for_bars(self, bar_type: BarType, indicator: Indicator) -> None: ...
    def save(self) -> dict[str, bytes]: ...
    def load(self, state: dict[str, bytes]) -> None: ...
    def add_synthetic(self, synthetic: SyntheticInstrument) -> None: ...
    def update_synthetic(self, synthetic: SyntheticInstrument) -> None: ...
    def queue_for_executor(
        self,
        func: Callable[..., Any],
        args: tuple | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> TaskId: ...
    def run_in_executor(
        self,
        func: Callable[..., Any],
        args: tuple | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> TaskId: ...
    def queued_task_ids(self) -> list[TaskId]: ...
    def active_task_ids(self) -> list[TaskId]: ...
    def has_queued_tasks(self) -> bool: ...
    def has_active_tasks(self) -> bool: ...
    def has_any_tasks(self) -> bool: ...
    def cancel_task(self, task_id: TaskId) -> None: ...
    def cancel_all_tasks(self) -> None: ...
    def _start(self) -> None: ...
    def _stop(self) -> None: ...
    def _resume(self) -> None: ...
    def _reset(self) -> None: ...
    def _dispose(self) -> None: ...
    def _degrade(self) -> None: ...
    def _fault(self) -> None: ...
    def subscribe_data(
        self,
        data_type: DataType,
        client_id: ClientId | None = None,
        instrument_id: InstrumentId | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_instruments(
        self,
        venue: Venue,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_instrument(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType = ...,
        depth: int = 0,
        client_id: ClientId | None = None,
        managed: bool = True,
        pyo3_conversion: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_order_book_depth(
        self,
        instrument_id: InstrumentId,
        book_type: BookType = ...,
        depth: int = 0,
        client_id: ClientId | None = None,
        managed: bool = True,
        pyo3_conversion: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_order_book_at_interval(
        self,
        instrument_id: InstrumentId,
        book_type: BookType = ...,
        depth: int = 0,
        interval_ms: int = 1000,
        client_id: ClientId | None = None,
        managed: bool = True,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_quote_ticks(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_trade_ticks(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_mark_prices(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_index_prices(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_bars(
        self,
        bar_type: BarType,
        client_id: ClientId | None = None,
        await_partial: bool = False,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_instrument_status(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def subscribe_instrument_close(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_data(
        self,
        data_type: DataType,
        client_id: ClientId | None = None,
        instrument_id: InstrumentId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_instruments(
        self,
        venue: Venue,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_instrument(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_order_book_depth(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_order_book_at_interval(
        self,
        instrument_id: InstrumentId,
        interval_ms: int = 1000,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_quote_ticks(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_trade_ticks(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_mark_prices(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_index_prices(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_bars(
        self,
        bar_type: BarType,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def unsubscribe_instrument_status(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def publish_data(self, data_type: DataType, data: Data) -> None: ...
    def publish_signal(self, name: str, value, ts_event: int = 0) -> None: ...
    def subscribe_signal(self, name: str = "") -> None: ...
    def request_data(
        self,
        data_type: DataType,
        client_id: ClientId,
        instrument_id: InstrumentId | None = None,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        limit: int = 0,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4: ...
    def request_instrument(
        self,
        instrument_id: InstrumentId,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4: ...
    def request_instruments(
        self,
        venue: Venue,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4: ...
    def request_order_book_snapshot(
        self,
        instrument_id: InstrumentId,
        limit: int = 0,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        params: dict[str, Any] | None = None,
    ) -> UUID4: ...
    def request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        limit: int = 0,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4: ...
    def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        limit: int = 0,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4: ...
    def request_bars(
        self,
        bar_type: BarType,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        limit: int = 0,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4: ...
    def request_aggregated_bars(
        self,
        bar_types: list[BarType],
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        limit: int = 0,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        include_external_data: bool = False,
        update_subscriptions: bool = False,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4: ...
    def is_pending_request(self, request_id: UUID4) -> bool: ...
    def has_pending_requests(self) -> bool: ...
    def pending_requests(self) -> set[UUID4]: ...
    def handle_instrument(self, instrument: Instrument) -> None: ...
    def handle_instruments(self, instruments: list[Instrument]) -> None: ...
    def handle_order_book_deltas(self, deltas) -> None: ...
    def handle_order_book_depth(self, depth: OrderBookDepth10) -> None: ...
    def handle_order_book(self, order_book: OrderBook) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_quote_ticks(self, ticks: list[QuoteTick]) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_mark_price(self, mark_price: MarkPriceUpdate) -> None: ...
    def handle_index_price(self, index_price: IndexPriceUpdate) -> None: ...
    def handle_trade_ticks(self, ticks: list[TradeTick]) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def handle_bars(self, bars: list[Bar]) -> None: ...
    def handle_instrument_status(self, data: InstrumentStatus) -> None: ...
    def handle_instrument_close(self, update: InstrumentClose) -> None: ...
    def handle_data(self, data: Data) -> None: ...
    def handle_signal(self, signal: Data) -> None: ...
    def handle_historical_data(self, data: Data) -> None: ...
    def handle_event(self, event: Event) -> None: ...
    def _handle_data_response(self, response: DataResponse) -> None: ...
    def _handle_instrument_response(self, response: DataResponse) -> None: ...
    def _handle_instruments_response(self, response: DataResponse) -> None: ...
    def _handle_quote_ticks_response(self, response: DataResponse) -> None: ...
    def _handle_trade_ticks_response(self, response: DataResponse) -> None: ...
    def _handle_bars_response(self, response: DataResponse) -> None: ...
    def _handle_aggregated_bars_response(self, response: DataResponse) -> None: ...
    def _finish_response(self, request_id: UUID4) -> None: ...
    def _handle_indicators_for_quote(self, indicators: list[Indicator], tick: QuoteTick) -> None: ...
    def _handle_indicators_for_trade(self, indicators: list[Indicator], tick: TradeTick) -> None: ...
    def _handle_indicators_for_bar(self, indicators: list[Indicator], bar: Bar) -> None: ...

