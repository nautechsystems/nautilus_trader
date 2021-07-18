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

from typing import Any, Callable

import cython
import numpy as np

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition


cdef str WILDCARD = "*"


cdef class Subscription:
    """
    Represents a subscription to a particular topic.

    This is an internal class intended to be used by the message bus to
    organize channels and their subscribers.

    Notes
    -----
    The subscription equality is determined by the topic and handler,
    priority is not considered (and could change).

    """

    def __init__(
        self,
        str topic,
        handler not None: Callable[[Any], None],
        int priority=0,
    ):
        """
        Initialize a new instance of the ``Subscription`` class.

        Parameters
        ----------
        topic : str
            The topic for the subscription. May include wildcard glob patterns.
        handler : Callable[[Message], None]
            The handler for the subscription.
        priority : int
            The priority for the subscription.

        Raises
        ------
        ValueError
            If topic is not a valid string.
        ValueError
            If priority is negative (< 0).

        """
        Condition.valid_string(topic, "topic")
        Condition.not_negative_int(priority, "priority")

        self._topic_str = topic
        self.topic = topic.replace(WILDCARD, "")
        self.handler = handler
        self.priority = priority

    def __eq__(self, Subscription other) -> bool:
        return self.topic == other.topic and self.handler == other.handler

    def __lt__(self, Subscription other) -> bool:
        return self.priority < other.priority

    def __le__(self, Subscription other) -> bool:
        return self.priority <= other.priority

    def __gt__(self, Subscription other) -> bool:
        return self.priority > other.priority

    def __ge__(self, Subscription other) -> bool:
        return self.priority >= other.priority

    def __hash__(self) -> int:
        return hash((self.topic, self.handler))

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"topic={self._topic_str}, "
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
        Clock clock not None,
        Logger logger not None,
        str name=None,
    ):
        """
        Initialize a new instance of the ``MessageBus`` class.

        Parameters
        ----------
        clock : Clock
            The clock for the message bus.
        logger : Logger
            The logger for the message bus.
        name : str, optional
            The custom name for the message bus.

        Raises
        ------
        ValueError
            If name is not None and not a valid string.

        """
        if name is None:
            name = "MessageBus"
        Condition.valid_string(name, "name")

        self._clock = clock
        self._log = LoggerAdapter(component=name, logger=logger)

        self._channels = {}    # type: dict[str, Subscription[:]]
        self._patterns = None  # type: Subscription[:]
        self._patterns_len = 0

        # Counters
        self.processed_count = 0

    cpdef list channels(self):
        """
        Return all topic channels with active subscribers (including '*').

        Returns
        -------
        list[str]

        """
        cdef list channels = []
        channels.extend(list(self._channels.keys()))
        if self._patterns is not None:
            channels.extend([s.topic + WILDCARD for s in list(self._patterns)])
        return channels

    cpdef list subscriptions(self, str topic):
        """
        Return all subscriptions for the given message type.

        Parameters
        ----------
        topic : str
            The topic filter.
            If None then will return subscriptions for ALL messages.

        Returns
        -------
        list[Subscription]

        """
        Condition.valid_string(topic, "topic")

        topic = topic.replace(WILDCARD, "")

        cdef list output = []
        for sub in self._channels.get(topic, []):
            output.append(sub)

        if self._patterns is not None:
            for sub in list(self._patterns):
                if sub.topic.startswith(topic):
                    output.append(sub)

        return output

    cpdef void subscribe(
        self,
        str topic,
        handler: Callable[[Any], None],
        int priority=0,
    ) except *:
        """
        Subscribe to the given message type.

        Parameters
        ----------
        topic : MessageType
            The message type to subscribe to. If "*" then subscribes to ALL messages.
        handler : Callable[[Any], None]
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
        Condition.valid_string(topic, "topic")
        Condition.not_none(handler, "handler")

        # Create subscription
        cdef Subscription sub = Subscription(
            topic=topic,
            handler=handler,
            priority=priority,
        )

        # Get current subscriptions for topic
        if WILDCARD in topic:
            self._subscribe_pattern(sub)
        else:
            self._subscribe_channel(sub)

    cdef void _subscribe_pattern(self, Subscription sub) except *:
        if self._patterns is None:
            subscriptions = []
        else:
            subscriptions = list(self._patterns)

        # Check if already exists
        if sub in subscriptions:
            self._log.warning(f"{sub} already exists.")
            return

        subscriptions.append(sub)
        subscriptions = sorted(subscriptions, reverse=True)
        self._patterns = np.ascontiguousarray(subscriptions)
        self._patterns_len = len(subscriptions)
        self._log.debug(f"Added {sub}.")

    cdef void _subscribe_channel(self, Subscription sub) except *:
        cdef list subscriptions = list(self._channels.get(sub.topic, []))
        # Check if already exists
        if sub in subscriptions:
            self._log.warning(f"{sub} already exists.")
            return

        subscriptions.append(sub)
        subscriptions = sorted(subscriptions, reverse=True)
        self._channels[sub.topic] = np.ascontiguousarray(subscriptions)
        self._log.info(f"Added {sub}.")

    cpdef void unsubscribe(self, str topic, handler: Callable[[Any], None]) except *:
        """
        Unsubscribe the handler from the given message type.

        Parameters
        ----------
        topic : str, optional
            The topic to unsubscribe from. If "*" then unsubscribes from ALL messages.
        handler : Callable[[Any], None]
            The handler for the subscription.

        """
        Condition.valid_string(topic, "topic")
        Condition.not_none(handler, "handler")

        cdef Subscription sub = Subscription(topic=topic, handler=handler)

        # Get current subscriptions for topic
        if WILDCARD in topic:
            self._unsubscribe_pattern(sub)
        else:
            self._unsubscribe_channel(sub)

    cdef void _unsubscribe_pattern(self, Subscription sub) except *:
        if self._patterns is None:
            subscriptions = []
        else:
            subscriptions = list(self._patterns)

        # Check if exists
        if sub not in subscriptions:
            self._log.warning(f"{sub} not found.")
            return

        subscriptions.remove(sub)
        subscriptions = sorted(subscriptions, reverse=True)
        if not subscriptions:
            self._patterns = None
            self._patterns_len = 0
        else:
            self._patterns = np.ascontiguousarray(subscriptions)
            self._patterns_len = len(subscriptions)
        self._log.debug(f"Removed {sub}.")

    cdef void _unsubscribe_channel(self, Subscription sub) except *:
        cdef list subscriptions = list(self._channels.get(sub.topic, []))

        # Check if already exists
        if sub not in subscriptions:
            self._log.warning(f"{sub} not found.")
            return

        subscriptions.remove(sub)
        self._log.debug(f"Removed {sub}.")

        if not subscriptions:
            del self._channels[sub.topic]
            return

        subscriptions = sorted(subscriptions, reverse=True)
        self._channels[sub.topic] = np.ascontiguousarray(subscriptions)

    cpdef void publish(self, str topic, msg: Any) except *:
        """
        Publish the given message.

        Subscription handlers will receive the message in priority order
        (highest first).

        Parameters
        ----------
        topic : str
            The topic to publish on (determines the channel and matching patterns).
        msg : object
            The message to publish.

        """
        self.publish_c(topic, msg)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef void publish_c(self, str topic, msg: Any) except *:
        Condition.not_none(topic, "topic")
        Condition.not_none(msg, "msg")

        cdef Subscription sub
        cdef Subscription[:] subscriptions = self._channels.get(topic)
        if subscriptions is not None:
            # Send to channel subscriptions
            for i in range(len(subscriptions)):
                sub = subscriptions[i]
                sub.handler(msg)

        # Check all pattern subscriptions
        for i in range(self._patterns_len):
            sub = self._patterns[i]
            if topic.__contains__(sub.topic):
                sub.handler(msg)

        self.processed_count += 1
