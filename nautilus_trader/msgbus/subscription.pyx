# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.correctness cimport Condition


cdef class Subscription:
    """
    Represents a subscription to a particular topic.

    This is an internal class intended to be used by the message bus to organize
    topics and their subscribers.

    Parameters
    ----------
    topic : str
        The topic for the subscription. May include wildcard characters `*` and `?`.
    handler : Callable[[Message], None]
        The handler for the subscription.
    priority : int
        The priority for the subscription.

    Raises
    ------
    ValueError
        If `topic` is not a valid string.
    ValueError
        If `handler` is not of type `Callable`.
    ValueError
        If `priority` is negative (< 0).

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
        # Convert handler to string to avoid builtin_function_or_method hashing issues
        return hash((self.topic, str(self.handler)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"topic={self.topic}, "
            f"handler={self.handler}, "
            f"priority={self.priority})"
        )
