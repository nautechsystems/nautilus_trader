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
from nautilus_trader.model.identifiers cimport TraderId


cdef str WILDCARD = "*"


cdef class Subscription:
    """
    Represents a subscription to a particular topic.

    This is an internal class intended to be used by the message bus to organize
    topics and their subscribers.

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
            The topic for the subscription. May include wildcard characters.
        handler : Callable[[Message], None]
            The handler for the subscription.
        priority : int
            The priority for the subscription.

        Raises
        ------
        ValueError
            If topic is not a valid string.
        ValueError
            If handler is not of type callable.
        ValueError
            If priority is negative (< 0).

        """
        Condition.valid_string(topic, "topic")
        Condition.callable(handler, "handler")
        Condition.not_negative_int(priority, "priority")

        self.topic = topic
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
                f"topic={self.topic}, "
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
        TraderId trader_id not None,
        Clock clock not None,
        Logger logger not None,
        str name=None,
    ):
        """
        Initialize a new instance of the ``MessageBus`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID associated with the message bus.
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

        self.trader_id = trader_id

        self._clock = clock
        self._log = LoggerAdapter(component=name, logger=logger)

        self._endpoints = {}      # type: dict[str, Callable[[Any], None]]
        self._patterns = {}       # type: dict[str, Subscription[:]]
        self._subscriptions = {}  # type: dict[Subscription, list[str]]

        # Counters
        self.processed_count = 0

    cpdef list endpoints(self):
        """
        Return all endpoint addresses registered with the message bus.

        Returns
        -------
        list[str]

        """
        return list(self._endpoints.keys())

    cpdef list topics(self):
        """
        Return all topics with active subscribers.

        Returns
        -------
        list[str]

        """
        return sorted(set([s.topic for s in self._subscriptions.keys()]))

    cpdef list subscriptions(self, str topic=None):
        """
        Return all subscriptions matching the given topic.

        Parameters
        ----------
        topic : str, optional
            The topic filter. May include wildcard characters.
            If None then filter is for ALL topics.

        Returns
        -------
        list[Subscription]

        """
        if topic is None:
            topic = WILDCARD
        Condition.valid_string(topic, "topic")

        return [s for s in self._subscriptions if is_matching(s.topic, topic)]

    cpdef bint has_subscribers(self, str topic=None):
        """
        If the message bus has subscribers for the give topic.

        Parameters
        ----------
        topic : str, optional
            The topic filter. May include wildcard characters.
            If None then query is for ALL topics.

        Returns
        -------
        bool

        """
        return len(self.subscriptions(topic)) > 0

    cpdef void register(self, str endpoint, handler: Callable[[Any], None]) except *:
        """
        Register the given handler to receive messages at the endpoint address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to register.
        handler : Callable[[Any], None]
            The handler for the registration.

        Raises
        ------
        ValueError
            If endpoint is not a valid string.
        ValueError
            If handler is not of type callable.
        KeyError
            If endpoint already registered.

        """
        Condition.valid_string(endpoint, "endpoint")
        Condition.callable(handler, "handler")
        Condition.not_in(endpoint, self._endpoints, "endpoint", "self._endpoints")

        self._endpoints[endpoint] = handler

    cpdef void deregister(self, str endpoint, handler: Callable[[Any], None]) except *:
        """
        De-register the given handler from the endpoint address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to deregister.
        handler : Callable[[Any], None]
            The handler for the de-registration.

        Raises
        ------
        ValueError
            If endpoint is not a valid string.
        ValueError
            If handler is not of type callable.
        KeyError
            If endpoint is not registered.
        ValueError
            If handler is not registered at the endpoint.

        """
        Condition.valid_string(endpoint, "endpoint")
        Condition.callable(handler, "handler")
        Condition.is_in(endpoint, self._endpoints, "endpoint", "self._endpoints")
        Condition.equal(handler, self._endpoints[endpoint], "handler", "self._endpoints[endpoint]")

        del self._endpoints[endpoint]

    cpdef void send(self, str endpoint, msg: Any) except *:
        """
        Send the given message to the given endpoint address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to send to.
        msg : object
            The message to send.

        """
        Condition.not_none(endpoint, "endpoint")
        Condition.not_none(msg, "msg")

        handler = self._endpoints.get(endpoint)
        if handler is None:
            self._log.error(f"Cannot send message: no endpoint registered at '{endpoint}'.")
            return

        handler(msg)

    cpdef void subscribe(
        self,
        str topic,
        handler: Callable[[Any], None],
        int priority=0,
    ) except *:
        """
        Subscribe to the given message topic with the given callback handler.

        Parameters
        ----------
        topic : str
            The topic for the subscription. May include wildcard characters.
        handler : Callable[[Any], None]
            The handler for the subscription.
        priority : int, optional
            The priority for the subscription. Determines the ordering of
            handlers receiving messages being processed, higher priority
            handlers will receive messages prior to lower priority handlers.

        Raises
        ------
        ValueError
            If topic is not a valid string.
        ValueError
            If handler is not of type callable.

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
        Condition.callable(handler, "handler")

        # Create subscription
        cdef Subscription sub = Subscription(
            topic=topic,
            handler=handler,
            priority=priority,
        )

        # Check if already exists
        if sub in self._subscriptions:
            self._log.warning(f"{sub} already exists.")
            return

        cdef list matches = []
        cdef list patterns = list(self._patterns.keys())

        cdef str pattern
        cdef list subs
        for pattern in patterns:
            if is_matching(topic, pattern):
                subs = list(self._patterns[pattern])
                subs.append(sub)
                self._patterns[pattern] = np.ascontiguousarray(sorted(subs, reverse=True))
                matches.append(pattern)

        self._subscriptions[sub] = sorted(matches)

        self._log.debug(f"Added {sub}.")

    cpdef void unsubscribe(self, str topic, handler: Callable[[Any], None]) except *:
        """
        Unsubscribe the given callback handler from the given message topic.

        Parameters
        ----------
        topic : str, optional
            The topic to unsubscribe from. May include wildcard characters.
        handler : Callable[[Any], None]
            The handler for the subscription.

        Raises
        ------
        ValueError
            If topic is not a valid string.
        ValueError
            If handler is not of type callable.

        """
        Condition.valid_string(topic, "topic")
        Condition.callable(handler, "handler")

        cdef Subscription sub = Subscription(topic=topic, handler=handler)

        cdef list patterns = self._subscriptions.get(sub)

        # Check if exists
        if patterns is None:
            self._log.warning(f"{sub} not found.")
            return

        cdef str pattern
        for pattern in patterns:
            subs = list(self._patterns[pattern])
            subs.remove(sub)
            self._patterns[pattern] = np.ascontiguousarray(sorted(subs, reverse=True))

        del self._subscriptions[sub]

        self._log.debug(f"Removed {sub}.")

    cpdef void publish(self, str topic, msg: Any) except *:
        """
        Publish the given message.

        Subscription handlers will receive the message in priority order
        (highest first).

        Parameters
        ----------
        topic : str
            The topic to publish on. May include wildcard characters.
        msg : object
            The message to publish.

        """
        self.publish_c(topic, msg)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef void publish_c(self, str topic, msg: Any) except *:
        Condition.not_none(topic, "topic")
        Condition.not_none(msg, "msg")

        cdef Subscription[:] subs = self._patterns.get(topic)
        if subs is None:
            subs = self._resolve_subscriptions(topic)

        # Send to all matched subscribers
        cdef int i
        for i in range(len(subs)):
            subs[i].handler(msg)

        self.processed_count += 1

    cdef Subscription[:] _resolve_subscriptions(self, str topic):
        cdef list subs_list = []
        cdef Subscription existing_sub
        for existing_sub in self._subscriptions:
            if is_matching(topic, existing_sub.topic):
                subs_list.append(existing_sub)

        subs_list = sorted(subs_list, reverse=True)
        cdef Subscription[:] subs_array = np.ascontiguousarray(subs_list, dtype=Subscription)
        self._patterns[topic] = subs_array

        cdef list matches
        for sub in subs_array:
            matches = self._subscriptions.get(sub, [])
            if topic not in matches:
                matches.append(topic)
            self._subscriptions[sub] = sorted(matches)

        return subs_array


cdef inline bint is_matching(str topic, str pattern) except *:
    """
    Return a value indicating whether the topic matches with the pattern.

    Given a topic and pattern potentially containing wildcard characters, i.e.
    '*' and '?', where '?' can match any single character in the topic, and '*'
    can match any number of characters including zero characters.

    Parameters
    ----------
    topic : str
        The topic string.
    pattern : str
        The pattern to match on.

    Returns
    -------
    bool

    """
    # Get length of string and wildcard pattern
    cdef int n = len(topic)
    cdef int m = len(pattern)

    # Create a DP lookup table
    cdef list t = [[False for x in range(m + 1)] for y in range(n + 1)]

    # If both pattern and string are empty: match
    t[0][0] = True

    # Handle empty string case (i == 0)
    cdef int j
    for j in range(1, m + 1):
        if pattern[j - 1] == '*':
            t[0][j] = t[0][j - 1]

    # Build a matrix in a bottom-up manner
    cdef int i
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            if pattern[j - 1] == '*':
                t[i][j] = t[i - 1][j] or t[i][j - 1]
            elif pattern[j - 1] == '?' or topic[i - 1] == pattern[j - 1]:
                t[i][j] = t[i - 1][j - 1]

    # Last cell stores the answer
    return t[n][m]
