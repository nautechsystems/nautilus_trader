from datetime import datetime

from nautilus_trader.cache.config import CacheConfig
from stubs.accounting.accounts.base import Account
from stubs.common.actor import Actor
from stubs.model.identifiers import AccountId
from stubs.model.identifiers import ClientId
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import ComponentId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import PositionId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import VenueOrderId
from stubs.model.instruments.base import Instrument
from stubs.model.instruments.synthetic import SyntheticInstrument
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.orders.base import Order
from stubs.model.position import Position
from stubs.trading.strategy import Strategy

class CacheDatabaseFacade:
    """
    The base class for all cache databases.

    Parameters
    ----------
    config : CacheConfig, optional
        The configuration for the database.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, config: CacheConfig | None = None) -> None: ...
    def close(self) -> None:
        """Abstract method (implement in subclass)."""
    def flush(self) -> None:
        """Abstract method (implement in subclass)."""
    def keys(self, pattern: str = "*") -> list[str]:
        """Abstract method (implement in subclass)."""
    def load_all(self) -> dict:
        """Abstract method (implement in subclass)."""
    def load(self) -> dict:
        """Abstract method (implement in subclass)."""
    def load_currencies(self) -> dict:
        """Abstract method (implement in subclass)."""
    def load_instruments(self) -> dict:
        """Abstract method (implement in subclass)."""
    def load_synthetics(self) -> dict:
        """Abstract method (implement in subclass)."""
    def load_accounts(self) -> dict:
        """Abstract method (implement in subclass)."""
    def load_orders(self) -> dict:
        """Abstract method (implement in subclass)."""
    def load_positions(self) -> dict:
        """Abstract method (implement in subclass)."""
    def load_index_order_position(self) -> dict:
        """Abstract method (implement in subclass)."""
    def load_index_order_client(self) -> dict:
        """Abstract method (implement in subclass)."""
    def load_currency(self, code: str) -> Currency:
        """Abstract method (implement in subclass)."""
    def load_instrument(self, instrument_id: InstrumentId) -> Instrument:
        """Abstract method (implement in subclass)."""
    def load_synthetic(self, instrument_id: InstrumentId) -> SyntheticInstrument:
        """Abstract method (implement in subclass)."""
    def load_account(self, account_id: AccountId) -> Account:
        """Abstract method (implement in subclass)."""
    def load_order(self, client_order_id: ClientOrderId) -> Order:
        """Abstract method (implement in subclass)."""
    def load_position(self, position_id: PositionId) -> Position:
        """Abstract method (implement in subclass)."""
    def load_actor(self, component_id: ComponentId) -> dict:
        """Abstract method (implement in subclass)."""
    def load_strategy(self, strategy_id: StrategyId) -> dict:
        """Abstract method (implement in subclass)."""
    def add(self, key: str, value: bytes) -> None:
        """Abstract method (implement in subclass)."""
    def add_currency(self, currency: Currency) -> None:
        """Abstract method (implement in subclass)."""
    def add_instrument(self, instrument: Instrument) -> None:
        """Abstract method (implement in subclass)."""
    def add_synthetic(self, synthetic: SyntheticInstrument) -> None:
        """Abstract method (implement in subclass)."""
    def add_account(self, account: Account) -> None:
        """Abstract method (implement in subclass)."""
    def add_order(self, order: Order, position_id: PositionId | None = None, client_id: ClientId | None = None) -> None:
        """Abstract method (implement in subclass)."""
    def add_position(self, position: Position) -> None:
        """Abstract method (implement in subclass)."""
    def index_venue_order_id(self, client_order_id: ClientOrderId, venue_order_id: VenueOrderId) -> None:
        """Abstract method (implement in subclass)."""
    def index_order_position(self, client_order_id: ClientOrderId, position_id: PositionId) -> None:
        """Abstract method (implement in subclass)."""
    def update_account(self, event: Account) -> None:
        """Abstract method (implement in subclass)."""
    def update_order(self, order: Order) -> None:
        """Abstract method (implement in subclass)."""
    def update_position(self, position: Position) -> None:
        """Abstract method (implement in subclass)."""
    def update_actor(self, actor: Actor) -> None:
        """Abstract method (implement in subclass)."""
    def update_strategy(self, strategy: Strategy) -> None:
        """Abstract method (implement in subclass)."""
    def snapshot_order_state(self, order: Order) -> None:
        """Abstract method (implement in subclass)."""
    def snapshot_position_state(self, position: Position, ts_snapshot: int, unrealized_pnl: Money | None = None) -> None:
        """Abstract method (implement in subclass)."""
    def delete_order(self, client_order_id: ClientOrderId) -> None:
        """Abstract method (implement in subclass)."""
    def delete_position(self, position_id: PositionId) -> None:
        """Abstract method (implement in subclass)."""
    def delete_account_event(self, account_id: AccountId, event_id: str) -> None:
        """Abstract method (implement in subclass)."""
    def delete_actor(self, component_id: ComponentId) -> None:
        """Abstract method (implement in subclass)."""
    def delete_strategy(self, strategy_id: StrategyId) -> None:
        """Abstract method (implement in subclass)."""
    def heartbeat(self, timestamp: datetime) -> None:
        """Abstract method (implement in subclass)."""

