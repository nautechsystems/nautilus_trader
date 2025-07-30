from typing import Any, Callable, ClassVar

from nautilus_trader.core.nautilus_pyo3 import UUID4, BarType, MessageBus, OrderBook, OrderBookDelta, SyntheticInstrument
from nautilus_trader.core.nautilus_pyo3 import ClientId
from nautilus_trader.core.nautilus_pyo3 import Data
from nautilus_trader.core.nautilus_pyo3 import DataType
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from stubs.cache.cache import Cache
from stubs.common.component import Clock, Component
from stubs.data.aggregation import BarAggregator
from stubs.data.client import DataClient
from stubs.data.messages import DataCommand, DataResponse, RequestData

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
    
    _time_bars_interval_type: ClassVar[Any]
    _time_bars_timestamp_on_close: ClassVar[bool]
    _time_bars_skip_first_non_full_bar: ClassVar[bool]
    _time_bars_build_with_no_updates: ClassVar[bool]
    _time_bars_origin_offset: ClassVar[dict]
    _time_bars_build_delay: ClassVar[int]
    _validate_data_sequence: ClassVar[bool]
    _buffer_deltas: ClassVar[bool]
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
        overwritten.

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