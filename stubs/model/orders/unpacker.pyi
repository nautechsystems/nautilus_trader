from typing import Any

from stubs.model.events.order import OrderInitialized
from stubs.model.orders.base import Order

class OrderUnpacker:
    """
    Provides a means of unpacking orders from value dictionaries.
    """

    @staticmethod
    def unpack(values: dict[str, Any]) -> Order:
        """
        Return an order unpacked from the given values.

        Parameters
        ----------
        values : dict[str, object]

        Returns
        -------
        Order

        """
        ...
    @staticmethod
    def from_init(init: OrderInitialized) -> Order:
        """
        Return an order initialized from the given event.

        Parameters
        ----------
        init : OrderInitialized
            The event to initialize with.

        Returns
        -------
        Order

        """
        ...
