from typing import Any

from nautilus_trader.common.enums import ComponentState
from nautilus_trader.model.enums import TradingState
from stubs.core.message import Command
from stubs.core.message import Event
from stubs.core.uuid import UUID4
from stubs.model.identifiers import Identifier
from stubs.model.identifiers import TraderId

class ShutdownSystem(Command):
    """
    Represents a command to shut down a system and terminate the process.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID associated with the event.
    component_id : Identifier
        The component ID associated with the event.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reason : str, optional
        The reason for the shutdown command (can be None).
    """

    def __init__(
        self,
        trader_id: TraderId,
        component_id: Identifier,
        command_id: UUID4,
        ts_init: int,
        reason: str | None = None,
    ) -> None: ...
    def __eq__(self, other: Command) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> ShutdownSystem:
        """
        Return a shutdown system command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        ShutdownSystem

        """
        ...
    @staticmethod
    def to_dict(obj: ShutdownSystem) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class ComponentStateChanged(Event):
    """
    Represents an event which includes information on the state of a component.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID associated with the event.
    component_id : Identifier
        The component ID associated with the event.
    component_type : str
        The component type.
    state : ComponentState
        The component state.
    config : dict[str, Any]
        The component configuration for the event.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the component state event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        trader_id: TraderId,
        component_id: Identifier,
        component_type: str,
        state: ComponentState,
        config: dict[str, Any],
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def id(self) -> UUID4:
        """
        The event message identifier.

        Returns
        -------
        UUID4

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the event occurred.

        Returns
        -------
        int

        """
        ...
    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> ComponentStateChanged: 
        """
        Return a component state changed event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        ComponentStateChanged

        """
        ...
    @staticmethod
    def to_dict(obj: ComponentStateChanged) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...


class RiskEvent(Event):
    """
    The base class for all risk events.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID associated with the event.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the component state event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        trader_id: TraderId,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    @property
    def id(self) -> UUID4:
        """
        The event message identifier.

        Returns
        -------
        UUID4

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the event occurred.

        Returns
        -------
        int

        """
        ...
    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        ...


class TradingStateChanged(RiskEvent):
    """
    Represents an event where trading state has changed at the `RiskEngine`.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID associated with the event.
    state : TradingState
        The trading state for the event.
    config : dict[str, Any]
        The configuration of the risk engine.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the component state event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        trader_id: TraderId,
        state: TradingState,
        config: dict[str, Any],
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def id(self) -> UUID4:
        """
        The event message identifier.

        Returns
        -------
        UUID4

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the event occurred.

        Returns
        -------
        int

        """
        ...
    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> TradingStateChanged:
        """
        Return a trading state changed event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        TradingStateChanged

        """
        ...
    @staticmethod
    def to_dict(obj: TradingStateChanged) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
