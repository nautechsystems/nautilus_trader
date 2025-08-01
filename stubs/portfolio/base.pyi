from typing import Any


class PortfolioFacade:
    """
    Provides a read-only facade for a `Portfolio`.
    """

    def account(self, venue: Venue) -> Account:
        """Abstract method (implement in subclass)."""
    def balances_locked(self, venue: Venue) -> dict[Any, Any]:
        """Abstract method (implement in subclass)."""
    def margins_init(self, venue: Venue) -> dict[Any, Any]:
        """Abstract method (implement in subclass)."""
    def margins_maint(self, venue: Venue) -> dict[Any, Any]:
        """Abstract method (implement in subclass)."""
    def realized_pnls(self, venue: Venue) -> dict[Any, Any]:
        """Abstract method (implement in subclass)."""
    def unrealized_pnls(self, venue: Venue) -> dict[Any, Any]:
        """Abstract method (implement in subclass)."""
    def total_pnls(self, venue: Venue) -> dict[Any, Any]:
        """Abstract method (implement in subclass)."""
    def net_exposures(self, venue: Venue) -> dict[Any, Any]:
        """Abstract method (implement in subclass)."""
    def realized_pnl(self, instrument_id: InstrumentId) -> Money:
        """Abstract method (implement in subclass)."""
    def unrealized_pnl(self, instrument_id: InstrumentId, price: Price | None = None) -> Money:
        """Abstract method (implement in subclass)."""
    def total_pnl(self, instrument_id: InstrumentId, price: Price | None = None) -> Money:
        """Abstract method (implement in subclass)."""
    def net_exposure(self, instrument_id: InstrumentId, price: Price | None = None) -> Money:
        """Abstract method (implement in subclass)."""
    def net_position(self, instrument_id: InstrumentId) -> object:
        """Abstract method (implement in subclass)."""
    def is_net_long(self, instrument_id: InstrumentId) -> bool:
        """Abstract method (implement in subclass)."""
    def is_net_short(self, instrument_id: InstrumentId) -> bool:
        """Abstract method (implement in subclass)."""
    def is_flat(self, instrument_id: InstrumentId) -> bool:
        """Abstract method (implement in subclass)."""
    def is_completely_flat(self) -> bool:
        """Abstract method (implement in subclass)."""
