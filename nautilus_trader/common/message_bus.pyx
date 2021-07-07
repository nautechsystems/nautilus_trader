# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from typing import Callable

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.type cimport MessageType


cdef class Subscription:
    """
    Represents a subscription to a particular message type.

    This is an internal class intended to be used by the message bus to
    organize channels and their subscribers.

    Notes
    -----
    The subscription equality is determined by the msg_type and handler,
    priority is not considered (and could change).

    """

    def __init__(
        self,
        MessageType msg_type not None,
        handler not None: Callable,
        int priority=0,
    ):
        """
        Initialize a new instance of the ``Subscription`` class.

        Parameters
        ----------
        msg_type : MessageType
            The message type for the subscription.
        handler : Callable
            The handler for the subscription.
        priority : int
            The priority for the subscription.

        """
        self.msg_type = msg_type
        self.handler = handler
        self.priority = priority

    def __eq__(self, Subscription other) -> bool:
        return self.msg_type == other.msg_type and self.handler == other.handler

    def __lt__(self, Subscription other) -> bool:
        return self.priority < other.priority

    def __le__(self, Subscription other) -> bool:
        return self.priority <= other.priority

    def __gt__(self, Subscription other) -> bool:
        return self.priority > other.priority

    def __ge__(self, Subscription other) -> bool:
        return self.priority >= other.priority

    def __hash__(self) -> int:
        return hash((self.msg_type, self.handler))

    def __str__(self) -> str:
        return (f"{type(self).__name__}("
                f"msg_type={self.msg_type}, "
                f"handler={self.handler})")

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"msg_type={self.msg_type}, "
                f"handler={self.handler}, "
                f"priority={self.priority})")


cdef class MessageBus:
    """
    Provides a generic message bus to facilitate consumers subscribing to
    publishing producers.

    The bus provides both a producer and consumer API.
    """

    def __init__(
        self,
        str name not None,
        Clock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the ``MessageBus`` class.

        Parameters
        ----------
        name : str
            The component name for the message bus.
        clock : Clock
            The clock for the message bus.
        logger : Logger
            The logger for the message bus.

        """
        Condition.not_none(name, "name")

        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(component=name, logger=logger)

        self._channels = {}    # type: dict[type, list[Subscription]]

        # Counters
        self.processed_count = 0

    cpdef list channels(self):
        """
        Return all channels with active subscribers.

        Returns
        -------
        list[type]

        """
        return list(self._channels.keys())

    cpdef list subscriptions(self, MessageType msg_type):
        """
        Return all subscriptions for the given message type.

        Parameters
        ----------
        msg_type : MessageType
            The message type filter.

        Returns
        -------
        list[Subscription]

        """
        Condition.not_none(msg_type, "msg_type")

        cdef list subscriptions = self._channels.get(msg_type.type)
        if subscriptions is None:
            return []

        cdef list output = []
        for sub in subscriptions:
            if msg_type.header is None or msg_type.header == sub.msg_type.header:
                output.append(sub)

        return output

    cpdef void subscribe(self, MessageType msg_type, handler: Callable, int priority=0) except *:
        """
        Subscribe to the given message type.

        Parameters
        ----------
        msg_type : MessageType
            The message type to subscribe to.
        handler : Callable
            The handler for the subscription.
        priority : int
            The priority for the subscription. Determines the ordering of
            handlers receiving messages being processed, higher priority
            handlers will receive messages prior to lower priority handlers.

        Warnings
        --------
        Assigning priority handling is an advanced feature which shouldn't
        normally be needed by most users. Only assign a higher priority to the
        subscription if you are certain of what you're doing. If an inappropriate
        priority is assigned then the handler may receive messages before core
        system components have been able to conduct the necessary calculations
        for logically sound behaviour.

        """
        Condition.not_none(msg_type, "msg_type")
        Condition.not_none(handler, "handler")

        # Create subscription
        cdef Subscription sub = Subscription(
            msg_type=msg_type,
            handler=handler,
            priority=priority,
        )

        # Get channel subscriptions
        cdef list subscriptions = self._channels.get(msg_type.type, [])
        if sub in subscriptions:
            self._log.warning(f"{sub} already exists.")
            return

        # Add to subscriptions
        subscriptions.append(sub)

        # Add to channel in priority order
        self._channels[msg_type.type] = sorted(subscriptions, reverse=True)
        self._log.info(f"Added {repr(sub)}.")

    cpdef void unsubscribe(self, MessageType msg_type, handler: Callable) except *:
        """
        Unsubscribe the handler from the given message type.

        Parameters
        ----------
        msg_type : MessageType
            The message type to unsubscribe from.
        handler : Callable
            The handler for the subscription.

        """
        Condition.not_none(msg_type, "msg_type")
        Condition.not_none(handler, "handler")

        cdef Subscription sub = Subscription(msg_type=msg_type, handler=handler)

        # Get channel subscriptions
        cdef list subscriptions = self._channels.get(msg_type.type, [])
        if sub not in subscriptions:
            self._log.warning(f"{sub} not found.")
            return

        subscriptions.remove(sub)
        self._log.debug(f"Removed {sub}.")

        # Remove channel if no more handlers
        if not self._channels[msg_type.type]:
            del self._channels[msg_type.type]

    cpdef void publish(self, MessageType msg_type, message) except *:
        """
        Publish the given message.

        Parameters
        ----------
        msg_type : MessageType
            The message type to process. Determines the channel.
        message : object
            The message to process.

        """
        Condition.not_none(msg_type, "msg_type")
        Condition.not_none(message, "message")

        cdef list subscriptions = self._channels.get(msg_type.type)
        if not subscriptions:
            return

        cdef Subscription sub
        for sub in subscriptions:
            if sub.msg_type.key.issubset(msg_type.key):
                sub.handler(message)

        self.processed_count += 1
