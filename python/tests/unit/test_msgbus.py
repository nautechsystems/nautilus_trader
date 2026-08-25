# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Test msgbus behavior.
"""

import pytest

from nautilus_trader.common import MessageBus
from nautilus_trader.core import UUID4
from nautilus_trader.model import TraderId


@pytest.fixture
def trader_id() -> object:
    """
    Trader id.
    """
    return TraderId.from_str("TRADER-001")


@pytest.fixture
def bus(trader_id: TraderId) -> object:
    """
    Bus.
    """
    return MessageBus(trader_id=trader_id)


def test_instantiate_defaults(bus: object, trader_id: TraderId) -> None:
    """
    Test instantiate defaults.
    """
    assert bus.trader_id == trader_id
    assert bus.name == "MessageBus"
    assert bus.has_backing is False
    assert bus.sent_count == 0
    assert bus.req_count == 0
    assert bus.res_count == 0
    assert bus.pub_count == 0


def test_instantiate_with_custom_name(trader_id: TraderId) -> None:
    """
    Test instantiate with custom name.
    """
    bus = MessageBus(trader_id=trader_id, name="CustomBus")
    assert bus.name == "CustomBus"


def test_endpoints_empty(bus: object) -> None:
    """
    Test endpoints empty.
    """
    assert bus.endpoints() == []


def test_register_adds_endpoint(bus: object) -> None:
    """
    Test register adds endpoint.
    """
    bus.register("mailbox", [].append)
    assert bus.endpoints() == ["mailbox"]


def test_deregister_removes_endpoint(bus: object) -> None:
    """
    Test deregister removes endpoint.
    """
    handler = [].append
    bus.register("mailbox", handler)
    bus.deregister("mailbox", handler)
    assert bus.endpoints() == []


def test_send_delivers_to_endpoint(bus: object) -> None:
    """
    Test send delivers to endpoint.
    """
    received = []
    bus.register("mailbox", received.append)
    bus.send("mailbox", "msg")
    assert received == ["msg"]
    assert bus.sent_count == 1


def test_send_no_endpoint_increments_count(bus: object) -> None:
    """
    Test send no endpoint increments count.
    """
    bus.send("nowhere", "msg")
    assert bus.sent_count == 1


def test_send_multiple_increments_count(bus: object) -> None:
    """
    Test send multiple increments count.
    """
    received = []
    bus.register("ep", received.append)
    bus.send("ep", "a")
    bus.send("ep", "b")
    assert received == ["a", "b"]
    assert bus.sent_count == 2


def test_deregister_and_send_return_none(bus: object) -> None:
    """
    Test deregister and send return none.
    """
    handler = [].append
    bus.register("mailbox", handler)

    assert bus.send("mailbox", "msg") is None
    assert bus.deregister("mailbox", handler) is None


def test_topics_empty(bus: object) -> None:
    """
    Test topics empty.
    """
    assert bus.topics() == []


def test_subscriptions_empty(bus: object) -> None:
    """
    Test subscriptions empty.
    """
    assert bus.subscriptions() == []


def test_has_subscribers_false_when_empty(bus: object) -> None:
    """
    Test has subscribers false when empty.
    """
    assert not bus.has_subscribers()


def test_subscriptions_with_pattern_filter(bus: object) -> None:
    """
    Test subscriptions with pattern filter.
    """
    bus.subscribe("data.quotes.*", [].append)
    bus.subscribe("data.trades.*", [].append)
    bus.subscribe("events.order.*", [].append)

    data_subs = bus.subscriptions("data.*")
    all_subs = bus.subscriptions()

    assert len(data_subs) == 2
    assert len(all_subs) == 3


def test_has_subscribers_with_pattern(bus: object) -> None:
    """
    Test has subscribers with pattern.
    """
    bus.subscribe("data.quotes.BINANCE", [].append)
    assert bus.has_subscribers("data.quotes.BINANCE")
    assert not bus.has_subscribers("events.order.S1")


def test_subscribe_adds_topic(bus: object) -> None:
    """
    Test subscribe adds topic.
    """
    bus.subscribe("system", [].append)
    assert "system" in bus.topics()


def test_subscribe_shows_has_subscribers(bus: object) -> None:
    """
    Test subscribe shows has subscribers.
    """
    bus.subscribe("events.*", [].append)
    assert bus.has_subscribers()


def test_subscribe_duplicate_ignored(bus: object) -> None:
    """
    Test subscribe duplicate ignored.
    """
    handler = [].append
    bus.subscribe("a", handler)
    bus.subscribe("a", handler)
    assert len(bus.subscriptions()) == 1


def test_unsubscribe_removes_subscription(bus: object) -> None:
    """
    Test unsubscribe removes subscription.
    """
    handler = [].append
    bus.subscribe("events.order*", handler)
    bus.unsubscribe("events.order*", handler)
    assert bus.subscriptions() == []


def test_unsubscribe_nonexistent_does_nothing(bus: object) -> None:
    """
    Test unsubscribe nonexistent does nothing.
    """
    bus.unsubscribe("missing", [].append)
    assert bus.subscriptions() == []


def test_is_subscribed_lifecycle(bus: object) -> None:
    """
    Test is subscribed lifecycle.
    """

    def handler(msg: object) -> object:
        """
        Handle the callback.
        """
        return msg

    assert not bus.is_subscribed("topic.test", handler)
    bus.subscribe("topic.test", handler)
    assert bus.is_subscribed("topic.test", handler)
    bus.unsubscribe("topic.test", handler)
    assert not bus.is_subscribed("topic.test", handler)


def test_publish_with_no_subscribers(bus: object) -> None:
    """
    Test publish with no subscribers.
    """
    bus.publish("empty.topic", "hello")
    assert bus.pub_count == 1


def test_publish_delivers_to_subscriber(bus: object) -> None:
    """
    Test publish delivers to subscriber.
    """
    received = []
    bus.subscribe("system", received.append)
    bus.publish("system", "hello")
    assert received == ["hello"]
    assert bus.pub_count == 1


def test_publish_delivers_to_multiple_subscribers(bus: object) -> None:
    """
    Test publish delivers to multiple subscribers.
    """
    sub1, sub2, sub3 = [], [], []
    bus.subscribe("system", sub1.append)
    bus.subscribe("system", sub2.append)
    bus.subscribe("system", sub3.append)
    bus.publish("system", "hello")
    assert sub1 == ["hello"]
    assert sub2 == ["hello"]
    assert sub3 == ["hello"]
    assert bus.pub_count == 1


def test_publish_wildcard_star(bus: object) -> None:
    """
    Test publish wildcard star.
    """
    received = []
    bus.subscribe("events.order*", received.append)
    bus.publish("events.order.SCALPER-001", "ORDER")
    assert received == ["ORDER"]


def test_publish_no_match_filters(bus: object) -> None:
    """
    Test publish no match filters.
    """
    received = []
    bus.subscribe("events.position*", received.append)
    bus.publish("events.order.S-001", "ORDER")
    assert received == []


def test_publish_star_catches_all(bus: object) -> None:
    """
    Test publish star catches all.
    """
    all_msgs = []
    specific = []
    bus.subscribe("*", all_msgs.append)
    bus.subscribe("MyTopic", specific.append)
    bus.publish("MyTopic", "OK!")
    assert specific == ["OK!"]
    assert all_msgs == ["OK!"]


def test_publish_question_mark_pattern(bus: object) -> None:
    """
    Test publish question mark pattern.
    """
    received = []
    bus.subscribe("test.?", received.append)
    bus.publish("test.a", "ok1")
    bus.publish("test.1", "ok2")
    bus.publish("test.12", "nope")
    assert received == ["ok1", "ok2"]


def test_publish_combined_wildcards(bus: object) -> None:
    """
    Test publish combined wildcards.
    """
    received = []
    bus.subscribe("data.*.BINANCE.ETH*", received.append)
    bus.publish("data.trades.BINANCE.ETHUSDT", "t1")
    bus.publish("data.quotes.BINANCE.ETHUSDT", "q1")
    bus.publish("data.trades.BINANCE.BTCUSDT", "nope")
    assert received == ["t1", "q1"]


def test_publish_late_subscribe(bus: object) -> None:
    """
    Test publish late subscribe.
    """
    received = []
    bus.publish("events.order.S-001", "early")
    bus.subscribe("events.order*", received.append)
    bus.publish("events.order.S-001", "late")
    assert received == ["late"]
    assert bus.pub_count == 2


def test_publish_priority_order(bus: object) -> None:
    """
    Test publish priority order.
    """
    order = []

    def low(msg: object) -> None:
        """
        Low.
        """
        order.append(f"low-{msg}")

    def high(msg: object) -> None:
        """
        High.
        """
        order.append(f"high-{msg}")

    bus.subscribe("orders", low, priority=0)
    bus.subscribe("orders", high, priority=10)
    bus.publish("orders", "123")
    assert order == ["high-123", "low-123"]


def test_publish_python_objects(bus: object) -> None:
    """
    Test publish python objects.
    """
    received = []
    bus.subscribe("data", received.append)
    obj = {"key": [1, 2, 3], "nested": {"a": True}}
    bus.publish("data", obj)
    assert received == [obj]
    assert received[0] is obj


def test_request_response_round_trip(bus: object) -> None:
    """
    Test request response round trip.
    """
    endpoint_msgs = []
    callback_msgs = []

    bus.register("service", endpoint_msgs.append)

    class FakeRequest:
        """
        Collect fake request tests.
        """

        def __init__(self, req_id: object, callback: object) -> None:
            """
            Initialize the helper.
            """
            self.id = req_id
            self.callback = callback

    class FakeResponse:
        """
        Collect fake response tests.
        """

        def __init__(self, correlation_id: object) -> None:
            """
            Initialize the helper.
            """
            self.correlation_id = correlation_id

    req_id = UUID4()
    request = FakeRequest(req_id, callback_msgs.append)

    bus.request("service", request)
    assert bus.req_count == 1
    assert bus.is_pending_request(req_id)
    assert len(endpoint_msgs) == 1

    response = FakeResponse(req_id)
    bus.response(response)
    assert bus.res_count == 1
    assert not bus.is_pending_request(req_id)
    assert len(callback_msgs) == 1


def test_duplicate_request_id_rejected(bus: object) -> None:
    """
    Test duplicate request id rejected.
    """
    endpoint_msgs = []
    bus.register("service", endpoint_msgs.append)

    class FakeRequest:
        """
        Collect fake request tests.
        """

        def __init__(self, req_id: object, callback: object) -> None:
            """
            Initialize the helper.
            """
            self.id = req_id
            self.callback = callback

    req_id = UUID4()
    req = FakeRequest(req_id, [].append)

    bus.request("service", req)
    assert bus.req_count == 1

    bus.request("service", req)
    assert bus.req_count == 1


def test_is_pending_request_false_when_empty(bus: object) -> None:
    """
    Test is pending request false when empty.
    """
    assert not bus.is_pending_request(UUID4())


INVALID_ENDPOINTS = [
    pytest.param("", "was empty", id="empty"),
    pytest.param("   ", "was all whitespace", id="spaces"),
    pytest.param("\t\n", "was all whitespace", id="tab-newline"),
    pytest.param("*", "contained invalid characters", id="star"),
    pytest.param("mailbox.*", "contained invalid characters", id="trailing-star"),
    pytest.param("mail?ox", "contained invalid characters", id="question-mark"),
]


@pytest.mark.parametrize(("endpoint", "message"), INVALID_ENDPOINTS)
def test_register_invalid_endpoint_raises(bus: object, endpoint: object, message: object) -> None:
    """
    Test register invalid endpoint raises.
    """
    with pytest.raises(ValueError, match=message) as exc_info:
        bus.register(endpoint, [].append)

    assert type(exc_info.value) is ValueError
    assert bus.endpoints() == []


@pytest.mark.parametrize(("endpoint", "message"), INVALID_ENDPOINTS)
def test_deregister_invalid_endpoint_raises(bus: object, endpoint: object, message: object) -> None:
    """
    Test deregister invalid endpoint raises.
    """
    handler = [].append
    bus.register("mailbox", handler)

    with pytest.raises(ValueError, match=message) as exc_info:
        bus.deregister(endpoint, handler)

    assert type(exc_info.value) is ValueError
    assert bus.endpoints() == ["mailbox"]


@pytest.mark.parametrize(("endpoint", "message"), INVALID_ENDPOINTS)
def test_send_invalid_endpoint_raises(bus: object, endpoint: object, message: object) -> None:
    """
    Test send invalid endpoint raises.
    """
    received = []
    bus.register("mailbox", received.append)

    with pytest.raises(ValueError, match=message) as exc_info:
        bus.send(endpoint, "msg")

    assert type(exc_info.value) is ValueError
    assert bus.sent_count == 0
    assert received == []


@pytest.mark.parametrize(("endpoint", "message"), INVALID_ENDPOINTS)
def test_request_invalid_endpoint_raises(bus: object, endpoint: object, message: object) -> None:
    """
    Test request invalid endpoint raises.
    """

    class FakeRequest:
        """
        Collect fake request tests.
        """

        def __init__(self, req_id: object, callback: object) -> None:
            """
            Initialize the helper.
            """
            self.id = req_id
            self.callback = callback

    received = []
    bus.register("service", received.append)
    req_id = UUID4()

    with pytest.raises(ValueError, match=message) as exc_info:
        bus.request(endpoint, FakeRequest(req_id, [].append))

    assert type(exc_info.value) is ValueError
    assert bus.req_count == 0
    assert not bus.is_pending_request(req_id)
    assert received == []


def test_register_validates_endpoint_before_building_handler(bus: object) -> None:
    """
    Test register validates endpoint before building handler.
    """

    class ExplodingRepr:
        """
        Collect exploding repr tests.
        """

        def __repr__(self) -> str:
            """
            Repr.
            """
            raise AssertionError("handler must not be built for an invalid endpoint")

        def __call__(self, msg: object) -> None:
            """
            Call.
            """

    with pytest.raises(ValueError, match="was empty") as exc_info:
        bus.register("", ExplodingRepr())

    assert type(exc_info.value) is ValueError
    assert bus.endpoints() == []


def test_request_validates_endpoint_before_reading_request(bus: object) -> None:
    """
    Test request validates endpoint before reading request.
    """

    class RequestWithoutId:
        """
        Collect request without id tests.
        """

    with pytest.raises(ValueError, match="was empty") as exc_info:
        bus.request("", RequestWithoutId())

    assert type(exc_info.value) is ValueError
    assert bus.req_count == 0


INVALID_PATTERNS = [
    pytest.param("", "was empty", id="empty"),
    pytest.param("   ", "was all whitespace", id="spaces"),
    pytest.param("\t\n", "was all whitespace", id="tab-newline"),
]


@pytest.mark.parametrize(("pattern", "message"), INVALID_PATTERNS)
def test_subscribe_invalid_pattern_raises(bus: object, pattern: object, message: object) -> None:
    """
    Test subscribe invalid pattern raises.
    """
    with pytest.raises(ValueError, match=message) as exc_info:
        bus.subscribe(pattern, [].append)

    assert type(exc_info.value) is ValueError
    assert bus.topics() == []
    assert bus.subscriptions() == []


@pytest.mark.parametrize(("pattern", "message"), INVALID_PATTERNS)
def test_unsubscribe_invalid_pattern_raises(bus: object, pattern: object, message: object) -> None:
    """
    Test unsubscribe invalid pattern raises.
    """
    handler = [].append
    bus.subscribe("system", handler)

    with pytest.raises(ValueError, match=message) as exc_info:
        bus.unsubscribe(pattern, handler)

    assert type(exc_info.value) is ValueError
    assert bus.topics() == ["system"]


@pytest.mark.parametrize(("pattern", "message"), INVALID_PATTERNS)
def test_is_subscribed_invalid_pattern_raises(
    bus: object,
    pattern: object,
    message: object,
) -> None:
    """
    Test is subscribed invalid pattern raises.
    """
    with pytest.raises(ValueError, match=message) as exc_info:
        bus.is_subscribed(pattern, [].append)

    assert type(exc_info.value) is ValueError


@pytest.mark.parametrize(("pattern", "message"), INVALID_PATTERNS)
def test_subscriptions_invalid_pattern_raises(
    bus: object,
    pattern: object,
    message: object,
) -> None:
    """
    Test subscriptions invalid pattern raises.
    """
    with pytest.raises(ValueError, match=message) as exc_info:
        bus.subscriptions(pattern)

    assert type(exc_info.value) is ValueError


@pytest.mark.parametrize(("pattern", "message"), INVALID_PATTERNS)
def test_has_subscribers_invalid_pattern_raises(
    bus: object,
    pattern: object,
    message: object,
) -> None:
    """
    Test has subscribers invalid pattern raises.
    """
    with pytest.raises(ValueError, match=message) as exc_info:
        bus.has_subscribers(pattern)

    assert type(exc_info.value) is ValueError


def test_subscribe_validates_pattern_before_building_handler(bus: object) -> None:
    """
    Test subscribe validates pattern before building handler.
    """

    class ExplodingRepr:
        """
        Collect exploding repr tests.
        """

        def __repr__(self) -> str:
            """
            Repr.
            """
            raise AssertionError("handler must not be built for an invalid pattern")

        def __call__(self, msg: object) -> None:
            """
            Call.
            """

    with pytest.raises(ValueError, match="was empty") as exc_info:
        bus.subscribe("", ExplodingRepr())

    assert type(exc_info.value) is ValueError
    assert bus.subscriptions() == []


@pytest.mark.parametrize("pattern", ["*", "data.*", "test.?", "a?b", "data.*.BINANCE.ETH*"])
def test_subscribe_accepts_wildcard_patterns(bus: object, pattern: object) -> None:
    """
    Test subscribe accepts wildcard patterns.
    """
    bus.subscribe(pattern, [].append)

    assert bus.topics() == [pattern]
    assert bus.has_subscribers(pattern)


def test_subscriptions_entries_report_topic_and_handler(bus: object) -> None:
    """
    Test subscriptions entries report topic and handler.
    """

    def handler(msg: object) -> object:
        """
        Handle the callback.
        """
        return msg

    bus.subscribe("data.quotes", handler)
    bus.subscribe("events.order", handler)

    expected_quotes = f"Subscription(topic=data.quotes, handler={handler!r})"
    expected_order = f"Subscription(topic=events.order, handler={handler!r})"

    assert sorted(bus.subscriptions()) == sorted([expected_quotes, expected_order])
    assert bus.subscriptions("data.*") == [expected_quotes]


def test_streaming_type_registration(bus: object) -> None:
    """
    Test streaming type registration.
    """
    assert not bus.is_streaming_type(int)
    bus.add_streaming_type(int)
    assert bus.is_streaming_type(int)
    assert int in bus.streaming_types()


def test_streaming_type_not_confused_with_other_types(bus: object) -> None:
    """
    Test streaming type not confused with other types.
    """
    bus.add_streaming_type(int)
    assert not bus.is_streaming_type(str)
    assert not bus.is_streaming_type(float)


def test_add_listener_receives_published_bytes(bus: object) -> None:
    """
    Test add listener receives published bytes.
    """
    events = []

    class DummyListener:
        """
        Collect dummy listener tests.
        """

        def is_closed(self) -> bool:
            """
            Is closed.
            """
            return False

        def publish(self, topic: object, payload: object) -> None:
            """
            Publish.
            """
            events.append((topic, payload))

    bus.add_listener(DummyListener())
    bus.publish("any.topic", b"data")
    assert events == [("any.topic", b"data")]


def test_add_listener_skips_closed(bus: object) -> None:
    """
    Test add listener skips closed.
    """
    events = []

    class DummyListener:
        """
        Collect dummy listener tests.
        """

        def __init__(self, closed: object = False) -> None:
            """
            Initialize the helper.
            """
            self._closed = closed

        def is_closed(self) -> object:
            """
            Is closed.
            """
            return self._closed

        def publish(self, topic: object, payload: object) -> None:
            """
            Publish.
            """
            events.append((topic, payload))

    bus.add_listener(DummyListener(closed=True))
    bus.publish("any.topic", b"data")
    assert events == []


def test_has_subscribers_with_wildcard_pattern(bus: object) -> None:
    """
    Test has subscribers with wildcard pattern.
    """
    bus.subscribe("data.instrument.SIM.*", [].append)
    assert bus.has_subscribers("data.instrument.*")
    assert not bus.has_subscribers("events.*")


def test_dispose_clears_state(bus: object) -> None:
    """
    Test dispose clears state.
    """
    bus.subscribe("topic", [].append)
    bus.register("ep", [].append)
    bus.dispose()
    assert bus.endpoints() == []
    assert bus.topics() == []
    assert bus.subscriptions() == []


def test_dispose_clears_correlation_index(bus: object) -> None:
    """
    Test dispose clears correlation index.
    """

    class FakeRequest:
        """
        Collect fake request tests.
        """

        def __init__(self) -> None:
            """
            Initialize the helper.
            """
            self.id = UUID4()
            self.callback = lambda r: None

    bus.register("ep", [].append)
    bus.request("ep", FakeRequest())
    bus.dispose()
    assert bus.req_count == 1
