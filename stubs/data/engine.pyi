from typing import Any, Callable, ClassVar, Union, Type, Tuple

from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.core.datetime import DateTime
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.data import Bar, QuoteTick, TradeTick, MarkPriceUpdate, IndexPriceUpdate, InstrumentStatus, InstrumentClose, CustomData, OrderBookDepth10, OrderBookDeltas
from nautilus_trader.data.messages import RequestInstruments, RequestInstrument, RequestOrderBookSnapshot, RequestQuoteTicks, RequestTradeTicks, RequestBars, DataCommand, RequestData, DataResponse
from nautilus_trader.data.messages import SubscribeData, UnsubscribeData, SubscribeInstruments, UnsubscribeInstruments, SubscribeInstrument, UnsubscribeInstrument, SubscribeOrderBook, UnsubscribeOrderBook, SubscribeQuoteTicks, UnsubscribeQuoteTicks, SubscribeTradeTicks, UnsubscribeTradeTicks, SubscribeMarkPrices, UnsubscribeMarkPrices, SubscribeIndexPrices, UnsubscribeIndexPrices, SubscribeBars, UnsubscribeBars, SubscribeInstrumentStatus, UnsubscribeInstrumentStatus, SubscribeInstrumentClose, UnsubscribeInstrumentClose
from nautilus_trader.model.enums import BookType # Added missing import
from nautilus_trader.common.component import Clock, Component # Updated to direct import
from nautilus_trader.data.client import DataClient, MarketDataClient # Updated to direct import
from nautilus_trader.cache.cache import Cache # Updated to direct import
from nautilus_trader.data.aggregation import BarAggregator, TimeBarAggregator, TickBarAggregator, VolumeBarAggregator, ValueBarAggregator # Updated to direct import


class DataEngine(Component):
    """
    Provides a high-performance data engine for managing many `DataClient`
    instances, for the asynchronous ingest of data.

    Parameters
    ----------
    msgbus : MessageBus
        The message bus for the engine.
    cache : Cache
        The cache for the engine.
    clock : Clock
        The clock for the engine.
    config : DataEngineConfig, optional
        The configuration for the instance.
    """

    debug: bool
    command_count: int
    data_count: int
    request_count: int
    response_count: int
    
    _time_bars_interval_type: Any
    _time_bars_timestamp_on_close: bool
    _time_bars_skip_first_non_full_bar: bool
    _time_bars_build_with_no_updates: bool
    _time_bars_origin_offset: dict
    _time_bars_build_delay: int
    _validate_data_sequence: bool
    _buffer_deltas: bool
    _cache: Cache
    _clients: dict[ClientId, DataClient]
    _routing_map: dict[Venue, DataClient]
    _default_client: DataClient | None
    _external_clients: set[ClientId]
    _catalogs: dict[str, ParquetDataCatalog]
    _order_book_intervals: dict[tuple[InstrumentId, int], list[Callable[[OrderBook], None]]]
    _bar_aggregators: dict[BarType, BarAggregator]
    _synthetic_quote_feeds: dict[InstrumentId, list[SyntheticInstrument]]
    _synthetic_trade_feeds: dict[InstrumentId, list[SyntheticInstrument]]
    _subscribed_synthetic_quotes: list[InstrumentId]
    _subscribed_synthetic_trades: list[InstrumentId]
    _buffered_deltas_map: dict[InstrumentId, list[OrderBookDelta]]
    _snapshot_info: dict[str, SnapshotInfo]
    _query_group_n_responses: dict[UUID4, int]
    _query_group_responses: dict[UUID4, list]

    def __init__(
        self,
        msgbus: MessageBus,
        cache: Cache,
        clock: Clock,
        config: DataEngineConfig | None = None,
    ) -> None: ...
    @property
    def registered_clients(self) -> list[ClientId]:
        """
        Return the execution clients registered with the engine.

        Returns
        -------
        list[ClientId]

        """
        ...
    @property
    def default_client(self) -> ClientId | None:
        """
        Return the default data client registered with the engine.

        Returns
        -------
        ClientId or ``None``

        """
        ...
    @property
    def routing_map(self) -> dict[Venue, DataClient]:
        """
        Return the default data client registered with the engine.

        Returns
        -------
        ClientId or ``None``

        """
        ...
    def connect(self) -> None:
        """
        Connect the engine by calling connect on all registered clients.
        """
        ...
    def disconnect(self) -> None:
        """
        Disconnect the engine by calling disconnect on all registered clients.
        """
        ...
    def check_connected(self) -> bool:
        """
        Check all of the engines clients are connected.

        Returns
        -------
        bool
            True if all clients connected, else False.

        """
        ...
    def check_disconnected(self) -> bool:
        """
        Check all of the engines clients are disconnected.

        Returns
        -------
        bool
            True if all clients disconnected, else False.

        """
        ...
    def register_catalog(self, catalog: ParquetDataCatalog, name: str = "catalog_0") -> None:
        """
        Register the given data catalog with the engine.

        Parameters
        ----------
        catalog : ParquetDataCatalog
            The data catalog to register.
        name : str, default 'catalog_0'
            The name of the catalog to register.

        """
        ...
    def register_client(self, client: DataClient) -> None:
        """
        Register the given data client with the data engine.

        Parameters
        ----------
        client : DataClient
            The client to register.

        Raises
        ------
        ValueError
            If `client` is already registered.

        """
        ...
    def register_default_client(self, client: DataClient) -> None:
        """
        Register the given client as the default routing client (when a specific
        venue routing cannot be found).

        Any existing default routing client will be overwritten.

        Parameters
        ----------
        client : DataClient
            The client to register.

        """
        ...
    def register_venue_routing(self, client: DataClient, venue: Venue) -> None:
        """
        Register the given client to route messages to the given venue.

        Any existing client in the routing map for the given venue will be
        overwrite.

        Parameters
        ----------
        venue : Venue
            The venue to route messages to.
        client : DataClient
            The client for the venue routing.

        """
        ...
    def deregister_client(self, client: DataClient) -> None:
        """
        Deregister the given data client from the data engine.

        Parameters
        ----------
        client : DataClient
            The data client to deregister.

        """
        ...
    def subscribed_custom_data(self) -> list[DataType]:
        """
        Return the custom data types subscribed to.

        Returns
        -------
        list[DataType]

        """
        ...
    def subscribed_instruments(self) -> list[InstrumentId]:
        """
        Return the instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_order_book_deltas(self) -> list[InstrumentId]:
        """
        Return the order book delta instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_order_book_snapshots(self) -> list[InstrumentId]:
        """
        Return the order book snapshot instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_quote_ticks(self) -> list[InstrumentId]:
        """
        Return the quote tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_trade_ticks(self) -> list[InstrumentId]:
        """
        Return the trade tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_mark_prices(self) -> list[InstrumentId]:
        """
        Return the mark price update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_index_prices(self) -> list[InstrumentId]:
        """
        Return the index price update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_bars(self) -> list[BarType]:
        """
        Return the bar types subscribed to.

        Returns
        -------
        list[BarType]

        """
        ...
    def subscribed_instrument_status(self) -> list[InstrumentId]:
        """
        Return the status update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_instrument_close(self) -> list[InstrumentId]:
        """
        Return the close price instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_synthetic_quotes(self) -> list[InstrumentId]:
        """
        Return the synthetic instrument quotes subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_synthetic_trades(self) -> list[InstrumentId]:
        """
        Return the synthetic instrument trades subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def _on_start(self) -> None: ...
    def _on_stop(self) -> None: ...
    def _start(self) -> None: ...
    def _stop(self) -> None: ...
    def _reset(self) -> None: ...
    def _dispose(self) -> None: ...
    def stop_clients(self) -> None:
        """
        Stop the registered clients.
        """
        ...
    def execute(self, command: DataCommand) -> None:
        """
        Execute the given data command.

        Parameters
        ----------
        command : DataCommand
            The command to execute.

        """
        ...
    def process(self, data: Data) -> None:
        """
        Process the given data.

        Parameters
        ----------
        data : Data
            The data to process.

        """
        ...
    def request(self, request: RequestData) -> None:
        """
        Handle the given request.

        Parameters
        ----------
        request : RequestData
            The request to handle.

        """
        ...
    def response(self, response: DataResponse) -> None:
        """
        Handle the given response.

        Parameters
        ----------
        response : DataResponse
            The response to handle.

        """
        ...
    def _execute_command(self, command: DataCommand) -> None: ...
    def _handle_subscribe(self, client: DataClient, command: SubscribeData) -> None: ...
    def _handle_unsubscribe(self, client: DataClient, command: UnsubscribeData) -> None: ...
    def _handle_subscribe_instruments(self, client: MarketDataClient, command: SubscribeInstruments) -> None: ...
    def _handle_subscribe_instrument(self, client: MarketDataClient, command: SubscribeInstrument) -> None: ...
    def _handle_subscribe_order_book_deltas(self, client: MarketDataClient, command: SubscribeOrderBook) -> None: ...
    def _handle_subscribe_order_book_depth(self, client: MarketDataClient, command: SubscribeOrderBook) -> None: ...
    def _handle_subscribe_order_book_snapshots(self, client: MarketDataClient, command: SubscribeOrderBook) -> None: ...
    def _setup_order_book(self, client: MarketDataClient, command: SubscribeOrderBook) -> None: ...
    def _create_new_book(self, instrument_id: InstrumentId, book_type: BookType) -> None: ...
    def _handle_subscribe_quote_ticks(self, client: MarketDataClient, command: SubscribeQuoteTicks) -> None: ...
    def _handle_subscribe_synthetic_quote_ticks(self, instrument_id: InstrumentId) -> None: ...
    def _handle_subscribe_trade_ticks(self, client: MarketDataClient, command: SubscribeTradeTicks) -> None: ...
    def _handle_subscribe_synthetic_trade_ticks(self, instrument_id: InstrumentId) -> None: ...
    def _handle_subscribe_mark_prices(self, client: MarketDataClient, command: SubscribeMarkPrices) -> None: ...
    def _handle_subscribe_index_prices(self, client: MarketDataClient, command: SubscribeIndexPrices) -> None: ...
    def _handle_subscribe_bars(self, client: MarketDataClient, command: SubscribeBars) -> None: ...
    def _handle_subscribe_data(self, client: DataClient, command: SubscribeData) -> None: ...
    def _handle_subscribe_instrument_status(self, client: MarketDataClient, command: SubscribeInstrumentStatus) -> None: ...
    def _handle_subscribe_instrument_close(self, client: MarketDataClient, command: SubscribeInstrumentClose) -> None: ...
    def _handle_unsubscribe_instruments(self, client: MarketDataClient, command: UnsubscribeInstruments) -> None: ...
    def _handle_unsubscribe_instrument(self, client: MarketDataClient, command: UnsubscribeInstrument) -> None: ...
    def _handle_unsubscribe_order_book_deltas(self, client: MarketDataClient, command: UnsubscribeOrderBook) -> None: ...
    def _handle_unsubscribe_order_book_snapshots(self, client: MarketDataClient, command: UnsubscribeOrderBook) -> None: ...
    def _handle_unsubscribe_quote_ticks(self, client: MarketDataClient, command: UnsubscribeQuoteTicks) -> None: ...
    def _handle_unsubscribe_trade_ticks(self, client: MarketDataClient, command: UnsubscribeTradeTicks) -> None: ...
    def _handle_unsubscribe_mark_prices(self, client: MarketDataClient, command: UnsubscribeMarkPrices) -> None: ...
    def _handle_unsubscribe_index_prices(self, client: MarketDataClient, command: UnsubscribeIndexPrices) -> None: ...
    def _handle_unsubscribe_bars(self, client: MarketDataClient, command: UnsubscribeBars) -> None: ...
    def _handle_unsubscribe_data(self, client: DataClient, command: UnsubscribeData) -> None: ...
    def _handle_unsubscribe_instrument_status(self, client: MarketDataClient, command: UnsubscribeInstrumentStatus) -> None: ...
    def _handle_unsubscribe_instrument_close(self, client: MarketDataClient, command: UnsubscribeInstrumentClose) -> None: ...
    def _handle_request(self, request: RequestData) -> None: ...
    def _handle_request_instruments(self, client: DataClient, request: RequestInstruments) -> None: ...
    def _handle_request_instrument(self, client: DataClient, request: RequestInstrument) -> None: ...
    def _handle_request_order_book_snapshot(self, client: DataClient, request: RequestOrderBookSnapshot) -> None: ...
    def _handle_request_quote_ticks(self, client: DataClient, request: RequestQuoteTicks) -> None: ...
    def _handle_request_trade_ticks(self, client: DataClient, request: RequestTradeTicks) -> None: ...
    def _handle_request_bars(self, client: DataClient, request: RequestBars) -> None: ...
    def _handle_request_data(self, client: DataClient, request: RequestData) -> None: ...
    def _handle_date_range_request(self, client: DataClient, request: RequestData) -> None: ...
    def _date_range_client_request(self, client: DataClient, request: RequestData) -> None: ...
    def _log_request_warning(self, request: RequestData) -> None: ...
    def _query_catalog(self, request: RequestData) -> None: ...
    def _handle_data(self, data: Data) -> None: ...
    def _handle_instrument(self, instrument: Instrument, update_catalog: bool = False, force_update_catalog: bool = False) -> None: ...
    def _handle_order_book_delta(self, delta: OrderBookDelta) -> None: ...
    def _handle_order_book_deltas(self, deltas: OrderBookDeltas) -> None: ...
    def _handle_order_book_depth(self, depth: OrderBookDepth10) -> None: ...
    def _handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def _handle_trade_tick(self, tick: TradeTick) -> None: ...
    def _handle_mark_price(self, mark_price: MarkPriceUpdate) -> None: ...
    def _handle_index_price(self, index_price: IndexPriceUpdate) -> None: ...
    def _handle_bar(self, bar: Bar) -> None: ...
    def _handle_instrument_status(self, data: InstrumentStatus) -> None: ...
    def _handle_close_price(self, data: InstrumentClose) -> None: ...
    def _handle_custom_data(self, data: CustomData) -> None: ...
    def _handle_response(self, response: DataResponse) -> None: ...
    def _new_query_group(self, correlation_id: UUID4, n_components: int) -> None: ...
    def _handle_query_group(self, response: DataResponse) -> DataResponse | None: ...
    def _check_bounds(self, response: DataResponse) -> None: ...
    def _update_catalog(
        self,
        ticks: list,
        data_cls: type,
        identifier: Any,
        start: int | None = None,
        end: int | None = None,
        is_instrument: bool = False,
        force_update_catalog: bool = False,
    ) -> None: ...
    def _catalog_last_timestamp(
        self,
        data_cls: Type,
        identifier = ...,
    ) -> Tuple[DateTime | None, object | None]: ...
    def _handle_instruments(self, instruments: list[Instrument], update_catalog: bool = False, force_update_catalog: bool = False) -> None: ...
    def _handle_quote_ticks(self, ticks: list[QuoteTick]) -> None: ...
    def _handle_trade_ticks(self, ticks: list[TradeTick]) -> None: ...
    def _handle_bars(self, bars: list[Bar], partial: Bar | None) -> None: ...
    def _handle_aggregated_bars(self, ticks: list, params: dict) -> dict: ...
    def _internal_update_instruments(self, instruments: list[Instrument]) -> None: ... # skip-validate
    def _update_order_book(self, data: Data) -> None: ...
    def _snapshot_order_book(self, snap_event: TimeEvent) -> None: ...
    def _publish_order_book(self, instrument_id: InstrumentId, topic: str) -> None: ...
    def _create_bar_aggregator(self, instrument: Instrument, bar_type: BarType) -> BarAggregator: ...
    def _start_bar_aggregator(self, client: MarketDataClient, command: SubscribeBars) -> None: ...
    def _stop_bar_aggregator(self, client: MarketDataClient, command: UnsubscribeBars) -> None: ...
    def _update_synthetics_with_quote(self, synthetics: list[SyntheticInstrument], update: QuoteTick) -> None: ...
    def _update_synthetic_with_quote(self, synthetic: SyntheticInstrument, update: QuoteTick) -> None: ...
    def _update_synthetics_with_trade(self, synthetics: list[SyntheticInstrument], update: TradeTick) -> None: ...
    def _update_synthetic_with_trade(self, synthetic: SyntheticInstrument, update: TradeTick) -> None: ...

