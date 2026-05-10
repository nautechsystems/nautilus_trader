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

import pytest

from nautilus_trader.common import MessageBus
from nautilus_trader.core import UUID4
from nautilus_trader.model import TraderId


@pytest.fixture
def trader_id():
    return TraderId.from_str("TRADER-001")


@pytest.fixture
def bus(trader_id):
    return MessageBus(trader_id=trader_id)


def test_instantiate_defaults(bus, trader_id):
    assert bus.trader_id == trader_id
    assert bus.name == "MessageBus"
    assert bus.has_backing is False
    assert bus.sent_count == 0
    assert bus.req_count == 0
    assert bus.res_count == 0
    assert bus.pub_count == 0


def test_instantiate_with_custom_name(trader_id):
    bus = MessageBus(trader_id=trader_id, name="CustomBus")
    assert bus.name == "CustomBus"


def test_endpoints_empty(bus):
    assert bus.endpoints() == []


def test_register_adds_endpoint(bus):
    bus.register("mailbox", [].append)
    assert bus.endpoints() == ["mailbox"]


def test_deregister_removes_endpoint(bus):
    handler = [].append
    bus.register("mailbox", handler)
    bus.deregister("mailbox", handler)
    assert bus.endpoints() == []


def test_send_delivers_to_endpoint(bus):
    received = []
    bus.register("mailbox", received.append)
    bus.send("mailbox", "msg")
    assert received == ["msg"]
    assert bus.sent_count == 1


def test_send_no_endpoint_increments_count(bus):
    bus.send("nowhere", "msg")
    assert bus.sent_count == 1


def test_send_multiple_increments_count(bus):
    received = []
    bus.register("ep", received.append)
    bus.send("ep", "a")
    bus.send("ep", "b")
    assert received == ["a", "b"]
    assert bus.sent_count == 2


def test_topics_empty(bus):
    assert bus.topics() == []


def test_subscriptions_empty(bus):
    assert bus.subscriptions() == []


def test_has_subscribers_false_when_empty(bus):
    assert not bus.has_subscribers()


def test_subscriptions_with_pattern_filter(bus):
    bus.subscribe("data.quotes.*", [].append)
    bus.subscribe("data.trades.*", [].append)
    bus.subscribe("events.order.*", [].append)

    data_subs = bus.subscriptions("data.*")
    all_subs = bus.subscriptions()

    assert len(data_subs) == 2
    assert len(all_subs) == 3


def test_has_subscribers_with_pattern(bus):
    bus.subscribe("data.quotes.BINANCE", [].append)
    assert bus.has_subscribers("data.quotes.BINANCE")
    assert not bus.has_subscribers("events.order.S1")


def test_subscribe_adds_topic(bus):
    bus.subscribe("system", [].append)
    assert "system" in bus.topics()


def test_subscribe_shows_has_subscribers(bus):
    bus.subscribe("events.*", [].append)
    assert bus.has_subscribers()


def test_subscribe_duplicate_ignored(bus):
    handler = [].append
    bus.subscribe("a", handler)
    bus.subscribe("a", handler)
    assert len(bus.subscriptions()) == 1


def test_unsubscribe_removes_subscription(bus):
    handler = [].append
    bus.subscribe("events.order*", handler)
    bus.unsubscribe("events.order*", handler)
    assert bus.subscriptions() == []


def test_unsubscribe_nonexistent_does_nothing(bus):
    bus.unsubscribe("missing", [].append)
    assert bus.subscriptions() == []


def test_is_subscribed_lifecycle(bus):
    def handler(msg):
        return msg

    assert not bus.is_subscribed("topic.test", handler)
    bus.subscribe("topic.test", handler)
    assert bus.is_subscribed("topic.test", handler)
    bus.unsubscribe("topic.test", handler)
    assert not bus.is_subscribed("topic.test", handler)


def test_publish_with_no_subscribers(bus):
    bus.publish("empty.topic", "hello")
    assert bus.pub_count == 1


def test_publish_delivers_to_subscriber(bus):
    received = []
    bus.subscribe("system", received.append)
    bus.publish("system", "hello")
    assert received == ["hello"]
    assert bus.pub_count == 1


def test_publish_delivers_to_multiple_subscribers(bus):
    sub1, sub2, sub3 = [], [], []
    bus.subscribe("system", sub1.append)
    bus.subscribe("system", sub2.append)
    bus.subscribe("system", sub3.append)
    bus.publish("system", "hello")
    assert sub1 == ["hello"]
    assert sub2 == ["hello"]
    assert sub3 == ["hello"]
    assert bus.pub_count == 1


def test_publish_wildcard_star(bus):
    received = []
    bus.subscribe("events.order*", received.append)
    bus.publish("events.order.SCALPER-001", "ORDER")
    assert received == ["ORDER"]


def test_publish_no_match_filters(bus):
    received = []
    bus.subscribe("events.position*", received.append)
    bus.publish("events.order.S-001", "ORDER")
    assert received == []


def test_publish_star_catches_all(bus):
    all_msgs = []
    specific = []
    bus.subscribe("*", all_msgs.append)
    bus.subscribe("MyTopic", specific.append)
    bus.publish("MyTopic", "OK!")
    assert specific == ["OK!"]
    assert all_msgs == ["OK!"]


def test_publish_question_mark_pattern(bus):
    received = []
    bus.subscribe("test.?", received.append)
    bus.publish("test.a", "ok1")
    bus.publish("test.1", "ok2")
    bus.publish("test.12", "nope")
    assert received == ["ok1", "ok2"]


def test_publish_combined_wildcards(bus):
    received = []
    bus.subscribe("data.*.BINANCE.ETH*", received.append)
    bus.publish("data.trades.BINANCE.ETHUSDT", "t1")
    bus.publish("data.quotes.BINANCE.ETHUSDT", "q1")
    bus.publish("data.trades.BINANCE.BTCUSDT", "nope")
    assert received == ["t1", "q1"]


def test_publish_late_subscribe(bus):
    received = []
    bus.publish("events.order.S-001", "early")
    bus.subscribe("events.order*", received.append)
    bus.publish("events.order.S-001", "late")
    assert received == ["late"]
    assert bus.pub_count == 2


def test_publish_priority_order(bus):
    order = []

    def low(msg):
        order.append(f"low-{msg}")

    def high(msg):
        order.append(f"high-{msg}")

    bus.subscribe("orders", low, priority=0)
    bus.subscribe("orders", high, priority=10)
    bus.publish("orders", "123")
    assert order == ["high-123", "low-123"]


def test_publish_python_objects(bus):
    received = []
    bus.subscribe("data", received.append)
    obj = {"key": [1, 2, 3], "nested": {"a": True}}
    bus.publish("data", obj)
    assert received == [obj]
    assert received[0] is obj


def test_request_response_round_trip(bus):
    endpoint_msgs = []
    callback_msgs = []

    bus.register("service", endpoint_msgs.append)

    class FakeRequest:
        def __init__(self, req_id, callback):
            self.id = req_id
            self.callback = callback

    class FakeResponse:
        def __init__(self, correlation_id):
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


def test_duplicate_request_id_rejected(bus):
    endpoint_msgs = []
    bus.register("service", endpoint_msgs.append)

    class FakeRequest:
        def __init__(self, req_id, callback):
            self.id = req_id
            self.callback = callback

    req_id = UUID4()
    req = FakeRequest(req_id, [].append)

    bus.request("service", req)
    assert bus.req_count == 1

    bus.request("service", req)
    assert bus.req_count == 1


def test_is_pending_request_false_when_empty(bus):
    assert not bus.is_pending_request(UUID4())


def test_streaming_type_registration(bus):
    assert not bus.is_streaming_type(int)
    bus.add_streaming_type(int)
    assert bus.is_streaming_type(int)
    assert int in bus.streaming_types()


def test_streaming_type_not_confused_with_other_types(bus):
    bus.add_streaming_type(int)
    assert not bus.is_streaming_type(str)
    assert not bus.is_streaming_type(float)


def test_add_listener_receives_published_bytes(bus):
    events = []

    class DummyListener:
        def is_closed(self):
            return False

        def publish(self, topic, payload):
            events.append((topic, payload))

    bus.add_listener(DummyListener())
    bus.publish("any.topic", b"data")
    assert events == [("any.topic", b"data")]


def test_add_listener_skips_closed(bus):
    events = []

    class DummyListener:
        def __init__(self, closed=False):
            self._closed = closed

        def is_closed(self):
            return self._closed

        def publish(self, topic, payload):
            events.append((topic, payload))

    bus.add_listener(DummyListener(closed=True))
    bus.publish("any.topic", b"data")
    assert events == []


def test_has_subscribers_with_wildcard_pattern(bus):
    bus.subscribe("data.instrument.SIM.*", [].append)
    assert bus.has_subscribers("data.instrument.*")
    assert not bus.has_subscribers("events.*")


def test_dispose_clears_state(bus):
    bus.subscribe("topic", [].append)
    bus.register("ep", [].append)
    bus.dispose()
    assert bus.endpoints() == []
    assert bus.topics() == []
    assert bus.subscriptions() == []


def test_dispose_clears_correlation_index(bus):
    class FakeRequest:
        def __init__(self):
            self.id = UUID4()
            self.callback = lambda r: None

    bus.register("ep", [].append)
    bus.request("ep", FakeRequest())
    bus.dispose()
    assert bus.req_count == 1
