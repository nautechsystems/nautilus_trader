from datetime import datetime
from typing import Any

from nautilus_trader.cache.config import CacheConfig
from stubs.accounting.accounts.base import Account
from stubs.cache.facade import CacheDatabaseFacade
from stubs.common.actor import Actor
from stubs.core.uuid import UUID4
from stubs.model.identifiers import AccountId
from stubs.model.identifiers import ClientId
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import ComponentId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import PositionId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import TraderId
from stubs.model.identifiers import VenueOrderId
from stubs.model.instruments.base import Instrument
from stubs.model.instruments.synthetic import SyntheticInstrument
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.orders.base import Order
from stubs.model.position import Position
from stubs.serialization.base import Serializer
from stubs.trading.strategy import Strategy

class CacheDatabaseAdapter(CacheDatabaseFacade):
    """
    Provides a generic cache database adapter.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the adapter.
    instance_id : UUID4
        The instance ID for the adapter.
    serializer : Serializer
        The serializer for database operations.
    config : CacheConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `CacheConfig`.

    Warnings
    --------
    Redis can only accurately store int64 types to 17 digits of precision.
    Therefore nanosecond timestamp int64's with 19 digits will lose 2 digits of
    precision when persisted. One way to solve this is to ensure the serializer
    converts timestamp int64's to strings on the way into Redis, and converts
    timestamp strings back to int64's on the way out. One way to achieve this is
    to set the `timestamps_as_str` flag to true for the `MsgSpecSerializer`, as
    per the default implementations for both `TradingNode` and `BacktestEngine`.
    """

    def __init__(
        self,
        trader_id: TraderId,
        instance_id: UUID4,
        serializer: Serializer,
        config: CacheConfig | None = None,
    ) -> None: ...
    def close(self) -> None:
        """
        Close the backing database adapter.

        """
        ...
    def flush(self) -> None:
        """
        Flush the database which clears all data.

        """
        ...
    def keys(self, pattern: str = "*") -> list[str]:
        """
        Return all keys in the database matching the given `pattern`.

        Parameters
        ----------
        pattern : str, default '*'
            The glob-style pattern to match against the keys in the database.

        Returns
        -------
        list[str]

        Raises
        ------
        ValueError
            If `pattern` is not a valid string.

        Warnings
        --------
        Using the default '*' pattern string can have serious performance implications and
        can take a long time to execute if many keys exist in the database. This operation
        can lead to high memory and CPU usage, and should be used with caution, especially
        in production environments.

        """
        ...
    def load_all(self) -> dict[str, dict]:
        """
        Load all cache data from the database.

        Returns
        -------
        dict[str, dict]
            A dictionary containing all cache data organized by category.

        """
        ...
    def load(self) -> dict[str, bytes]:
        """
        Load all general objects from the database.

        Returns
        -------
        dict[str, bytes]

        """
        ...
    def load_currencies(self) -> dict[str, Currency]:
        """
        Load all currencies from the database.

        Returns
        -------
        dict[str, Currency]

        """
        ...
    def load_instruments(self) -> dict[InstrumentId, Instrument]:
        """
        Load all instruments from the database.

        Returns
        -------
        dict[InstrumentId, Instrument]

        """
        ...
    def load_synthetics(self) -> dict[InstrumentId, SyntheticInstrument]:
        """
        Load all synthetic instruments from the database.

        Returns
        -------
        dict[InstrumentId, SyntheticInstrument]

        """
        ...
    def load_accounts(self) -> dict[AccountId, Account]:
        """
        Load all accounts from the database.

        Returns
        -------
        dict[AccountId, Account]

        """
        ...
    def load_orders(self) -> dict[ClientOrderId, Order]:
        """
        Load all orders from the database.

        Returns
        -------
        dict[ClientOrderId, Order]

        """
        ...
    def load_positions(self) -> dict[PositionId, Position]:
        """
        Load all positions from the database.

        Returns
        -------
        dict[PositionId, Position]

        """
        ...
    def load_index_order_position(self) -> dict[ClientOrderId, PositionId]:
        """
        Load the order to position index from the database.

        Returns
        -------
        dict[ClientOrderId, PositionId]

        """
        ...
    def load_index_order_client(self) -> dict[ClientOrderId, ClientId]:
        """
        Load the order to execution client index from the database.

        Returns
        -------
        dict[ClientOrderId, ClientId]

        """
        ...
    def load_currency(self, code: str) -> Currency | None:
        """
        Load the currency associated with the given currency code (if found).

        Parameters
        ----------
        code : str
            The currency code to load.

        Returns
        -------
        Currency or ``None``

        """
        ...
    def load_instrument(self, instrument_id: InstrumentId) -> Instrument | None:
        """
        Load the instrument associated with the given instrument ID
        (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.

        Returns
        -------
        Instrument or ``None``

        """
        ...
    def load_synthetic(self, instrument_id: InstrumentId) -> SyntheticInstrument | None:
        """
        Load the synthetic instrument associated with the given synthetic instrument ID
        (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The synthetic instrument ID to load.

        Returns
        -------
        SyntheticInstrument or ``None``

        Raises
        ------
        ValueError
            If `instrument_id` is not for a synthetic instrument.

        """
        ...
    def load_account(self, account_id: AccountId) -> Account | None:
        """
        Load the account associated with the given account ID (if found).

        Parameters
        ----------
        account_id : AccountId
            The account ID to load.

        Returns
        -------
        Account or ``None``

        """
        ...
    def load_order(self, client_order_id: ClientOrderId) -> Order | None:
        """
        Load the order associated with the given client order ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to load.

        Returns
        -------
        Order or ``None``

        """
        ...
    def load_position(self, position_id: PositionId) -> Position | None:
        """
        Load the position associated with the given ID (if found).

        Parameters
        ----------
        position_id : PositionId
            The position ID to load.

        Returns
        -------
        Position or ``None``

        """
        ...
    def load_actor(self, component_id: ComponentId) -> dict[str, Any]:
        """
        Load the state for the given actor.

        Parameters
        ----------
        component_id : ComponentId
            The ID of the actor state dictionary to load.

        Returns
        -------
        dict[str, Any]

        """
        ...
    def delete_actor(self, component_id: ComponentId) -> None:
        """
        Delete the given actor from the database.

        Parameters
        ----------
        component_id : ComponentId
            The ID of the actor state dictionary to delete.

        """
        ...
    def load_strategy(self, strategy_id: StrategyId) -> dict[str, bytes]:
        """
        Load the state for the given strategy.

        Parameters
        ----------
        strategy_id : StrategyId
            The ID of the strategy state dictionary to load.

        Returns
        -------
        dict[str, bytes]

        """
        ...
    def delete_strategy(self, strategy_id: StrategyId) -> None:
        """
        Delete the given strategy from the database.

        Parameters
        ----------
        strategy_id : StrategyId
            The ID of the strategy state dictionary to delete.

        """
        ...
    def delete_order(self, client_order_id: ClientOrderId) -> None:
        """
        Delete the given order from the database.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to delete.

        """
        ...
    def delete_position(self, position_id: PositionId) -> None:
        """
        Delete the given position from the database.

        Parameters
        ----------
        position_id : PositionId
            The position ID to delete.

        """
        ...
    def delete_account_event(self, account_id: AccountId, event_id: str) -> None:
        """
        Delete the given account event from the database.

        Parameters
        ----------
        account_id : AccountId
            The account ID to delete events for.
        event_id : str
            The event ID to delete.

        """
        ...
    def add(self, key: str, value: bytes) -> None:
        """
        Add the given general object value to the database.

        Parameters
        ----------
        key : str
            The key to write to.
        value : bytes
            The object value.

        """
        ...
    def add_currency(self, currency: Currency) -> None:
        """
        Add the given currency to the database.

        Parameters
        ----------
        currency : Currency
            The currency to add.

        """
        ...
    def add_instrument(self, instrument: Instrument) -> None:
        """
        Add the given instrument to the database.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        ...
    def add_synthetic(self, synthetic: SyntheticInstrument) -> None:
        """
        Add the given synthetic instrument to the database.

        Parameters
        ----------
        synthetic : SyntheticInstrument
            The synthetic instrument to add.

        """
        ...
    def add_account(self, account: Account) -> None:
        """
        Add the given account to the database.

        Parameters
        ----------
        account : Account
            The account to add.

        """
        ...
    def add_order(self, order: Order, position_id: PositionId | None = None, client_id: ClientId | None = None) -> None:
        """
        Add the given order to the database.

        Parameters
        ----------
        order : Order
            The order to add.
        position_id : PositionId, optional
            The position ID to associate with this order.
        client_id : ClientId, optional
            The execution client ID to associate with this order.

        """
        ...
    def add_position(self, position: Position) -> None:
        """
        Add the given position to the database.

        Parameters
        ----------
        position : Position
            The position to add.

        """
        ...
    def index_venue_order_id(self, client_order_id: ClientOrderId, venue_order_id: VenueOrderId) -> None:
        """
        Add an index entry for the given `venue_order_id` to `client_order_id`.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to index.
        venue_order_id : VenueOrderId
            The venue order ID to index.

        """
        ...
    def index_order_position(self, client_order_id: ClientOrderId, position_id: PositionId) -> None:
        """
        Add an index entry for the given `client_order_id` to `position_id`.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to index.
        position_id : PositionId
            The position ID to index.

        """
        ...
    def update_actor(self, actor: Actor) -> None:
        """
        Update the given actor state in the database.

        Parameters
        ----------
        actor : Actor
            The actor to update.

        """
        ...
    def update_strategy(self, strategy: Strategy) -> None:
        """
        Update the given strategy state in the database.

        Parameters
        ----------
        strategy : Strategy
            The strategy to update.

        """
        ...
    def update_account(self, account: Account) -> None:
        """
        Update the given account in the database.

        Parameters
        ----------
        account : The account to update (from last event).

        """
        ...
    def update_order(self, order: Order) -> None:
        """
        Update the given order in the database.

        Parameters
        ----------
        order : Order
            The order to update (from last event).

        """
        ...
    def update_position(self, position: Position) -> None:
        """
        Update the given position in the database.

        Parameters
        ----------
        position : Position
            The position to update (from last event).

        """
        ...
    def snapshot_order_state(self, order: Order) -> None:
        """
        Snapshot the state of the given `order`.

        Parameters
        ----------
        order : Order
            The order for the state snapshot.

        """
        ...
    def snapshot_position_state(
        self,
        position: Position,
        ts_snapshot: int,
        unrealized_pnl: Money | None = None,
    ) -> None:
        """
        Snapshot the state of the given `position`.

        Parameters
        ----------
        position : Position
            The position for the state snapshot.
        ts_snapshot : uint64_t
            UNIX timestamp (nanoseconds) when the snapshot was taken.
        unrealized_pnl : Money, optional
            The unrealized PnL for the state snapshot.

        """
        ...
    def heartbeat(self, timestamp: datetime) -> None:
        """
        Add a heartbeat at the given `timestamp`.

        Parameters
        ----------
        timestamp : datetime
            The timestamp for the heartbeat.

        """
        ...
